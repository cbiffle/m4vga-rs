//! 3D wireframe rook demo.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

mod model;

use gfx;

use core::sync::atomic::{AtomicUsize, Ordering};

use stm32f4;

use m4vga::math::{
    Augment, HomoTransform, Mat4f, Matrix, Project, Vec2, Vec2i, Vec3f,
};
use m4vga::rast::text_10x16::AChar;
use m4vga::util::rw_lock::ReadWriteLock;
use stm32f4::stm32f407::interrupt;

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    entry()
}

fn entry() -> ! {
    let front = ReadWriteLock::new({
        // We can't fit two buffers in bitband-accessible SRAM, so this one
        // lives in CCM. This means we can't render into it directly. Instead,
        // we'll *copy* the contents of `DRAW_BUF` at each vblank.
        #[link_section = ".local_bss"]
        static mut FRONT_BUF: [u32; 800 * 600 / 32] = [0; 800 * 600 / 32];

        // Safety: we can determine by lexical scoping that this is the only
        // possible mutable reference to FRONT_BUF.
        unsafe { &mut FRONT_BUF }
    });

    let mut bg: gfx::PackedBitBuffer<'static> = {
        // This buffer goes in default SRAM, which is where bitbanding can
        // reach.
        static mut DRAW_BUF: [u32; 800 * 600 / 32] = [0; 800 * 600 / 32];

        // Safety: we can determine by lexical scoping that this is the only
        // possible mutable reference to DRAW_BUF.
        gfx::PackedBitBuffer::new(
            sram112_alias(unsafe { &mut DRAW_BUF }),
            800 / 32,
        )
    };

    let vertex_buf: &mut [Vec2i; model::VERTEX_COUNT] = {
        static mut VBUF: [Vec2i; model::VERTEX_COUNT] =
            [Vec2(0, 0); model::VERTEX_COUNT];
        unsafe { &mut VBUF }
    };

    let message = ReadWriteLock::new({
        static mut BUF: [AChar; 81] = [AChar::from_ascii_char(b' '); 81];
        unsafe { &mut BUF }
    });

    {
        let mut message = message.lock_mut();
        // chars/10:0         1         2         3         4
        let text = b"2450 triangles made from 5151 segments, \
                 drawn at 60Hz at 800x600, mixed mode -- ";
        for (dst, &b) in message.iter_mut().zip(text as &[_]) {
            *dst = AChar::from_ascii_char(b).with_foreground(0b101010);
        }
        for c in &mut message[0..15] {
            *c = c.with_foreground(0b111111);
        }
        for c in &mut message[25..38] {
            *c = c.with_foreground(0b111111).with_background(0b110000);
        }
        for c in &mut message[49..53] {
            *c = c.with_foreground(0b000011);
        }
    }

    let clut = AtomicUsize::new(0xFF00);

    let mut projection = Mat4f::translate((800. / 2., 600. / 2., 0.).into())
        * Mat4f::scale((600. / 2., 600. / 2., 1.).into())
        * Mat4f::perspective(-10., -10., 10., 10., 20., 100.)
        * Mat4f::translate((0., 0., -75.).into());

    let rot_step_z = Mat4f::rotate_z(0.01);
    let rot_step_y = Mat4f::rotate_y(0.02);
    let mut model = Mat4f::rotate_y(3.1415926 / 2.);

    let fine_scroll = AtomicUsize::new(0);

    let edges = check_edges(&model::EDGES);

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx, _| {
                if ln == 0 || ln == 516 {
                    m4vga::rast::solid_color_fill(tgt, ctx, 800, 0);
                    ctx.repeat_lines = 99;
                    return;
                }

                if ln < 500 {
                    // Bitmapped wireframe display.
                    m4vga::measurement::sig_d_set();

                    let front = front.try_lock().expect("front unavail");

                    let offset = ln * (800 / 32);
                    m4vga::rast::bitmap_1::unpack(
                        &front[offset..offset + (800 / 32)],
                        &clut,
                        &mut tgt[0..800],
                    );
                    ctx.target_range = 0..800; // 800 pixels now valid
                    m4vga::measurement::sig_d_clear();
                } else {
                    // Text display. This implements the "smooth" part of smooth
                    // scrolling: honoring the `fine_scroll` value by shifting
                    // the display to the left. We do this by adjusting our
                    // `tgt` slice.
                    let fs = fine_scroll.load(Ordering::Relaxed);
                    m4vga::rast::text_10x16::unpack(
                        &**message.try_lock().expect("message unavail"),
                        m4vga::font_10x16::FONT.as_glyph_slices(),
                        &mut tgt[16 - fs..],
                        ln - 500,
                        81,
                    );
                    ctx.target_range = 16..816;
                }
            },
            // This closure contains the main loop of the program.
            |vga| loop {
                vga.sync_to_vblank();
                // This multi-step check-and-store sequence is okay because
                // we're the only writer.
                let fs = fine_scroll.load(Ordering::Acquire);
                if fs == 9 {
                    fine_scroll.store(0, Ordering::Release);
                    message.lock_mut().rotate_left(1);
                } else {
                    fine_scroll.store(fs + 1, Ordering::Release);
                }

                m4vga::measurement::sig_c_set();
                // Copy draw buffer to front buffer. We are racing here: we must
                // release the lock on front before scanout starts, or we'll
                // panic.
                m4vga::util::copy_words::copy_words(
                    bg.as_word_slice(),
                    &mut **front.lock_mut(),
                );
                m4vga::measurement::sig_c_clear();

                bg.clear();

                transform_vertices(
                    projection * model,
                    &model::VERTICES,
                    vertex_buf,
                );

                m4vga::measurement::sig_c_set();
                draw_edges(&mut bg, edges, vertex_buf);
                m4vga::measurement::sig_c_clear();

                model = model * rot_step_y.clone();
                projection = projection * rot_step_z.clone();

                vga.video_on();
            },
        )
}

/// Transforms a vertex slice and projects to 2D.
fn transform_vertices(m: Mat4f, vertices: &[Vec3f], out: &mut [Vec2i]) {
    for (dst, src) in out.iter_mut().zip(vertices) {
        let v = (m * src.augment()).project();
        *dst = (v.0 as i32, v.1 as i32).into();
    }
}

fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && x < 800 && y >= 0 && y < 600
}

/// Draws wireframe edges into `buf`.
///
/// This takes a pre-transformed point cloud, `vertex_table`, and draws edges
/// connecting points in the cloud as specified in `edge_table`.
fn draw_edges(
    buf: &mut gfx::PackedBitBuffer,
    edge_table: &[CheckedEdge],
    vertex_table: &[Vec2i; model::VERTEX_COUNT],
) {
    let mut bits = buf.as_bits();
    for edge in edge_table {
        let (p0, p1) = vertex_lookup(vertex_table, edge);

        bits.draw_line_unclipped(
            p0.0 as usize,
            p0.1 as usize,
            p1.0 as usize,
            p1.1 as usize,
        )
    }
}

fn sram112_alias<T>(slice: &mut [T]) -> &mut [T] {
    const SRAM_SIZE: usize = 112 * 1024;

    let addr = slice.as_ptr() as usize;
    assert!(addr < SRAM_SIZE);
    match slice.last() {
        None => &mut [],
        Some(r) => {
            assert!((r as *const _ as usize) < SRAM_SIZE);
            unsafe {
                core::slice::from_raw_parts_mut(
                    (addr | 0x20000000) as *mut T,
                    slice.len(),
                )
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
struct CheckedEdge((usize, usize));

fn check_edges(raw: &[(usize, usize)]) -> &[CheckedEdge] {
    for &(s, e) in raw {
        assert!(s < model::VERTEX_COUNT && e < model::VERTEX_COUNT);
    }
    // Safety: CheckedEdge is a transparent struct that simply adds type
    // information.
    unsafe { core::mem::transmute(raw) }
}

fn vertex_lookup<'v>(
    vtable: &'v [Vec2i; model::VERTEX_COUNT],
    edge: &CheckedEdge,
) -> (&'v Vec2i, &'v Vec2i) {
    // Safety: CheckedEdge means the indices have already been checked.
    unsafe {
        (
            vtable.get_unchecked((edge.0).0),
            vtable.get_unchecked((edge.0).1),
        )
    }
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
