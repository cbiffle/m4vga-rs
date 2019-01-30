use stm32f4::stm32f407 as device;

use core::sync::atomic::Ordering;

use crate::{vert_state, set_vert_state, NEXT_XFER, VState, TIMING, LINE, HSTATE_HW, acquire_hw};
use crate::timing::Timing;

/// Entry point for the horizontal timing state machine ISR.
///
/// Note: this is `etl_stm32f4xx_tim4_handler` in the C++.
pub fn hstate_isr() {
    let hw = acquire_hw(&HSTATE_HW);

    // TODO: this appears to be the most concise way of read-modify-writing a
    // register and saving the prior value in the current svd2rust API. Report a
    // bug.
    let sr = hw.tim4.sr.read();
    hw.tim4.sr.write(|w| unsafe { w.bits(sr.bits()) }
                     .cc2if().clear_bit()
                     .cc3if().clear_bit());

    if sr.cc2if().bit_is_set() {
        if vert_state().is_displayed_state() {
            // Note: we are racing PendSV end-of-rasterization for control of
            // this lock.
            let params = NEXT_XFER.try_lock().expect("hstate xfer");
            let dma_xfer = unsafe { core::mem::transmute(params.dma_cr_bits) };
            start_of_active_video(
                &hw.dma2,
                &hw.tim1,
                dma_xfer,
                params.use_timer,
            );
        }
    }

    if sr.cc3if().bit_is_set() {
        let line = end_of_active_video(
            &hw.tim1,
            &hw.tim4,
            &hw.gpiob,
            TIMING.try_lock().expect("hstate timing").as_ref().unwrap(),
            LINE.load(Ordering::Relaxed),
        );
        LINE.store(line, Ordering::Relaxed);
    }
}

fn start_of_active_video(dma: &device::DMA2,
                         drq_timer: &device::TIM1,
                         dma_xfer: device::dma2::s5cr::W,
                         use_timer_drq: bool) {
    // Clear stream 5 flags. HIFCR is a write-1-to-clear register.
    dma.hifcr.write(|w| w
                    .cdmeif5().set_bit()
                    .cteif5().set_bit()
                    .chtif5().set_bit()
                    .ctcif5().set_bit());

    // Start the countdown for first DRQ, if relevant.
    drq_timer.cr1.write(|w| w.urs().counter_only()
                        .cen().bit(use_timer_drq));

    // Configure DMA stream.
    dma.s5cr.write(|w| { *w = dma_xfer; w });
}

/// Handler for the end-of-active-video horizontal state event.
///
/// Returns the number of the next scanline.
#[must_use = "you forgot to advance the line"]
fn end_of_active_video(drq_timer: &device::TIM1,
                       h_timer: &device::TIM4,
                       gpiob: &device::GPIOB,
                       current_timing: &Timing,
                       current_line: usize)
    -> usize
{
    // The end-of-active-video (EAV) event is always significant, as it advances
    // the line state machine and kicks off PendSV.

    // Shut off TIM1; only really matters in reduced-horizontal mode.
    drq_timer.cr1.write(|w| w.urs().counter_only()
                        .cen().clear_bit());

    // Apply timing changes requested by the last rasterizer.
    // TODO: TIM4 CCR2 writes are unsafe, which is a bug
    h_timer.ccr2.write(|w| unsafe {
        w.bits(
            (current_timing.sync_pixels
            + current_timing.back_porch_pixels - current_timing.video_lead)
            as u32
            //+ working_buffer_shape.offset TODO am I implementing offset?
        )
    });

    // Pend a PendSV to process hblank tasks.
    cortex_m::peripheral::SCB::set_pendsv();

    // We've finished this line; figure out what to do on the next one.
    let next_line = current_line + 1;
    let mut rollover = false;
    if next_line == current_timing.vsync_start_line ||
            next_line == current_timing.vsync_end_line {
        // Either edge of vsync pulse.
        {
            // TODO: really unfortunate toggle code. File bug.
            let odr = gpiob.odr.read().bits();
            let mask = 1 << 7;
            gpiob.bsrr.write(|w| unsafe {
                w.bits(
                    (!odr & mask) | ((odr & mask) << 16)
                )
            });
        }
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

    if rollover { 0 } else { next_line }
}
