//! Interrupt handler for "background" (i.e. lower priority than the timing
//! interrupts) rasterization.

use stm32f4::stm32f407 as device;

use core::sync::atomic::Ordering;

use crate::util::spin_lock::SpinLock;
use crate::{WorkingBuffer, WORKING_BUFFER_SIZE, vert_state, acquire_hw, HPSHARE, NextTransfer, TIMING, LINE, RASTER};
use crate::timing::{Timing, MIN_CYCLES_PER_PIXEL};
use crate::rast::{RasterCtx, TargetBuffer};

/// State used by the raster maintenance (PendSV) ISR.
struct RasterState {
    /// Rasterization working buffer in closely-coupled RAM. During
    /// rasterization, the CPU can scribble into this buffer freely without
    /// interfering with any ongoing DMA transfer.
    working_buffer: &'static mut WorkingBuffer,

    /// Flag indicating that a new scanline has been written to `working_buffer`
    /// since the last scan buffer update, and so data needs to be copied.
    update_scan_buffer: bool,

    /// Rasterizer parameters for the contents of `working_buffer`.
    raster_ctx: RasterCtx,
}

static mut RASTER_STATE: SpinLock<RasterState> = SpinLock::new(RasterState {
    working_buffer: unsafe { &mut GLOBAL_WORKING_BUFFER },
    update_scan_buffer: false,
    raster_ctx: RasterCtx {
        cycles_per_pixel: 4,
        repeat_lines: 0,
        target_range: 0..0,
    },
});

/// Rasterization working buffer in closely-coupled RAM. During rasterization,
/// the CPU can scribble into this buffer freely without interfering with any
/// ongoing DMA transfer.
///
/// A reference to this is stashed in `RASTER_STATE`; you don't want to touch
/// this directly. (I'd make it an anonymous array but that would prevent me
/// from using `link_section`.)
#[link_section = ".local_ram"]
static mut GLOBAL_WORKING_BUFFER: WorkingBuffer = [0; WORKING_BUFFER_SIZE];

/// Casting from a word-aligned working buffer to a byte-aligned pixel buffer.
fn working_buffer_as_u8(words: &mut WorkingBuffer) -> &mut TargetBuffer {
    // Safety: these structures have exactly the same shape, and when we use it
    // as a buffer of u32, we're doing it only as a fast way of moving u8. The
    // main portability risk here is endianness, but so be it.
    unsafe {
        core::mem::transmute(words)
    }
}


/// Rasterization scanout buffer in the smaller AHB-attached SRAM. This is the
/// source for scanout DMA.
///
/// Written in PendSV by the `copy_words` routine, which transfers data from
/// `GLOBAL_WORKING_BUFFER`.
///
/// Read asynchronously by DMA during scanout.
#[link_section = ".scanout_ram"]
static mut GLOBAL_SCANOUT_BUFFER: WorkingBuffer = [0; WORKING_BUFFER_SIZE];


/// Entry point for the raster maintenance ISR, invoked as PendSV.
pub fn maintain_raster_isr() {
    // Safety: RASTER_STATE is mut only because rustc is really picky about
    // seeing uses of mut statics like GLOBAL_WORKING_BUFFER in the initializers
    // of non-mut statics.
    let mut state = unsafe { RASTER_STATE.try_lock() }.expect("pendsv state");

    let vs = vert_state();

    // First, prepare for scanout from SAV on this line.  This has two purposes:
    // it frees up the rasterization target buffer so that we can overwrite it,
    // and it applies pixel timing choices from the *last* rasterizer run to the
    // scanout machine so that we can replace them as well.
    //
    // This writes to the scanout buffer *and* accesses AHB/APB peripherals, so
    // it *cannot* run concurrently with scanot -- so we do it first, during
    // hblank.
    if vs.is_displayed_state() {
        {
            #[cfg(feature = "measurement")]
            crate::measurement::sig_b_set();
            let mut share = acquire_hw(&HPSHARE);
            let hw = &share.hw;
            let (dma_cr, use_timer) = {
                prepare_for_scanout(
                    &hw.dma2,
                    &hw.tim1,
                    &state.raster_ctx,
                )
            };
            // [OX] TODO: omg there is no actual way to get the bits out of this
            let dma_cr_bits = unsafe { core::mem::transmute(dma_cr) };

            // Note: we are now racing hstate SAV for control of this lock.
            share.xfer = NextTransfer {
                dma_cr_bits, 
                use_timer,
            };
            #[cfg(feature = "measurement")]
            crate::measurement::sig_b_clear();
        }
        if state.update_scan_buffer {
            update_scan_buffer(
                state.raster_ctx.target_range.end,
                &mut state.working_buffer, 
            );
        }
    }

    // Allow the application to do additional work during what's left of hblank.
    //vga_hblank_interrupt();

    // Second, rasterize the *next* line, if there's a useful next line.
    // Rasterization can take a while, and may run concurrently with scanout.
    // As a result, we just stash our results in places where the *next* PendSV
    // will find and apply them.
    if vs.is_rendered_state() {
        #[cfg(feature = "measurement")]
        crate::measurement::sig_b_set();

        let state = &mut *state;

        // Hold the TIMING lock for as short as possible.
        let Timing { add_cycles_per_pixel, video_start_line, ..} =
            *TIMING.try_lock().expect("pendsv timing").as_mut().unwrap();

        state.update_scan_buffer = rasterize_next_line(
            add_cycles_per_pixel + MIN_CYCLES_PER_PIXEL,
            video_start_line,
            &mut state.raster_ctx,
            &mut state.working_buffer,
        );
        #[cfg(feature = "measurement")]
        crate::measurement::sig_b_clear();
    }
}

/// Copy the first `len_bytes` of `working` into the global scanout buffer for
/// DMA.
fn update_scan_buffer(len_bytes: usize,
                      working: &mut WorkingBuffer) {
    if len_bytes > 0 {
        // We're going to move words, so round up to find the number of words to
        // move.
        //
        // Note: the user could pass in any value for `len_bytes`, but it will
        // get bounds-checked below when used as a slice index.
        let count = (len_bytes + 3) / 4;

        let scan = unsafe { &mut GLOBAL_SCANOUT_BUFFER };

        crate::copy_words::copy_words(
            &working[..count],
            &mut scan[..count],
        );

        // Terminate with a word of black, to ensure that outputs return to
        // black level for hblank.
        scan[count] = 0;
    }
}

/// Sets up the scanout configuration. This is done well in advance of the
/// actual start of scanout.
///
/// This returns the two pieces of information that are needed to trigger
/// scanout the rest of the way: a CR value and a flag indicating whether the
/// scanout will use a timer-generated DRQ (`true`) or run at full speed
/// (`false`).
#[must_use = "scanout parameters are returned, not set globally"]
fn prepare_for_scanout(dma: &device::DMA2,
                       vtimer: &device::tim1::RegisterBlock,
                       ctx: &RasterCtx)
    -> (device::dma2::s5cr::W, bool)
{
    // Shut off the DMA stream for reconfiguration. This is a little
    // belt-and-suspenders.
    dma.s5cr.modify(|_, w| w.en().clear_bit());

    // TODO adjust this
    const DRQ_SHIFT_CYCLES: u32 = 2;

    // TODO oxidation note: okay, the svd2rust register access operations
    // are incredibly awkward for code like this. I'm really really tempted
    // to fork it.
    //
    // The issue: I want to create a *dynamic* value to load into the DMA
    // stream CR register *ahead of time*. And I want to do it without bit
    // shifting and error-prone offset constants.
    //
    // Notes below tagged with [OX].

    // [OX] so we can't have a dma_xfer_common constant like the C++ does,
    // because *none* of the register manipulation operations are const.
    // Instead, let's roll one here and hope the compiler figures out how to
    // optimize it :-(

    // Build the CR value we'll use to start the transfer, so that we don't
    // have to do it then -- which would increase IRQ-to-transfer latency.

    // [OX] First: all of this code is statically specialized for stream 5.
    // That's ridiculous. All streams are identical. But look at the type:
    let mut xfer = device::dma2::s5cr::W::reset_value();
    // [OX] can't do these alterations in the same line as the declaration
    // above, because they pass references around instead of W being a
    // simple value type, and it's thus treated as a temporary and freed.
    xfer
        .chsel().bits(6)
        .pl().very_high()
        .pburst().single()
        .mburst().single()
        .en().enabled();

    let length = ctx.target_range.end - ctx.target_range.start;

    if ctx.cycles_per_pixel > 4 {
        // Adjust reload frequency of TIM1 to accomodate desired pixel clock.
        // (ARR value is period - 1.)
        let reload = (ctx.cycles_per_pixel - 1) as u32;
        vtimer.arr.write(|w| unsafe {
            // TODO: ARR unsafe? BUG
            w.bits(reload)
        });
        // Force an update to reset the timer state.
        vtimer.egr.write(|w| w.ug().set_bit());
        // Configure the timer as *almost* ready to produce a DRQ, less a small
        // value (fudge factor).  Gotta do this after the update event, above,
        // because that clears CNT.
        vtimer.cnt.write(|w| unsafe {
            w.bits(reload - DRQ_SHIFT_CYCLES)
        });
        vtimer.sr.reset();

        dma.s5par.write(|w| unsafe {
            // Okay, this is legitimately unsafe. ;-)
            w.bits(0x40021015)  // High byte of GPIOE ODR (hack hack)
        });
        dma.s5m0ar.write(|w| unsafe {
            w.bits(&GLOBAL_SCANOUT_BUFFER as *const u32 as u32)
        });

        // The number of bytes read must exactly match the number of bytes
        // written, or the DMA controller will freak out.  Thus, we must adapt
        // the transfer size to the number of bytes transferred. The padding in
        // each case is because we want to send (at least) one byte of black
        // after any scanline.
        match length & 3 {
            0 => {
                xfer.msize().word();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 + 4));
            }
            2 => {
                xfer.msize().half_word();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 + 2));
            }
            _ => {
                xfer.msize().byte();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 + 1));
            }
        }

        xfer.dir().memory_to_peripheral()
            .minc().set_bit()
            .psize().byte()
            .pinc().clear_bit();
       
        (xfer, true)
    } else {
        // Note that we're using memory as the peripheral side.
        // This DMA controller is a little odd.
        dma.s5par.write(|w| unsafe {
            w.bits(&GLOBAL_SCANOUT_BUFFER as *const u32 as u32)
        });
        dma.s5m0ar.write(|w| unsafe {
            // Okay, this is legitimately unsafe. ;-)
            w.bits(0x40021015)  // High byte of GPIOE ODR (hack hack)
        });

        // The number of bytes read must exactly match the number of bytes
        // written, or the DMA controller will freak out.  Thus, we must adapt
        // the transfer size to the number of bytes transferred. The padding in
        // each case is because we want to send (at least) one byte of black
        // after any scanline.
        match length & 3 {
            0 => {
                xfer.psize().word();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 / 4 + 1));
            }
            2 => {
                xfer.psize().half_word();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 / 2 + 1));
            }
            _ => {
                xfer.psize().byte();
                dma.s5ndtr.write(|w| w.ndt().bits(length as u16 + 1));
            }
        }

        xfer
            .dir().memory_to_memory()
            .pinc().set_bit()
            .msize().byte()
            .minc().clear_bit();

        (xfer, false)
    }
}

#[must_use = "the update flag is pretty important"]
fn rasterize_next_line(cycles_per_pixel: usize,
                       video_start_line: usize,
                       ctx: &mut RasterCtx,
                       working: &mut WorkingBuffer)
    -> bool
{
    let current_line = LINE.load(Ordering::Relaxed);
    let next_line = current_line + 1;
    let visible_line = next_line - video_start_line;

    if ctx.repeat_lines == 0 {
        // Set up a default context for the rasterizer to alter if desired.
        *ctx = RasterCtx {
            cycles_per_pixel,
            repeat_lines: 0,
            target_range: 0..0,
        };
        // Invoke the rasterizer.
        RASTER.observe(|r| r(
                visible_line,
                working_buffer_as_u8(working),
                ctx,
        )).expect("raster observe");
        true
    } else {  // repeat_lines > 0
        ctx.repeat_lines -= 1;
        false
    }
}
