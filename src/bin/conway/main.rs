//! Conway's Life automaton, full screen, 60fps.

#![no_std]
#![no_main]

// Demo mains must elect one panic-handler crate. Here we use the ITM one, which
// is low-overhead.
extern crate panic_itm;

mod conway;

use core::sync::atomic::AtomicUsize;

use stm32f4;

use stm32f4::stm32f407::interrupt;
use m4vga_rs::vga;
use m4vga_rs::util::rw_lock::ReadWriteLock;

// this can go in the default SRAM
static mut BUF0: [u32; 800 * 600 / 32] = [0; 800*600/32];
// this needs to get placed because they won't both fit
#[link_section = ".local_ram"]
static mut BUF1: [u32; 800 * 600 / 32] = [0; 800*600/32];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    let fg = ReadWriteLock::new(unsafe { &mut BUF0 });
    let mut bg = unsafe { &mut BUF1 };
    let clut = AtomicUsize::new(0xFF00);

    // Give the driver its hardware resources...
    vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga_rs::vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx| {
                let fg = fg.try_lock().unwrap();
                let offset = ln * (800 / 32);
                vga::rast::bitmap_1::unpack(
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
    m4vga_rs::vga::bg_rast::maintain_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
fn TIM3() {
    m4vga_rs::vga::shock::shock_absorber_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM4() {
    m4vga_rs::vga::hstate::hstate_isr()
}
