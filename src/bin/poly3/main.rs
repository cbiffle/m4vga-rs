#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use cortex_m::singleton;
use stm32f4;
use stm32f4::stm32f407::interrupt;

use math::{Augment, HomoTransform, Mat4f, Project, Vec3, Vec3f, Vec3i};

use m4vga::util::rw_lock::ReadWriteLock;

mod render;

use render::{Raster, Tri};

extern "C" {
    fn fast_fill(start: *mut u8, end: *const u8, value: u8);
}

fn fill(range: &mut [u8], value: u8) {
    let start = range.as_mut_ptr();
    unsafe {
        let end = start.add(range.len());
        fast_fill(start, end, value)
    }
}

#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    entry()
}

const VERTEX_COUNT: usize = 8;

static VERTICES: [Vec3f; VERTEX_COUNT] = [
    Vec3(-10., 10., 10.),
    Vec3(10., 10., 10.),
    Vec3(10., -10., 10.),
    Vec3(-10., -10., 10.),
    Vec3(-10., 10., -10.),
    Vec3(10., 10., -10.),
    Vec3(10., -10., -10.),
    Vec3(-10., -10., -10.),
];

static TRIS: &[Tri] = &[
    Tri {
        vertex_indices: [0, 1, 2],
        color: 0b000011,
    },
    Tri {
        vertex_indices: [0, 2, 3],
        color: 0b000011,
    },
    Tri {
        vertex_indices: [1, 5, 6],
        color: 0b110000,
    },
    Tri {
        vertex_indices: [1, 6, 2],
        color: 0b110000,
    },
    Tri {
        vertex_indices: [4, 0, 3],
        color: 0b001100,
    },
    Tri {
        vertex_indices: [4, 3, 7],
        color: 0b001100,
    },
    Tri {
        vertex_indices: [5, 4, 7],
        color: 0b110011,
    },
    Tri {
        vertex_indices: [5, 7, 6],
        color: 0b110011,
    },
    Tri {
        vertex_indices: [4, 5, 1],
        color: 0b111100,
    },
    Tri {
        vertex_indices: [4, 1, 0],
        color: 0b111100,
    },
    Tri {
        vertex_indices: [3, 2, 6],
        color: 0b001111,
    },
    Tri {
        vertex_indices: [3, 6, 7],
        color: 0b001111,
    },
];

static RASTER: ReadWriteLock<Option<Raster>> = ReadWriteLock::new(None);

fn entry() -> ! {
    *RASTER.lock_mut() = Some(Raster::default());

    let transformed =
        singleton!(: [Vec3i; VERTEX_COUNT] = [Vec3(0,0,0); VERTEX_COUNT])
            .unwrap();

    let projection = Mat4f::translate((400., 300., 0.).into())
        * Mat4f::scale((300., 300., 1.).into())
        * Mat4f::perspective(-10., -10., 10., 10., 20., 100.)
        * Mat4f::translate((0., 0., -50.).into());

    let mut frame = 0;

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
            |ln, tgt, ctx, _| {
                m4vga::util::measurement::sig_d_set();
                let mut left_margin = 800;
                let mut right_margin = 0;
                RASTER
                    .try_lock_mut()
                    .expect("rast access")
                    .as_mut()
                    .unwrap()
                    .step(ln, |span, color| {
                        left_margin = left_margin.min(span.start);
                        right_margin = right_margin.max(span.end);
                        fill(&mut tgt[span.clone()], color);
                    });
                fill(&mut tgt[..left_margin], 0);
                if right_margin > left_margin {
                    fill(&mut tgt[right_margin..800], 0);
                }

                m4vga::util::measurement::sig_d_clear();
                ctx.target_range = 0..800; // 800 pixels now valid
            },
            |vga| loop {
                vga.sync_to_vblank();
                let model = Mat4f::rotate_y(frame as f32 * 0.1)
                    * Mat4f::rotate_z(frame as f32 * 0.05);
                let modelview = projection * model;
                for (t, s) in transformed.iter_mut().zip(VERTICES.iter()) {
                    let Vec3(x, y, z) = (modelview * s.augment()).project();
                    *t = Vec3(x as i32, y as i32, z as i32);
                }
                RASTER.lock_mut().as_mut().unwrap().reset(TRIS, transformed);
                vga.video_on();
                frame += 1;
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
