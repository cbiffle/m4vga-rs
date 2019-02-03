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
use m4vga::math::{Mat3f, Vec2};

use libm::F32Ext;

const X_SCALE: usize = 2;
const Y_SCALE: usize = 2;
const WIDTH: usize = 792 / X_SCALE;
const HEIGHT: usize = 600 / Y_SCALE;
const HALF_HEIGHT: usize = HEIGHT / 2;

const BUFFER_SIZE: usize = WIDTH * HALF_HEIGHT;
const BUFFER_WORDS: usize = BUFFER_SIZE / 4;
const BUFFER_STRIDE: usize = WIDTH / 4;

static mut TOP: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
#[link_section = ".local_bss"]
static mut BOT: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    let top = SpinLock::new(unsafe { &mut TOP });
    let bot = SpinLock::new(unsafe { &mut BOT });

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
                let ln = ln / Y_SCALE;
                let (buf, ln) = if ln < HALF_HEIGHT {
                    (unsafe { &mut TOP }, ln)
                } else {
                    (unsafe { &mut BOT }, ln - HALF_HEIGHT)
                };
                direct::direct_color(
                    ln,
                    tgt,
                    ctx,
                    buf,
                    BUFFER_STRIDE,
                );
                ctx.target_range = 0..WIDTH;
                ctx.cycles_per_pixel *= X_SCALE;
                ctx.repeat_lines = Y_SCALE - 1;
            },
            // This closure contains the main loop of the program.
            |vga| {
                let mut frame = 0;
                let mut m = Mat3f::identity();

                let cols = WIDTH as f32;
                let rows = HEIGHT as f32;

                vga.video_on();

                let rot = Mat3f::rotate(0.01);

                loop {
                    vga.sync_to_vblank();
                    m4vga::measurement::sig_d_set();

                    let s = (frame as f32 / 63.).sin() + 1.3;
                    let tx = frame as f32 / 10.;
                    let ty = (frame as f32 / 50.).sin() * 50.;

                    let m_ = m * Mat3f::translate(tx, ty) * Mat3f::scale(s, s);

                    let top_left = 
                        (m_ * Vec2([-cols/2., -rows/2.]).augment()).project();
                    let top_right =
                        (m_ * Vec2([ cols/2., -rows/2.]).augment()).project();
                    let bot_left =
                        (m_ * Vec2([-cols/2.,  rows/2.]).augment()).project();

                    let xi = (top_right - top_left) * (1. / cols);
                    let yi = (bot_left - top_left) * (1. / rows);
                    let mut ybase = top_left;
                    let mut buf = top.try_lock().expect("render access");
                    let bg = u32_as_u8_mut(&mut **buf);
                    for y in 0..HALF_HEIGHT {
                        {
                            let mut pos = ybase;
                            for x in 0..WIDTH {
                                bg[y * WIDTH + x] =
                                    tex_fetch(pos.0[0], pos.0[1]) as u8;
                                pos = pos + xi;
                            }
                        }
                        ybase = ybase + yi
                    }
                    drop(bg); drop(buf);

                    let mut buf = bot.try_lock().expect("render access");
                    let bg = u32_as_u8_mut(&mut **buf);
                    for y in 0..HALF_HEIGHT {
                        {
                            let mut pos = ybase;
                            for x in 0..WIDTH {
                                bg[y * WIDTH + x] =
                                    tex_fetch(pos.0[0], pos.0[1]) as u8;
                                pos = pos + xi;
                            }
                        }
                        ybase = ybase + yi
                    }
                    drop(bg); drop(buf);
                    m = m * rot;

                    frame += 1;

                    m4vga::measurement::sig_d_clear();
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
