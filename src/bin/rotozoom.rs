//! "Rotozoomer" showing affine texture transformation.

#![no_std]
#![no_main]

#[cfg(feature = "panic-itm")]
extern crate panic_itm;
#[cfg(feature = "panic-halt")]
extern crate panic_halt;

use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use stm32f4;
use stm32f4::stm32f407::interrupt;

use m4vga::rast::direct;
use m4vga::math::{Mat3f, Vec2};
use m4vga::priority;

use libm::F32Ext;

const X_SCALE: usize = 2;
const Y_SCALE: usize = 2;
const WIDTH: usize = 800 / X_SCALE;
const HEIGHT: usize = 600 / Y_SCALE;
const HALF_HEIGHT: usize = HEIGHT / 2;

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
            static mut TOP: [[u32; BUFFER_STRIDE]; HALF_HEIGHT] =
                [[0; BUFFER_STRIDE]; HALF_HEIGHT];
            // Safety: because of scoping this is clearly the only mutable
            // reference we generate to this static.
            unsafe { &mut TOP }
        },
        {
            #[link_section = ".local_bss"]
            static mut BOT: [[u32; BUFFER_STRIDE]; HALF_HEIGHT] =
                [[0; BUFFER_STRIDE]; HALF_HEIGHT];
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
                let buf = reader.take_line(ln/Y_SCALE, &p);
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
                let thread = priority::Thread::new_checked().unwrap();

                loop {
                    m4vga::measurement::sig_d_set();
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
                    for _ in 0..HEIGHT {
                        let mut buf = writer.generate_line(&thread);
                        let buf = u32_as_u8_mut(&mut *buf);
                        {
                            let mut pos = ybase;
                            for pixel in buf.iter_mut() {
                                *pixel = tex_fetch(pos.0[0], pos.0[1]) as u8;
                                pos = pos + xi;
                            }
                        }
                        ybase = ybase + yi;
                    }

                    m = m * rot;

                    frame += 1;

                    m4vga::measurement::sig_d_clear();
                    vga.sync_to_vblank();
                    writer.reset(&thread);
                }
            })
}

fn tex_fetch(u: f32, v: f32) -> u32 {
    u as i32 as u32 ^ v as i32 as u32
}

fn u32_as_u8_mut(r: &mut [u32; BUFFER_STRIDE]) -> &mut [u8; WIDTH] {
    assert_eq!(BUFFER_STRIDE * 4, WIDTH);
    unsafe {
        core::mem::transmute(r)
    }
}

/// A specialized framebuffer structure with two features:
///
/// 1. The framebuffer is split into two parts, because the whole thing won't
///    fit into any single RAM.
/// 2. It checks for aliasing on a scanline granularity so rendering can race
///    scanout more aggressively.
struct RaceBuffer {
    segments: [&'static mut [[u32; BUFFER_STRIDE]; HALF_HEIGHT]; 2],
    write_mark: AtomicUsize,
}

impl RaceBuffer {
    pub fn new(segments: [&'static mut [[u32; BUFFER_STRIDE]; HALF_HEIGHT]; 2])
        -> Self
    {
        RaceBuffer {
            segments,
            write_mark: 0.into(),
        }
    }

    pub fn split(&mut self) -> (RaceReader, RaceWriter) {
        (
            RaceReader {
                // Safety: this is an &mut, it cannot be null
                buf: unsafe { NonNull::new_unchecked(self) },
                _life: PhantomData,
            },
            RaceWriter {
                // Safety: this is an &mut, it cannot be null
                buf: unsafe { NonNull::new_unchecked(self) },
                _life: PhantomData,
            },
        )
    }
}

struct RaceReader<'a> {
    buf: NonNull<RaceBuffer>,
    _life: PhantomData<&'a ()>,
}

unsafe impl<'a> Send for RaceReader<'a> {}

impl<'a> RaceReader<'a> {
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    pub fn take_line<'r, P>(&'r mut self,
                            line_number: usize,
                            _: &'r P) -> &'r [u32; BUFFER_STRIDE]
        where P: priority::InterruptPriority,
    {
        let rendered = self.load_writer_progress();
        if line_number < rendered {
            let (seg_index, line_number) = if line_number < HALF_HEIGHT {
                (0, line_number)
            } else {
                (1, line_number - HALF_HEIGHT)
            };

            // Safety: the RaceWriter will only vend mutable references to lines
            // above `rendered`.
            unsafe {
                &self.buf.as_ref().segments[seg_index][line_number]
            }
        } else {
            panic!("tearing: scanout reached {} but rendering only {}",
                   line_number, rendered);
        }
    }
}

struct RaceWriter<'a> {
    buf: NonNull<RaceBuffer>,
    _life: PhantomData<&'a ()>,
}

impl<'a> RaceWriter<'a> {
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    pub fn generate_line(&mut self,
                         _: &priority::Thread) -> GenGuard {
        let line_number = self.load_writer_progress();
        let (seg_index, line_number) = if line_number < HALF_HEIGHT {
            (0, line_number)
        } else {
            (1, line_number - HALF_HEIGHT)
        };
        let buf = unsafe { self.buf.as_mut() };
        GenGuard {
            counter: &buf.write_mark,
            data: &mut buf.segments[seg_index][line_number],
        }
    }

    pub fn reset(&mut self, _: &priority::Thread) {
        unsafe { self.buf.as_ref() }.write_mark.store(0, Ordering::Relaxed)
    }
}

struct GenGuard<'a> {
    counter: &'a AtomicUsize,
    data: &'a mut [u32; BUFFER_STRIDE],
}

impl<'a> Drop for GenGuard<'a> {
    fn drop(&mut self) {
        self.counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl<'a> core::ops::Deref for GenGuard<'a> {
    type Target = [u32; BUFFER_STRIDE];
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
