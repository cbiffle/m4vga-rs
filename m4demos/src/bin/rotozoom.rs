//! "Rotozoomer" showing affine texture transformation.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use stm32f4;
use stm32f4::stm32f407::interrupt;

use math::{Augment, Mat3f, Matrix, Project, Vec2};

use m4vga::priority;
use m4vga::rast::direct;
use m4vga::util::race_buf::RaceBuffer;

use libm::F32Ext;

const X_SCALE: usize = 2;
const Y_SCALE: usize = 2;
const WIDTH: usize = 800 / X_SCALE;
const HEIGHT: usize = 600 / Y_SCALE;
const HALF_HEIGHT: usize = HEIGHT / 2;

const BUFFER_STRIDE: usize = WIDTH / 4;

type Row = [u32; BUFFER_STRIDE];
type Row8 = [u8; WIDTH];
type BufferBand = [Row; HALF_HEIGHT];

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Safety: as long as these are the *only* mutable references we produce to
    // those statics, we're good.
    let mut buf: RaceBuffer<&mut [Row], Row> = RaceBuffer::new([
        {
            static mut TOP: BufferBand = [[0; BUFFER_STRIDE]; HALF_HEIGHT];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut TOP }
        },
        {
            #[link_section = ".local_bss"]
            static mut BOT: BufferBand = [[0; BUFFER_STRIDE]; HALF_HEIGHT];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut BOT }
        },
    ]);
    let (mut reader, mut writer) = buf.split();

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels. Here, we just scribble a test pattern into
            // the target buffer.
            #[link_section = ".ramcode"]
            |ln, tgt, ctx, p| {
                let buf = reader.take_line(ln / Y_SCALE, &p);
                ctx.cycles_per_pixel *= X_SCALE;
                ctx.repeat_lines = Y_SCALE - 1;
                direct::direct_color(0, tgt, ctx, buf, BUFFER_STRIDE);
            },
            // This closure contains the main loop of the program.
            |vga| {
                let mut frame = 0;
                let mut m = Mat3f::identity();

                let cols = WIDTH as f32;
                let rows = HEIGHT as f32;

                vga.video_on();

                const ROT: f32 = 0.005;

                let rot = Mat3f::rotate(ROT);
                let thread = priority::Thread::new_checked().unwrap();

                loop {
                    m4vga::util::measurement::sig_d_set();
                    let s = (frame as f32 / 50.).sin() * 0.7 + 1.;
                    let tx = (frame as f32 / 100.).cos() * 100.;
                    let ty = 0.;

                    let m_ = m * Mat3f::translate(tx, ty) * Mat3f::scale(s, s);

                    let top_left =
                        (m_ * Vec2(-cols / 2., -rows / 2.).augment()).project();
                    let top_right =
                        (m_ * Vec2(cols / 2., -rows / 2.).augment()).project();
                    let bot_left =
                        (m_ * Vec2(-cols / 2., rows / 2.).augment()).project();

                    let xi = (top_right - top_left) * (1. / cols);
                    let yi = (bot_left - top_left) * (1. / rows);
                    let mut ybase = top_left;
                    for _ in 0..HEIGHT {
                        let mut buf = writer.generate_line(&thread);
                        let buf = u32_as_u8_mut(&mut *buf);
                        {
                            let mut pos = ybase;
                            for x in 0..WIDTH {
                                buf[x] = tex_fetch(pos.0, pos.1) as u8;
                                pos = pos + xi;
                            }
                        }
                        ybase = ybase + yi;
                    }

                    m = m * rot;

                    frame += 1;

                    m4vga::util::measurement::sig_d_clear();
                    vga.sync_to_vblank();
                    writer.reset(&thread);
                }
            },
        )
}

fn tex_fetch(u: f32, v: f32) -> u32 {
    u as i32 as u32 ^ v as i32 as u32
}

fn u32_as_u8_mut(r: &mut Row) -> &mut Row8 {
    assert_eq!(core::mem::size_of::<Row>(), core::mem::size_of::<Row8>());
    unsafe { core::mem::transmute(r) }
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
