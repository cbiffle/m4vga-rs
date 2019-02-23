//! Conway's Life automaton, full screen, 60fps.

#![no_std]
#![no_main]

#[cfg(feature = "panic-itm")]
extern crate panic_itm;
#[cfg(feature = "panic-halt")]
extern crate panic_halt;

mod conway;

use core::sync::atomic::AtomicUsize;

use stm32f4;

use stm32f4::stm32f407::interrupt;
use m4vga::util::rw_lock::ReadWriteLock;

// this can go in the default SRAM
static mut BUF0: [u32; 800 * 600 / 32] = [0; 800*600/32];
// this needs to get placed because they won't both fit
#[link_section = ".local_bss"]
static mut BUF1: [u32; 800 * 600 / 32] = [0; 800*600/32];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Safety: we're claiming the single exclusive reference to our static
    // buffer. Safe so long as we don't do it again.
    let fg = ReadWriteLock::new(unsafe { &mut BUF0 });
    // Safety: we're claiming the single exclusive reference to our static
    // buffer. Safe so long as we don't do it again.
    let mut bg = unsafe { &mut BUF1 };

    {
        use rand::{Rng, SeedableRng};
        let mut rng = rand::rngs::SmallRng::seed_from_u64(11181981);
        for word in bg.iter_mut() {
            *word = rng.gen();
        }
    }

    let clut = AtomicUsize::new(0xFF00);

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx, _| {
                m4vga::measurement::sig_d_set();

                let fg = fg.try_lock().expect("fg unavail");

                m4vga::measurement::sig_d_clear();

                let offset = ln * (800 / 32);
                m4vga::rast::bitmap_1::unpack(
                    &fg[offset .. offset + (800 / 32)],
                    &clut,
                    &mut tgt[0..800]
                );
                ctx.target_range = 0..800;  // 800 pixels now valid
            },
            // This closure contains the main loop of the program.
            |vga| loop {
                vga.sync_to_vblank();
                core::mem::swap(&mut bg, &mut *fg.lock_mut());

                conway::step(&*fg.lock(), bg);

                vga.video_on();
            })
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
#[link_section = ".ramcode"]
fn PendSV() {
    m4vga::pendsv_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
fn TIM3() {
    m4vga::tim3_shock_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
fn TIM4() {
    m4vga::tim4_horiz_isr()
}
