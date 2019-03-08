//! 3D wireframe rook demo.
//!
//! # Theory of operation
//!
//! This demo combines a wireframe rendering of a tumbling chess piece with
//! smooth-scrolling multi-colored text.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

mod model;

use core::sync::atomic::{AtomicUsize, Ordering};
use cortex_m::singleton;
use stm32f4;
use stm32f4::stm32f407::interrupt;

use gfx;
use m4vga::math::{Augment, HomoTransform, Mat4f, Project, Vec2, Vec2i, Vec3f};
use m4vga::rast::text_10x16::AChar;
use m4vga::util::rw_lock::ReadWriteLock;

// We can't fit a second buffer in bitband-accessible RAM, so the `front` buffer
// goes in CCM. This means we can't render into it directly using bitbanding,
// and we'll have to *copy* pixels from `back` to `front` during vsync.
//
// Because `front` is shared between the render loop and the rasterizer, we wrap
// it in a lock.
#[link_section = ".local_bss"]
static FRONT_BUF: ReadWriteLock<[u32; 800 * 600 / 32]> =
    ReadWriteLock::new([0; 800 * 600 / 32]);

// We need somewhere to store the text that we scroll across the screen. We will
// update it in-place, so it needs to be mutable. It's shared between the render
// loop and the text rasterizer, so it needs to be in a lock.
static MESSAGE: ReadWriteLock<[AChar; 81]> =
    ReadWriteLock::new([AChar::from_ascii_char(b' '); 81]);

/// Demo entry point and main loop. This is factored out of `main` because the
/// `cortex_m_rt` `entry` attribute mis-reports all error locations, making code
/// hard to debug.
fn entry() -> ! {
    // We need two framebuffers, which we'll call `front` and `back`.

    // The `back` buffer needs to live in the bitband target region of the
    // address space, so that our line-drawing code can get to it. An array
    // without any section attribute goes there by default per our linker
    // script.
    let back =
        singleton!(: [u32; 800 * 600 / 32] = [0; 800 * 600 / 32]).unwrap();
    let mut back = gfx::PackedBitBuffer::new(sram112_alias(back), 800 / 32);

    // Each vertex in the model is shared by multiple triangles. It would
    // therefore be wasteful to transform each triangle separately. Instead, we
    // save time by transforming all the unique vertices and writing their
    // screen-space projected versions into `vertex_buf` each frame.
    let vertex_buf = singleton!(
        : [Vec2i; model::VERTEX_COUNT] = [Vec2(0, 0); model::VERTEX_COUNT])
    .unwrap();

    // Foreground and background colors for the bitmap display:
    let clut = AtomicUsize::new(0xFF_00);

    // Base projection matrix, will be updated to animate.
    let mut projection = Mat4f::translate((800. / 2., 600. / 2., 0.).into())
        * Mat4f::scale((600. / 2., 600. / 2., 1.).into())
        * Mat4f::perspective(-10., -10., 10., 10., 20., 100.)
        * Mat4f::translate((0., 0., -75.).into());

    // Base model matrix, will be updated to animate.
    let mut model = Mat4f::rotate_y(3.1415926 / 2.);

    // Rotation steps around each animated axis.
    let rot_step_z = Mat4f::rotate_z(0.01);
    let rot_step_y = Mat4f::rotate_y(0.02);

    // This scans the static `EDGES` array at startup, doing all bounds checking
    // in advance, and returns a slice of `CheckedEdge`s that are cheaper to
    // draw.
    let edges = checked::VertexIndex::check_edges(&model::EDGES);

    // Text time!

    fill_message(&mut *MESSAGE.lock_mut());

    // Values between 1 and 9 slide rendered text by that many pixels to the
    // left. This is how we achieve smooth scrolling: we increment this to 9,
    // and then move the entire message left by one character while resetting
    // this to zero.
    let fine_scroll = AtomicUsize::new(0);

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx, _| {
                if ln == 0 || ln == 516 {
                    // The top and bottom of the screen use the cheapest
                    // rasterizer to draw empty space, to save CPU.
                    m4vga::rast::solid_color_fill(tgt, ctx, 800, 0);
                    ctx.repeat_lines = 99;
                    return;
                }

                if ln < 500 {
                    // Bitmapped wireframe display.
                    m4vga::measurement::sig_d_set();

                    let front = FRONT_BUF.try_lock().expect("front unavail");

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
                        &*MESSAGE.try_lock().expect("message unavail"),
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

                // Update the fine-scrolling state machine.  This multi-step
                // check-and-store sequence is okay because we're the only
                // writer.
                let fs = fine_scroll.load(Ordering::Acquire);
                if fs == 9 {
                    fine_scroll.store(0, Ordering::Release);
                    MESSAGE.lock_mut().rotate_left(1);
                } else {
                    fine_scroll.store(fs + 1, Ordering::Release);
                }

                m4vga::measurement::sig_c_set();
                // Copy draw buffer to front buffer. We are racing here: we must
                // release the lock on front before scanout starts, or we'll
                // panic.
                m4vga::util::copy_words::copy_words(
                    back.as_word_slice(),
                    &mut *FRONT_BUF.lock_mut(),
                );
                m4vga::measurement::sig_c_clear();

                back.clear();

                transform_vertices(
                    projection * model,
                    &model::VERTICES,
                    vertex_buf,
                );

                m4vga::measurement::sig_c_set();
                draw_edges(&mut back, edges, vertex_buf);
                m4vga::measurement::sig_c_clear();

                // Animate:
                model = model * rot_step_y.clone();
                projection = projection * rot_step_z.clone();

                vga.video_on();
            },
        )
}

/// Fills in the default message.
fn fill_message(message: &mut [AChar; 81]) {
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

/// Transforms a vertex slice and projects to 2D.
fn transform_vertices(m: Mat4f, vertices: &[Vec3f], out: &mut [Vec2i]) {
    for (dst, src) in out.iter_mut().zip(vertices) {
        let v = (m * src.augment()).project();
        *dst = (v.0 as i32, v.1 as i32).into();
    }
}

/// Draws wireframe edges into `buf`.
///
/// This takes a pre-transformed point cloud, `vertex_table`, and draws edges
/// connecting points in the cloud as specified in `edge_table`.
fn draw_edges(
    buf: &mut gfx::PackedBitBuffer,
    edge_table: &[(checked::VertexIndex, checked::VertexIndex)],
    vertex_table: &[Vec2i; model::VERTEX_COUNT],
) {
    let mut bits = buf.as_bits();
    for (start, end) in edge_table {
        let p0 = start.lookup(vertex_table);
        let p1 = end.lookup(vertex_table);

        bits.draw_line_unclipped(
            p0.0 as usize,
            p0.1 as usize,
            p1.0 as usize,
            p1.1 as usize,
        )
    }
}

/// Maps an address from the zero-based alias of SRAM112 into its native space
/// starting at `0x2000_0000`. This is required to use bitbanding.
///
/// Since the input slice is mutably borrowed, the alias is safe.
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

mod checked {
    use super::model;

    /// An index that has been verified to be less than `model::VERTEX_COUNT`.
    /// This lets us safely skip bounds checking when the index is repeatedly
    /// reused.
    ///
    /// Because the field is private, code outside this module can't violate
    /// this invariant (without `unsafe` shenanigans, of course.)
    ///
    /// (In a glorious future of const-generics we could generalize this
    /// concept, but for now, this is what I need.)
    #[derive(Copy, Clone, Debug)]
    #[repr(transparent)]
    pub struct VertexIndex(u16);

    impl VertexIndex {
        pub fn check_edges(
            raw: &[(u16, u16)],
        ) -> &[(VertexIndex, VertexIndex)] {
            // Verify that all indices in `raw` are valid vertex indices.
            for (i, &(start, end)) in raw.iter().enumerate() {
                assert!(
                    usize::from(start) < model::VERTEX_COUNT
                        && usize::from(end) < model::VERTEX_COUNT,
                    "Vertex index out of range at edge table [{}]",
                    i
                );
            }
            // Safety: `VertexIndex` is `repr(transparent)`, so we can
            // reinterpret a collection of `usize` safely. We have just verified
            // `VertexIndex`'s invariant above.
            unsafe { core::mem::transmute(raw) }
        }

        pub fn lookup<T>(self, table: &[T; model::VERTEX_COUNT]) -> &T {
            // Safety: the only way for someone outside this module to obtain a
            // `VertexIndex` is through `check_edges`, which has already
            // performed the bounds check.
            unsafe { table.get_unchecked(usize::from(self.0)) }
        }
    }
}

/// Entry point for runtime.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    entry()
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
