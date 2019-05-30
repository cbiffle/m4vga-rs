//! Interrupt handler for horizontal retrace.

use stm32f4::stm32f407 as device;

use core::sync::atomic::Ordering;

use crate::timing::Timing;
use crate::util::measurement;
use crate::util::stm32::CopyHack;
use crate::{
    acquire_hw, set_vert_state, vert_state, VState, HPSHARE, LINE, TIMING,
};

/// Horizontal state machine ISR: call this from `TIM4`.
///
/// This is one of three ISRs you must wire up for the driver to work. In the
/// simplest case, this means your application needs to include code like the
/// following:
///
/// ```
/// use stm32f4::interrupt;
///
/// #[interrupt]
/// fn TIM4() {
///     m4vga::tim4_horiz_isr()
/// }
/// ```
pub fn hstate_isr() {
    measurement::sig_a_set();

    // Start a critical section wrt PendSV here. We're higher priority, so
    // really this just detects races.
    let shared = acquire_hw(&HPSHARE);
    let hw = &shared.hw;

    // TODO: this appears to be the most concise way of read-modify-writing a
    // register and saving the prior value in the current svd2rust API. Report a
    // bug.
    let sr = hw.tim4.sr.read();
    // Safety: only unsafe due to upstream bug. TODO
    hw.tim4.sr.write(|w| {
        unsafe { w.bits(sr.bits()) }
            .cc2if()
            .clear_bit()
            .cc3if()
            .clear_bit()
    });

    // CC2 indicates start-of-active video.
    //
    // THIS PATH IS LATENCY SENSITIVE.
    if sr.cc2if().bit_is_set() {
        // Only bother doing work if we're not in vblank.
        if vert_state().is_displayed_state() {
            let params = &shared.xfer;
            start_of_active_video(
                &hw.dma2,
                &hw.tim1,
                params.dma_cr.copy_hack(),
                params.use_timer,
            );
        }
    }

    // CC3 indicates end-of-active video
    //
    // This path is not latency sensitive, but should be pretty quick to give
    // PendSV time to do stuff.
    if sr.cc3if().bit_is_set() {
        // We have work to do regardless of vertical state, because this routine
        // maintains the vertical state itself!
        let line = end_of_active_video(
            &hw.tim1,
            &hw.tim4,
            &hw.gpiob,
            TIMING.try_lock().expect("hstate timing").as_ref().unwrap(),
            LINE.load(Ordering::Relaxed),
        );
        LINE.store(line, Ordering::Relaxed);
    }

    measurement::sig_a_clear();
}

/// Routine for handling SAV (start-of-active video). This needs to do as little
/// work as possible to avoid messing up scanout timing.
fn start_of_active_video(
    dma: &device::DMA2,
    drq_timer: &device::TIM1,
    dma_xfer: device::dma2::s5cr::W,
    use_timer_drq: bool,
) {
    // This routine is currently 11 instructions in a release build.

    // Clear stream 5 flags. HIFCR is a write-1-to-clear register.
    //
    // (Write a constant to a fixed address.)
    dma.hifcr.write(|w| {
        w.cdmeif5()
            .set_bit()
            .cteif5()
            .set_bit()
            .chtif5()
            .set_bit()
            .ctcif5()
            .set_bit()
    });

    // Start the countdown for first DRQ, if relevant.
    //
    // (Write a slightly computed value to a fixed address.)
    drq_timer
        .cr1
        .write(|w| w.urs().counter_only().cen().bit(use_timer_drq));

    // Configure DMA stream.
    //
    // (Write a value passed in a register to a fixed address.)
    dma.s5cr.write(|w| {
        *w = dma_xfer;
        w
    });
}

/// Handler for the end-of-active-video horizontal state event.
///
/// Returns the number of the next scanline.
#[must_use = "you forgot to advance the line"]
fn end_of_active_video(
    drq_timer: &device::TIM1,
    h_timer: &device::TIM4,
    gpiob: &device::GPIOB,
    current_timing: &Timing,
    current_line: usize,
) -> usize {
    // The end-of-active-video (EAV) event is always significant, as it advances
    // the line state machine and kicks off PendSV.

    // Shut off TIM1; only really matters in reduced-horizontal mode.
    drq_timer
        .cr1
        .write(|w| w.urs().counter_only().cen().clear_bit());

    // Apply timing changes requested by the last rasterizer.
    // TODO: actually, if I'm not implementing the 'offset' field I used for
    // display distortion effects, I don't need to do this every scanline.
    if false {
        h_timer.ccr2.write(|w| {
            w.ccr2().bits(
                (current_timing.sync_pixels + current_timing.back_porch_pixels
                    - current_timing.video_lead) as u32, //+ working_buffer_shape.offset TODO am I implementing offset?
            )
        });
    }

    // Pend a PendSV to process hblank tasks. This can happen any time during
    // this routine -- it won't take effect until we return.
    cortex_m::peripheral::SCB::set_pendsv();

    // We've finished this line; figure out what to do on the next one.
    let next_line = current_line + 1;
    let mut rollover = false;
    if next_line == current_timing.vsync_start_line
        || next_line == current_timing.vsync_end_line
    {
        // Either edge of vsync pulse.
        // TODO: really unfortunate toggle code. File bug.
        let odr = gpiob.odr.read().bits();
        let mask = 1 << 7;
        gpiob.bsrr.write(|w| {
            use crate::util::stm32::AllWriteExt;
            w.bits_ext((!odr & mask) | ((odr & mask) << 16))
        });
    } else if next_line + 1 == current_timing.video_start_line {
        // We're one line before scanout begins -- need to start rasterizing.
        set_vert_state(VState::Starting);
    // TODO: used to have band-list-taken goo here. This would be an
    // appropriate place to lock the rasterization callback for the duration
    // of the frame, if desired.
    } else if next_line == current_timing.video_start_line {
        // Time to start output.  This will cause PendSV to copy rasterization
        // output into place for scanout, and the next SAV will start DMA.
        set_vert_state(VState::Active);
    } else if next_line + 1 == current_timing.video_end_line {
        // For the final line, suppress rasterization but continue preparing
        // previously rasterized data for scanout, and continue starting DMA in
        // SAV.
        set_vert_state(VState::Finishing);
    } else if next_line == current_timing.video_end_line {
        // All done!  Suppress all scanout activity.
        set_vert_state(VState::Blank);
        rollover = true;
    }

    if rollover {
        0
    } else {
        next_line
    }
}
