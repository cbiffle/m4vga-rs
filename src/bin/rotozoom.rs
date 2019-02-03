//! "Rotozoomer" showing affine texture transformation.

#![no_std]
#![no_main]

#[cfg(feature = "panic-itm")]
extern crate panic_itm;
#[cfg(feature = "panic-halt")]
extern crate panic_halt;

use stm32f4;
use stm32f4::stm32f407::interrupt;

use m4vga::rast::direct;
use m4vga::util::spin_lock::SpinLock;
use m4vga::math::{Mat3f, Vec2, lerp};

use libm::F32Ext;

const SCALE: usize = 4;
const WIDTH: usize = 800 / SCALE;
const HEIGHT: usize = 600 / SCALE;

const BUFFER_SIZE: usize = WIDTH * HEIGHT;
const BUFFER_WORDS: usize = BUFFER_SIZE / 4;
const BUFFER_STRIDE: usize = WIDTH / 4;

static mut BUF0: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
#[link_section = ".local_ram"]
static mut BUF1: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    let fg = SpinLock::new(unsafe { &mut BUF0 });
    let mut bg = unsafe { &mut BUF1 };

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels. Here, we just scribble a test pattern into
            // the target buffer.
            |ln, tgt, ctx| {
                let ln = ln / SCALE;
                let buf = fg.try_lock().expect("rast fg access");
                direct::direct_color(
                    ln,
                    tgt,
                    ctx,
                    *buf,
                    BUFFER_STRIDE,
                );
                ctx.target_range = 0..WIDTH;
                ctx.cycles_per_pixel *= SCALE;
                ctx.repeat_lines = SCALE - 1;
            },
            // This closure contains the main loop of the program.
            |vga| {
                let mut frame = 0;
                let mut m = Mat3f::identity();

                let cols = WIDTH as f32;
                let rows = HEIGHT as f32;

                vga.video_on();

                let rs = 0.01.sin();
                let rc = 0.01.cos();

                loop {
                    vga.sync_to_vblank();
                    core::mem::swap(&mut bg,
                                    &mut *fg.try_lock().expect("swap access"));
                    let bg = u32_as_u8_mut(bg);

                    let s = (frame as f32 / 63. + 1.3).sin() + 1.5;
                    let tx = (frame as f32 / 59.).cos() * 50.;
                    let ty = (frame as f32 / 50.).sin() * 50.;

                    let scale = Mat3f::scale(s, s);
                    let trans = Mat3f::translate(tx, ty);

                    let m_ = m * trans * scale;

                    let vertices = [
                        (m_ * Vec2([-cols/2., -rows/2.]).augment()).project(),
                        (m_ * Vec2([ cols/2., -rows/2.]).augment()).project(),
                        (m_ * Vec2([-cols/2.,  rows/2.]).augment()).project(),
                        (m_ * Vec2([ cols/2.,  rows/2.]).augment()).project(),
                    ];

                    let xi = (vertices[1] - vertices[0]) * (1. / cols);

                    for y in 0..HEIGHT {
                        let yr = y as f32 / rows;
                        let mut pos = lerp(vertices[0], vertices[2], yr);
                        for x in 0..WIDTH {
                            bg[y * WIDTH + x] =
                                tex_fetch(pos.0[0], pos.0[1]) as u8;
                            pos = pos + xi;
                        }
                    }

                    m = m * Mat3f::rotate(rc, rs);

                    frame += 1;
                }
            })
}

fn tex_fetch(u: f32, v: f32) -> u32 {
    u as i32 as u32 ^ v as i32 as u32
}

fn u32_as_u8_mut(slice: &mut [u32]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            slice.len() * 4,
        )
    }
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
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
#[link_section = ".ramcode"]
fn TIM4() {
    m4vga::tim4_horiz_isr()
}
