//! "Rotozoomer" showing affine texture transformation.

#![no_std]
#![no_main]

#[cfg(feature = "panic-itm")]
extern crate panic_itm;
#[cfg(feature = "panic-halt")]
extern crate panic_halt;

use core::sync::atomic::{AtomicUsize, Ordering};
use stm32f4;
use stm32f4::stm32f407::interrupt;

use m4vga::rast::direct;
use m4vga::math::{Mat3f, Vec2};

use libm::F32Ext;

const X_SCALE: usize = 2;
const Y_SCALE: usize = 2;
const WIDTH: usize = 800 / X_SCALE;
const HEIGHT: usize = 600 / Y_SCALE;
const HALF_HEIGHT: usize = HEIGHT / 2;

const BUFFER_SIZE: usize = WIDTH * HALF_HEIGHT;
const BUFFER_WORDS: usize = BUFFER_SIZE / 4;
const BUFFER_STRIDE: usize = WIDTH / 4;

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    // Safety: as long as these are the *only* mutable references we produce to
    // those statics, we're good.
    let mut buf = RaceBuffer::new([
        {
            static mut TOP: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut TOP }
        },
        {
            #[link_section = ".local_bss"]
            static mut BOT: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
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
            |_, tgt, ctx, _| {
                let buf = reader.take_line();
                ctx.cycles_per_pixel *= X_SCALE;
                ctx.repeat_lines = Y_SCALE - 1;
                direct::direct_color(
                    0,
                    tgt,
                    ctx,
                    buf,
                    BUFFER_STRIDE,
                );
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

                loop {
                    let s = (frame as f32 / 50.).sin() * 0.7 + 1.;
                    let tx = (frame as f32 / 100.).cos() * 100.;
                    let ty = 0.;

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
                    m4vga::measurement::sig_d_set();
                    for _ in 0..HEIGHT {
                        let mut buf = writer.generate_line();
                        let buf = u32_as_u8_mut(&mut *buf);
                        {
                            let mut pos = ybase;
                            for x in 0..WIDTH {
                                buf[x] = tex_fetch(pos.0[0], pos.0[1]) as u8;
                                pos = pos + xi;
                            }
                        }
                        ybase = ybase + yi
                    }

                    m = m * rot;

                    frame += 1;

                    m4vga::measurement::sig_d_clear();
                    vga.sync_to_vblank();
                    writer.reset();
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

/// A specialized framebuffer structure with two features:
///
/// 1. The framebuffer is split into two parts, because the whole thing won't
///    fit into any single RAM.
/// 2. It checks for aliasing on a scanline granularity so rendering can race
///    scanout more aggressively.
struct RaceBuffer {
    segments: [&'static mut [u32; BUFFER_WORDS]; 2],
    rendered: AtomicUsize,
    displayed: AtomicUsize,
}

impl RaceBuffer {
    pub fn new(segments: [&'static mut [u32; BUFFER_WORDS]; 2]) -> Self {
        RaceBuffer {
            segments,
            rendered: 0.into(),
            displayed: 0.into(),
        }
    }

    pub fn split(&mut self) -> (RaceReader, RaceWriter) {
        let alias = unsafe { & *(self as *const _) };
        (
            RaceReader { buf: alias },
            RaceWriter { buf: self },
        )
    }
}

struct RaceReader<'a> {
    buf: &'a RaceBuffer,
}

impl<'a> RaceReader<'a> {
    pub fn take_line(&mut self) -> &[u32] {
        let line_number = self.buf.displayed.fetch_add(1, Ordering::Relaxed);
        let rendered = self.buf.rendered.load(Ordering::Relaxed);
        let gap = rendered.wrapping_sub(line_number);

        if gap <= HEIGHT {
            let (seg_index, line_number) = if line_number < HALF_HEIGHT {
                (0, line_number)
            } else {
                (1, line_number - HALF_HEIGHT)
            };
            let offset = line_number * BUFFER_STRIDE;
            &self.buf.segments[seg_index][offset .. offset + BUFFER_STRIDE]
        } else {
            panic!("tearing: scanout reached {} but rendering only {}",
                   line_number, rendered);
        }
    }
}

struct RaceWriter<'a> {
    buf: &'a mut RaceBuffer,
}

impl<'a> RaceWriter<'a> {
    pub fn generate_line(&mut self) -> GenGuard {
        let line_number = self.buf.rendered.load(Ordering::Relaxed);
        let (seg_index, line_number) = if line_number < HALF_HEIGHT {
            (0, line_number)
        } else {
            (1, line_number - HALF_HEIGHT)
        };
        let offset = line_number * BUFFER_STRIDE;
        GenGuard {
            counter: &self.buf.rendered,
            data: &mut self.buf.segments[seg_index]
                [offset .. offset + BUFFER_STRIDE],
        }
    }

    pub fn reset(&mut self) {
        self.buf.rendered.store(0, Ordering::Relaxed);
        self.buf.displayed.store(0, Ordering::Relaxed);
    }
}

struct GenGuard<'a> {
    counter: &'a AtomicUsize,
    data: &'a mut [u32],
}

impl<'a> Drop for GenGuard<'a> {
    fn drop(&mut self) {
        self.counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl<'a> core::ops::Deref for GenGuard<'a> {
    type Target = [u32];
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a> core::ops::DerefMut for GenGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
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
