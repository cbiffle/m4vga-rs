//! Traditional "zooming down a tunnel" effect.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use m4vga_fx_common::{Demo, Raster, Render};
use m4vga_fx_tunnel as lib;

use stm32f4;
use stm32f4::stm32f407::interrupt;

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    let mut state = unsafe { lib::init() };
    let (mut raster_state, mut render_state) = state.split();
    let mut frame = 0;

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx, p0| {
                raster_state.raster_callback(ln, tgt, ctx, p0)
            },
            // This closure contains the main loop of the program.
            |vga| loop {
                vga.sync_to_vblank();
                render_state.render_frame(frame);
                frame = (frame + 1) % 65536;
                vga.video_on();
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
