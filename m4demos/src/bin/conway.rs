//! Conway's Game of Life at full resolution.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use stm32f4;
use stm32f4::stm32f407::interrupt;

use m4vga::priority;
use m4vga_fx_common::{Demo, Raster, Render};
use m4vga_fx_conway as fx;

const BUF_SIZE: usize = 800 * 600 / 32;

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Safety: as long as these are the *only* mutable references we produce to
    // those statics, we're good.
    let mut state = fx::State::new(
        // Foreground
        {
            static mut BUF0: [u32; BUF_SIZE] = [0; BUF_SIZE];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut BUF0 as &mut [_] }
        },
        // Background
        {
            #[link_section = ".local_bss"]
            static mut BUF1: [u32; BUF_SIZE] = [0; BUF_SIZE];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut BUF1 as &mut [_] }
        },
        // Foreground color
        0b11_11_11,
        // Background color
        0b00_00_00,
    );
    let (mut raster, mut render) = state.split();

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            #[link_section = ".ramcode"]
            |ln, tgt, ctx, p| raster.raster_callback(ln, tgt, ctx, p),
            |vga| {
                let mut frame = 0;
                let thread = priority::Thread::new_checked().unwrap();

                loop {
                    vga.sync_to_vblank();
                    render.render_frame(frame, thread);
                    frame += 1;
                }
            },
        )
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
#[link_section = ".ramcode"]
fn PendSV() {
    m4vga::pendsv_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM3() {
    m4vga::tim3_shock_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM4() {
    m4vga::tim4_horiz_isr()
}
