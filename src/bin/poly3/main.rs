#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use stm32f4;
use stm32f4::stm32f407::interrupt;

use m4vga::util::rw_lock::ReadWriteLock;
use m4vga::math::{Vec3, Vec3i};

mod render;

use render::{Raster, Tri};

extern "C" {
    fn fast_fill(
        start: *mut u8,
        end: *const u8,
        value: u8,
    );
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

static VERTICES: &[Vec3i] = &[
    Vec3(40, 40, 0),
    Vec3(700, 200, 0),
    Vec3(300, 500, 0),
    Vec3(400, 500, 0),
    Vec3(400, 40, 0),
];

static TRIS: &[Tri] = &[
    Tri {
        vertex_indices: [0, 1, 2],
        color: 0b00_00_11,
    },
    Tri {
        vertex_indices: [2, 1, 3],
        color: 0b00_11_00,
    },
    Tri {
        vertex_indices: [0, 4, 1],
        color: 0b11_00_00,
    },
];

static RASTER: ReadWriteLock<Option<Raster>> = ReadWriteLock::new(None);

fn entry() -> ! {
    *RASTER.lock_mut() = Some(Raster::default());

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels. Here, we just scribble a test pattern into
            // the target buffer.
            |ln, tgt, ctx, _| {
                m4vga::measurement::sig_d_set();
                let mut left_margin = 800;
                let mut right_margin = 0;
                RASTER.try_lock_mut()
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

                m4vga::measurement::sig_d_clear();
                ctx.target_range = 0..800; // 800 pixels now valid
            },
            |vga| loop {
                vga.sync_to_vblank();
                RASTER.lock_mut().as_mut().unwrap().reset(TRIS, VERTICES);
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
