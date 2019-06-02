//! "Rotozoomer" showing affine texture transformation.

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
use m4vga_fx_rotozoom as fx;

type BufferBand = [fx::Row; fx::HALF_HEIGHT];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Safety: as long as these are the *only* mutable references we produce to
    // those statics, we're good.
    let mut state = fx::State::new([
        {
            static mut TOP: BufferBand =
                [[0; fx::BUFFER_STRIDE]; fx::HALF_HEIGHT];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut TOP as &mut [fx::Row] }
        },
        {
            #[link_section = ".local_bss"]
            static mut BOT: BufferBand =
                [[0; fx::BUFFER_STRIDE]; fx::HALF_HEIGHT];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut BOT as &mut [fx::Row] }
        },
    ]);
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
