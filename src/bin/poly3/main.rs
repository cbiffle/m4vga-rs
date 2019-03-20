#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use cortex_m::singleton;
use stm32f4;
use stm32f4::stm32f407::interrupt;

use math::{
    Augment, HomoTransform, Mat4f, Project, Vec3, Vec3f, Vec3i, Vector,
};

use m4vga::util::rw_lock::ReadWriteLock;

mod model;
mod render;

use render::Raster;

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

static RASTER: ReadWriteLock<Raster> = ReadWriteLock::new(Raster::new());

const LIGHT: Vec3f = Vec3(-0.577, 0.577, 0.577);

fn entry() -> ! {
    let transformed = singleton!(: [Vec3i; model::VERTEX_COUNT] =
                     [Vec3(0,0,0); model::VERTEX_COUNT])
    .unwrap();

    let transformed_n = singleton!(: [Vec3f; model::NORMAL_COUNT] =
                     [Vec3(0.,0.,0.); model::NORMAL_COUNT])
    .unwrap();

    let projection = Mat4f::translate((400., 300., 0.).into())
        * Mat4f::scale((300., 300., 300.).into())
        * Mat4f::perspective(-10., -10., 10., 10., 20., 100.)
        * Mat4f::translate((0., 0., -70.).into());

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
                    .step(ln, |span, _color, normal| {
                        let color =
                            ((normal.dot(LIGHT) + 1.) * 1.7) as u8 * 0b010101;
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
                let model = Mat4f::rotate_y(frame as f32 * 0.05)
                    * Mat4f::rotate_z(frame as f32 * 0.025);
                let modelview = projection * model;

                // Project vertices into screen space.
                for (t, s) in
                    transformed.iter_mut().zip(model::VERTICES.iter())
                {
                    let Vec3(x, y, z) = (modelview * s.augment()).project();
                    *t = Vec3(x as i32, y as i32, z as i32);
                }

                // Project normals into model space.
                for (t, n) in
                    transformed_n.iter_mut().zip(model::NORMALS.iter())
                {
                    *t = (model * n.augment()).project();
                }

                RASTER.lock_mut()
                    .reset(&model::TRIS, transformed, transformed_n);
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
