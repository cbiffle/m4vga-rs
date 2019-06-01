#![cfg_attr(not(feature = "std"), no_std)]

use m4vga::util::spin_lock::SpinLock;

pub mod table;
pub mod render;

pub const NATIVE_WIDTH: usize = 800;
pub const NATIVE_HEIGHT: usize = 600;
const SCALE: usize = 2;

pub const WIDTH: usize = NATIVE_WIDTH / SCALE;
pub const HEIGHT: usize = NATIVE_HEIGHT / SCALE;
pub const HALF_WIDTH: usize = WIDTH / 2;
pub const HALF_HEIGHT: usize = HEIGHT / 2;

const BUFFER_SIZE: usize = WIDTH * HALF_HEIGHT;
pub const BUFFER_WORDS: usize = BUFFER_SIZE / 4;
pub const BUFFER_STRIDE: usize = WIDTH / 4;

#[cfg(target_os = "none")]
mod bare;
#[cfg(target_os = "none")]
pub use bare::*;

pub fn raster_callback<B>(
    ln: usize,
    target: &mut m4vga::rast::TargetBuffer,
    ctx: &mut m4vga::rast::RasterCtx,
    fg: &SpinLock<B>,
)
where B: AsMut<[u32]> + Send,
{
    // Our image is slightly smaller than the display. Black the top and
    // bottom borders.
    if ln < 4 || ln > 595 {
        m4vga::rast::solid_color_fill(target, ctx, 800, 0);
        return;
    }

    let mut buf = fg.try_lock().expect("rast fg access");
    let buf = buf.as_mut();

    let ln = ln / SCALE;

    if ln < HALF_HEIGHT {
        m4vga::rast::direct::direct_color(ln, target, ctx, buf, BUFFER_STRIDE);
    } else {
        m4vga::rast::direct::direct_color_mirror(
            ln,
            target,
            ctx,
            buf,
            BUFFER_STRIDE,
            HEIGHT,
        );
    }

    ctx.cycles_per_pixel *= SCALE;
    ctx.repeat_lines = SCALE - 1;
}

pub fn render_frame<B>(
    bg: &mut B,
    fg: &SpinLock<B>,
    table: &table::Table,
    frame: usize,
)
    where B: AsMut<[u32]> + Send,
{
    core::mem::swap(bg, &mut *fg.try_lock().expect("swap access"));
    let bg = u32_as_u8_mut(bg.as_mut());
    m4vga::util::measurement::sig_d_set();
    self::render::render(table, bg, frame);
    m4vga::util::measurement::sig_d_clear();
}

fn u32_as_u8_mut(slice: &mut [u32]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            slice.len() * 4,
        )
    }
}
