pub mod rast;

pub mod hstate;

use stm32f4::stm32f407 as device;
use cortex_m::peripheral as cm;

use cortex_m::peripheral::scb::SystemHandler;

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::ptr::NonNull;

use crate::armv7m::*;
use crate::stm32::copy_interrupt; // grrrr
use crate::util::spin_lock::{SpinLock, SpinLockGuard};

use self::rast::{RasterCtx, TargetBuffer};

pub type Pixel = u8;

pub const MAX_PIXELS_PER_LINE: usize = 800;

const SHOCK_ABSORBER_SHIFT_CYCLES: u32 = 20;

struct HStateHw {
    dma2: device::DMA2,
    tim1: device::TIM1,
    tim4: device::TIM4,
    gpiob: device::GPIOB,
}

static HSTATE_HW: SpinLock<Option<HStateHw>> = SpinLock::new(None);

/// Records when a driver instance has been initialized. This is only allowed to
/// happen once at the moment because we don't have perfect teardown code.
static mut DRIVER_INIT_FLAG: AtomicBool = AtomicBool::new(false);

/// Equivalent of `rast::TargetBuffer`, but as words to ensure alignment for
/// certain of our high-throughput routines.
type WorkingBuffer = [u32; WORKING_BUFFER_SIZE];
const WORKING_BUFFER_SIZE: usize = rast::TARGET_BUFFER_SIZE / 4;

/// Casting from a word-aligned working buffer to a byte-aligned pixel buffer.
fn working_buffer_as_u8(words: &mut WorkingBuffer) -> &mut rast::TargetBuffer {
    // Safety: these structures have exactly the same shape, and when we use it
    // as a buffer of u32, we're doing it only as a fast way of moving u8. The
    // main portability risk here is endianness, but so be it.
    unsafe {
        core::mem::transmute(words)
    }
}

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

/// Rasterization scanout buffer in the smaller AHB-attached SRAM. This is the
/// source for scanout DMA.
///
/// Written in PendSV by the `copy_words` routine, which transfers data from
/// `GLOBAL_WORKING_BUFFER`.
///
/// Read asynchronously by DMA during scanout.
#[link_section = ".scanout_ram"]
static mut GLOBAL_SCANOUT_BUFFER: WorkingBuffer = [0; WORKING_BUFFER_SIZE];

/// Groups parameters passed from PendSV to HState ISR, describing the next DMA
/// transfer.
struct NextTransfer {
    /// Bitwise representation of the DMA SxCR register value that starts the
    /// transfer.
    ///
    /// TODO: this should probably be `device::dma2::s5cr::W`, but that type has
    /// no `const` constructor for use in a static, and is generally just a pain
    /// in the ass to work with, so `u32` it is.
    dma_cr_bits: u32,
    /// Whether to use timer-mediated DMA to decrease horizontal resolution.
    use_timer: bool,
}

/// Parameters passed from PendSV to HState ISR.
static NEXT_XFER: SpinLock<NextTransfer> = SpinLock::new(NextTransfer {
    dma_cr_bits: 0,
    use_timer: false,
});

/// Description of the last render into the working buffer.
///
/// Written:
/// - At PendSV when rasterization completes.
///
/// Read:
/// - At PendSV to determine how much data to move from working to scanout.
/// - At PendSV to configure next DMA transfer.
/// - At PendSV to decide whether to repeat or re-rasterize.
static LAST_RENDER: SpinLock<RasterCtx> = SpinLock::new(RasterCtx {
    cycles_per_pixel: 4,
    repeat_lines: 0,
    target_range: 0..0,
});

#[derive(Copy, Clone, Debug)]
pub struct Timing {
    clock_config: ClockConfig,

    /// Number of additional AHB cycles per pixel clock cycle. The driver uses a
    /// minimum of 4 cycles per pixel; this field adds to that. Larger values
    /// reduce both the resolution and the compute/bandwidth requirements.
    add_cycles_per_pixel: usize,

    /// Total horizontal pixels per line, including blanking.
    line_pixels: usize,
    /// Length of horizontal sync pulse.
    sync_pixels: usize,
    /// Number of pixels between end of sync and start of video.
    back_porch_pixels: usize,
    /// Fudge factor, nudging the DMA interrupt backwards in time to compensate
    /// for startup code taking non-zero time.
    video_lead: usize,
    /// Maximum visible pixels per line.
    video_pixels: usize,
    /// Polarity of horizontal sync pulse.
    hsync_polarity: Polarity,

    /// Scanline number of onset of vertical sync pulse, numbered from end of
    /// active video.
    vsync_start_line: usize,
    /// Scanline number of end of vertical sync pulse, numbered from end of
    /// active video.
    vsync_end_line: usize,
    /// Scanline number of start of active video, numbered from end of active
    /// video.
    video_start_line: usize,
    /// Scanline number of end of active video, numbered from end of active
    /// video in the last frame. This is the number of lines per frame.
    video_end_line: usize,
    /// Polarity of the vertical sync pulse.
    vsync_polarity: Polarity,
}

const MIN_CYCLES_PER_PIXEL: usize = 4;

impl Timing {
    pub fn cycles_per_pixel(&self) -> usize {
        self.add_cycles_per_pixel + MIN_CYCLES_PER_PIXEL
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ClockConfig {
    crystal_hz: f32,
    crystal_divisor: usize,
    vco_multiplier: usize,
    general_divisor: usize,
    pll48_divisor: usize,

    ahb_divisor: usize,
    apb1_divisor: usize,
    apb2_divisor: usize,

    flash_latency: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Polarity {
    Positive = 0,
    Negative = 1,
}

pub struct Vga<MODE> {
    rcc: device::RCC,
    gpiob: device::GPIOB,
    gpioe: device::GPIOE,
    tim1: device::TIM1,
    tim3: device::TIM3,
    tim4: device::TIM4,
    dma2: device::DMA2,

    nvic: cm::NVIC,  // TODO probably should not own this

    _marker: PhantomData<MODE>,
}

#[derive(Debug)]
pub enum Idle {}

#[derive(Debug)]
pub enum Ready {}

static RASTER: rast::IRef = rast::IRef::new();

impl<T> Vga<T> {
    fn scan_buffer(&self) -> &'static mut [Pixel; rast::TARGET_BUFFER_SIZE] {
        unsafe {
            core::mem::transmute(&mut GLOBAL_SCANOUT_BUFFER)
        }
    }

    /// Busy-waits for the transition from active video to vertical blank.
    /// Because this waits for the *transition*, if you call this *during*
    /// vblank it will wait for an entire frame.
    pub fn sync_to_vblank(&self) {
        unimplemented!()
    }

    pub fn sync_on(&self) {
        // Configure PB6/PB7 for fairly sharp edges.
        self.gpiob.ospeedr.modify(|_, w| w
                                  .ospeedr6().high_speed()
                                  .ospeedr7().high_speed());
        // Disable pullups/pulldowns.
        self.gpiob.pupdr.modify(|_, w| w
                                .pupdr6().floating()
                                .pupdr7().floating());
        // Configure PB6 as AF2 and PB7 as output.
        self.gpiob.afrl.modify(|_, w| w.afrl6().af2());
        self.gpiob.moder.modify(|_, w| w
                                .moder6().alternate()
                                .moder7().output());
    }

    pub fn sync_off(&self) {
        self.gpiob.moder.modify(|_, w| w
                                .moder6().input()
                                .moder7().input());
        self.gpiob.pupdr.modify(|_, w| w
                                .pupdr6().pull_down()
                                .pupdr7().pull_down());
    }

    /// Disables video output. This is not synchronized and can happen in the
    /// middle of the frame; if that bothers you, synchronize with vblank.
    pub fn video_off(&self) {
        self.gpioe.pupdr.modify(|_, w| w
                                .pupdr8().pull_down()
                                .pupdr9().pull_down()
                                .pupdr10().pull_down()
                                .pupdr11().pull_down()
                                .pupdr12().pull_down()
                                .pupdr13().pull_down()
                                .pupdr14().pull_down()
                                .pupdr15().pull_down());
        self.gpioe.moder.modify(|_, w| w
                                .moder8().input()
                                .moder9().input()
                                .moder10().input()
                                .moder11().input()
                                .moder12().input()
                                .moder13().input()
                                .moder14().input()
                                .moder15().input());
    }
}

impl Vga<Idle> {
    /// Provides `rast` to the driver interrupt handler as the raster callback,
    /// and executes `scope`. When `scope` returns, `rast` is revoked. Note that
    /// this may require busy-waiting until the end of active video.
    ///
    /// During the execution of `scope` the application has access to the driver
    /// in a different state, `Vga<Ready>`, which exposes additional operations.
    pub fn with_raster<R>(&mut self,
                          mut rast: impl for<'c> FnMut(usize,
                                                       &'c mut TargetBuffer,
                                                       &'c mut RasterCtx)
                                         + Send,
                          scope: impl FnOnce(&mut Vga<Ready>) -> R)
        -> R
    {
        RASTER.donate(&mut rast, || {
            scope(unsafe { core::mem::transmute(self) })
        })
    }

    /// Configures video timing. This is only available when we aren't actively
    /// rasterizing.
    pub fn configure_timing(&mut self, timing: &Timing) {
        // TODO: timing consistency asserts

        self.video_off();
        self.sync_off();

        // Place the horizontal timers in reset, disabling interrupts.
        disable_h_timer(
            &mut self.nvic,
            &device::Interrupt::TIM4, 
            &self.rcc,
            |w| w.tim4rst().set_bit(),
        );
        disable_h_timer(
            &mut self.nvic,
            &device::Interrupt::TIM3,
            &self.rcc,
            |w| w.tim3rst().set_bit(),
        );

        // Busy-wait for pending DMA to complete.
        while self.dma2.s5cr.read().en().bit_is_set() {
            // busy wait
        }

        // Switch to new CPU clock settings.
        //rcc.configure_clocks(timing.clock_config);

        // Configure TIM3/4 for horizontal sync generation.
        configure_h_timer(
            timing,
            &mut self.nvic,
            &device::Interrupt::TIM3,
            &self.tim3,
            &self.rcc,
            |w| w.tim3en().set_bit(),
            |w| w.tim3rst().set_bit(),
        );
        configure_h_timer(
            timing,
            &mut self.nvic,
            &device::Interrupt::TIM4,
            &self.tim4,
            &self.rcc,
            |w| w.tim4en().set_bit(),
            |w| w.tim4rst().set_bit(),
        );

        // Adjust tim3's CC2 value back in time.
        self.tim3.ccr2.modify(|r, w| w.ccr2().bits(
                r.ccr2().bits() - SHOCK_ABSORBER_SHIFT_CYCLES));

        // Configure tim3 to distribute its enable signal as its trigger output.
        self.tim3.cr2.write(|w| w
                       .mms().enable()
                       .ccds().clear_bit());

        // Configure tim4 to trigger from tim3 and run forever.
        self.tim4.smcr.write(|w| unsafe {
            // BUG: TS and SMS contents not modeled, have to use raw bits
            w.ts().bits(0b10)
                .sms().bits(0b110)
        });


        // Turn on tim4's interrupts.
        self.tim4.dier.write(|w| w
                  .cc2ie().set_bit()    // Interrupt at start of active video.
                  .cc3ie().set_bit());  // Interrupt at end of active video.

        // Turn on only one of tim3's:
        // Interrupt at start of active video.
        self.tim3.dier.write(|w| w.cc2ie().set_bit());

        // Note: timers still not running.

        // Initialize vsync output to its starting state.
        match timing.vsync_polarity {
            Polarity::Positive => self.gpiob.bsrr.write(|w| w.br7().set_bit()),
            Polarity::Negative => self.gpiob.bsrr.write(|w| w.bs7().set_bit()),
        }

        /*
        TODO: since I've assigned ownership of the working buffer to the PendSV
        ISR, we can't do this here. I think this is okay, since it would only
        catch bugs in the very first rasterized scanline anyway.

        // Scribble over working buffer to help catch bugs.
        for pair in self.working_buffer().chunks_exact_mut(2) {
            pair[0] = 0xFF;
            pair[1] = 0x00;
        }
        */

        // Blank the final word of the scan buffer.
        for b in &mut self.scan_buffer()
                [timing.video_pixels .. timing.video_pixels + 4] {
            *b = 0
        }

        // Set up global state.
        LINE.store(0, Ordering::Relaxed);
        *TIMING.try_lock().unwrap() = Some(*timing);
        VERT_STATE.store(VState::Blank as usize, Ordering::Relaxed);

        // Start TIM3, which starts TIM4.
        enable_irq(&mut self.nvic, device::Interrupt::TIM3);
        enable_irq(&mut self.nvic, device::Interrupt::TIM4);
        self.tim3.cr1.modify(|_, w| w.cen().set_bit());

        self.sync_on();
    }
}

impl Vga<Ready> {
    /// Enables video output. This is not synchronized and can happen in the
    /// middle of the frame; if that bothers you, synchronize with vblank.
    pub fn video_on(&mut self) {
        // Disable pullups/pulldowns.
        self.gpioe.pupdr.modify(|_, w| w
                                .pupdr8().floating()
                                .pupdr9().floating()
                                .pupdr10().floating()
                                .pupdr11().floating()
                                .pupdr12().floating()
                                .pupdr13().floating()
                                .pupdr14().floating()
                                .pupdr15().floating());
        // Configure for very sharp edges. According to the reference manual
        // this sets the filter to 100MHz; at our 40MHz pixel clock this is an
        // improvement.
        self.gpioe.ospeedr.modify(|_, w| w
                                  .ospeedr8().very_high_speed()
                                  .ospeedr9().very_high_speed()
                                  .ospeedr10().very_high_speed()
                                  .ospeedr11().very_high_speed()
                                  .ospeedr12().very_high_speed()
                                  .ospeedr13().very_high_speed()
                                  .ospeedr14().very_high_speed()
                                  .ospeedr15().very_high_speed());
        // Configure for output.
        self.gpioe.moder.modify(|_, w| w
                                .moder8().output()
                                .moder9().output()
                                .moder10().output()
                                .moder11().output()
                                .moder12().output()
                                .moder13().output()
                                .moder14().output()
                                .moder15().output());
    }
}

fn a_test(vga: &mut Vga<Idle>) -> ! {
    let mut color = 0;
    vga.with_raster(
        |ln, tgt, rc| {
            rast::solid_color_fill(ln, tgt, rc, 800, color);
            color = color.wrapping_add(1);
        },
        |vga| {
            loop { }
        },
    )
}

pub fn init(mut nvic: cm::NVIC,
            scb: &mut cm::SCB,
            flash: &device::FLASH,
            dbg: &device::DBG,
            rcc: device::RCC,
            gpiob: device::GPIOB,
            gpioe: device::GPIOE,
            tim1: device::TIM1,
            tim3: device::TIM3,
            tim4: device::TIM4,
            dma2: device::DMA2,
            )
    -> Vga<Idle>
{
    let previous_instance = unsafe {
        DRIVER_INIT_FLAG.swap(true, Ordering::SeqCst)
    };
    assert_eq!(previous_instance, false);

    // Turn on I/O compensation cell to reduce noise on power supply.
    rcc.apb2enr.modify(|_, w| w.syscfgen().enabled());
    // TODO: CMPCR seems to be modeled as read-only (?)
    //p.SYSCFG.cmpcr.modify(|_, w| w.cmp_pd().enabled());

    // Turn a bunch of stuff on.
    rcc.ahb1enr.modify(|_, w| w
                       .gpioben().enabled()
                       .gpioeen().enabled()
                       .dma2en().enabled());

    // DMA configuration.
    
    // Configure FIFO.
    dma2.s5fcr.write(|w| w
                     .fth().quarter()
                     .dmdis().enabled()
                     .feie().disabled());
    // Enable the pixel-generation timer.
    rcc.apb2enr.modify(|_, w| w.tim1en().enabled());
    tim1.psc.reset(); // Divide by 1 => PSC=0
    tim1.cr1.write(|w| w.urs().counter_only());
    tim1.dier.write(|w| w.ude().set_bit());

    // Configure interrupt priorities. This is safe because we haven't enabled
    // interrupts yet.
    unsafe {
        nvic.set_priority(device::Interrupt::TIM4, 0x00);
        nvic.set_priority(device::Interrupt::TIM3, 0x10);
        scb.set_priority(SystemHandler::SysTick, 0xFF);
    }

    // Enable Flash cache and prefetching to reduce jitter.
    flash.acr.modify(|_, w| w
                     .dcen().enabled()
                     .icen().enabled()
                     .prften().enabled());

    // Stop all video-related timers on debug halt, which makes debugging
    // waaaaay easier.
    dbg.dbgmcu_apb1_fz.modify(|_, w| w
                              .dbg_tim4_stop().set_bit()
                              .dbg_tim3_stop().set_bit());
    dbg.dbgmcu_apb2_fz.modify(|_, w| w
                              .dbg_tim1_stop().set_bit());

    let vga = Vga {
        rcc,
        gpiob,
        gpioe,
        tim1,
        tim3,
        tim4,
        dma2,
        nvic,
        _marker: PhantomData,
    };
    vga.sync_off();
    vga.video_off();
    vga
}

/// Pattern for acquiring hardware resources loaned to an ISR in a static.
///
/// # Panics
///
/// If the `SpinLock` is locked when this is called. This would imply:
///
/// 1. that the IRQ got enabled too early, while the hardware is being
///    provisioned;
/// 2. That two ISRs are attempting to use the hardware without coordination.
/// 3. That a previous invocation of an ISR leaked the lock guard.
///
/// Also: if this is called before hardware is provisioned, implying that the
/// IRQ was enabled too early.
fn acquire_hw<T: Send>(lock: &SpinLock<Option<T>>) -> SpinLockGuard<T> {
    SpinLockGuard::map(
        lock.try_lock().expect("HW lock held at ISR"),
        |o| o.as_mut().expect("ISR fired without HW available"),
    )
}

static SHOCK_TIMER: SpinLock<Option<device::TIM3>> = SpinLock::new(None);

/// Entry point for the shock absorber ISR.
///
/// The purpose of the shock absorber is to idle the CPU, stopping I/D fetches
/// and lower-priority interrupts, until a higher-priority interrupt arrives.
/// This ensures that the interrupt latency of the higher-priority interrupt is
/// not affected by multi-cycle bus transactions, Flash wait states, or
/// tail-chaining.
///
/// Note: this is `etl_stm32f4xx_tim3_handler` in the C++.
pub fn shock_absorber_isr() {
    // Acknowledge IRQ so it doesn't re-occur.
    acquire_hw(&SHOCK_TIMER)
        .sr.modify(|_, w| w.cc2if().clear_bit());
    // Idle the CPU until an interrupt arrives.
    cortex_m::asm::wfi()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum VState {
    Blank = 0b00,
    Starting = 0b01,
    Active = 0b11,
    Finishing = 0b10,
}

impl VState {
    fn is_displayed_state(self) -> bool {
        (self as usize & 0b10) != 0
    }

    fn is_rendered_state(self) -> bool {
        (self as usize & 1) != 0
    }
}

static VERT_STATE: AtomicUsize = AtomicUsize::new(VState::Blank as usize);
static TIMING: SpinLock<Option<Timing>> = SpinLock::new(None);
static LINE: AtomicUsize = AtomicUsize::new(0);

fn vert_state() -> VState {
    match VERT_STATE.load(Ordering::Relaxed) {
        0b00 => VState::Blank,
        0b01 => VState::Starting,
        0b11 => VState::Active,
        _    => VState::Finishing,
    }
}

fn set_vert_state(s: VState) {
    VERT_STATE.store(s as usize, Ordering::Relaxed)
}

/// Entry point for the raster maintenance ISR, invoked as PendSV.
fn maintain_raster_isr() {
    // Safety: RASTER_STATE is mut only because rustc is really picky about
    // seeing uses of mut statics like GLOBAL_WORKING_BUFFER in the initializers
    // of non-mut statics.
    let mut state = unsafe { RASTER_STATE.try_lock() }.unwrap();

    let vs = vert_state();

    // First, prepare for scanout from SAV on this line.  This has two purposes:
    // it frees up the rasterization target buffer so that we can overwrite it,
    // and it applies pixel timing choices from the *last* rasterizer run to the
    // scanout machine so that we can replace them as well.
    //
    // This writes to the scanout buffer *and* accesses AHB/APB peripherals, so
    // it *cannot* run concurrently with scanout -- so we do it first, during
    // hblank.
    if vs.is_displayed_state() {
        if state.update_scan_buffer {
            update_scan_buffer(
                state.raster_ctx.target_range.end,
                &mut state.working_buffer, 
            );
        }
        let (dma_cr, use_timer) = {
            // We need to borrow hardware from the horizontal state machine.
            // Keep this scope as small as possible to avoid conflict.
            let hw = acquire_hw(&HSTATE_HW);
            prepare_for_scanout(
                &hw.dma2,
                &hw.tim1,
                &state.raster_ctx,
            )
        };
        // [OX] TODO: omg there is no actual way to get the bits out of this
        let dma_cr_bits = unsafe { core::mem::transmute(dma_cr) };

        // Note: we are now racing hstate SAV for control of this lock.
        *NEXT_XFER.try_lock().unwrap() = NextTransfer {
            dma_cr_bits, 
            use_timer,
        };
    }

    // Allow the application to do additional work during what's left of hblank.
    //vga_hblank_interrupt();

    // Second, rasterize the *next* line, if there's a useful next line.
    // Rasterization can take a while, and may run concurrently with scanout.
    // As a result, we just stash our results in places where the *next* PendSV
    // will find and apply them.
    if (vs.is_rendered_state()) {
        let state = &mut *state;
        rasterize_next_line(
            &*TIMING.try_lock().unwrap().as_mut().unwrap(),
            &mut state.raster_ctx,
            &mut state.working_buffer,
        );
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

fn rasterize_next_line(timing: &Timing,
                       ctx: &mut RasterCtx,
                       working: &mut WorkingBuffer)
    -> bool
{
    let current_line = LINE.load(Ordering::Relaxed);
    let next_line = current_line + 1;
    let visible_line = next_line - timing.video_start_line;

    if ctx.repeat_lines == 0 {
        // Set up a default context for the rasterizer to alter if desired.
        *ctx = RasterCtx {
            cycles_per_pixel: timing.cycles_per_pixel(),
            repeat_lines: 0,
            target_range: 0..0,
        };
        // Invoke the rasterizer.
        RASTER.observe(|r| r(
                current_line,
                working_buffer_as_u8(working),
                ctx,
        ));
        true
    } else {  // repeat_lines > 0
        ctx.repeat_lines -= 1;
        false
    }
}

/// Sets up the scanout configuration. This is done well in advance of the
/// actual start of scanout.
///
/// This returns the two pieces of information that are needed to trigger
/// scanout the rest of the way: a CR value and a flag indicating whether the
/// scanout will use a timer-generated DRQ (`true`) or run at full speed
/// (`false`).
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
            .minc().clear_bit()
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
            .minc().set_bit();

        (xfer, false)
    }
}

fn disable_h_timer(nvic: &mut cortex_m::peripheral::NVIC,
                   i: &device::Interrupt,
                   rcc: &device::RCC,
                   reset: impl FnOnce(&mut device::rcc::apb1rstr::W)
                               -> &mut device::rcc::apb1rstr::W) {
    disable_irq(nvic, copy_interrupt(i));
    rcc.apb1rstr.modify(|_, w| reset(w));
    cortex_m::asm::dsb();
    clear_pending_irq(copy_interrupt(i));
}

fn configure_h_timer(timing: &Timing,
                     nvic: &mut cortex_m::peripheral::NVIC,
                     i: &device::Interrupt,
                     tim: &device::tim3::RegisterBlock,
                     rcc: &device::RCC,
                     enable_clock: impl FnOnce(&mut device::rcc::apb1enr::W)
                               -> &mut device::rcc::apb1enr::W,
                     leave_reset: impl FnOnce(&mut device::rcc::apb1rstr::W)
                               -> &mut device::rcc::apb1rstr::W) {
    rcc.apb1enr.modify(|_, w| enable_clock(w));
    rcc.apb1rstr.modify(|_, w| leave_reset(w));

    // Configure the timer to count in pixels.  These timers live on APB1.
    // Like all APB timers they get their clocks doubled at certain APB
    // multipliers.
    let apb_cycles_per_pixel = if timing.clock_config.apb1_divisor > 1 {
        (timing.cycles_per_pixel() * 2 / timing.clock_config.apb1_divisor)
    } else {
        timing.cycles_per_pixel()
    };

    // TODO PSC fields are defined as unsafe - BUG
    tim.psc.write(|w| unsafe { w.bits(apb_cycles_per_pixel as u32 - 1) });

    // TODO ARR fields are defined as unsafe - BUG
    tim.arr.write(|w| unsafe { w.bits(timing.line_pixels as u32 - 1) });

    tim.ccr1.write(|w| unsafe { w.bits(timing.sync_pixels as u32) });
    tim.ccr2.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels - timing.video_lead) as u32
                )});;
    tim.ccr3.write(|w| unsafe { w.bits(
                (timing.sync_pixels
                 + timing.back_porch_pixels + timing.video_pixels) as u32
                )});

    tim.ccmr1_output.write(|w| {
        unsafe { w.oc1m().bits(0b110); }  // PWM1 TODO
        unsafe { w.cc1s().bits(0b00); } // output TODO
        w
    });

    tim.ccer.write(|w| w
                   .cc1e().set_bit()
                   .cc1p().bit(timing.hsync_polarity == Polarity::Negative));
}
