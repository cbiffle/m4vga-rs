//! Horizontal test pattern generator.
//!
//! This produces alternating vertical stripes of white-black pixels at full
//! horizontal resolution. It's useful for checking signal integrity: the
//! pattern is easy to observe on a scope, and it generates all the
//! high-frequency transients we can expect in practice.
//!
//! It's also about the simplest thing you can do with the library, so it serves
//! as a concise example.

#![no_std]
#![no_main]

// Demo mains must elect one panic-handler crate. Here we use the ITM one, which
// is low-overhead.
extern crate panic_itm;

use stm32f4;

use stm32f4::stm32f407 as device;
use stm32f4::stm32f407::interrupt;
use m4vga_rs::vga;

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Claim exclusive control of all peripherals from the runtime. This makes
    // us responsible for divvying them up between clients; we just hand the
    // relevant ones to the display driver, below.
    let mut cp = cortex_m::peripheral::Peripherals::take().unwrap();
    let p = device::Peripherals::take().unwrap();

    // Give the driver its hardware resources...
    vga::init(
        cp.NVIC,
        &mut cp.SCB,
        p.FLASH,
        &p.DBG,
        p.RCC,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2)
        // ...select a display timing...
        .configure_timing(&m4vga_rs::vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels. Here, we just scribble a test pattern into
            // the target buffer.
            |_, tgt, ctx| {
                let mut pixel = 0xFF;
                for t in &mut tgt[0..800] {
                    *t = pixel;
                    pixel ^= 0xFF;
                }
                ctx.target_range = 0..800;  // 800 pixels now valid
                ctx.repeat_lines = 599;     // don't ask again this frame
            },
            // This closure contains the main loop of the program.
            |vga| {
                // Enable outputs. The driver doesn't do this for you in case
                // you want to set up some graphics before doing so.
                vga.video_on();
                // Spin forever!
                loop {}
            })
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
fn PendSV() {
    m4vga_rs::vga::bg_rast::maintain_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
fn TIM3() {
    m4vga_rs::vga::shock::shock_absorber_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
fn TIM4() {
    m4vga_rs::vga::hstate::hstate_isr()
}
