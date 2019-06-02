#![cfg_attr(not(feature = "std"), no_std)]

use m4vga::util::spin_lock::SpinLock;
use m4vga_fx_common::{Demo, Raster, Render};

pub mod render;
pub mod table;

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

pub struct State<B, T> {
    pub fg: SpinLock<B>,
    pub bg: B,
    pub table: T,
}

pub struct RasterState<'a, B> {
    fg: &'a SpinLock<B>,
}

pub struct RenderState<'a, B, T> {
    fg: &'a SpinLock<B>,
    bg: &'a mut B,
    table: &'a T,
}

impl<'a, B, T> Demo<'a> for State<B, T>
where
    B: AsMut<[u32]> + Send + 'a,
    T: core::borrow::Borrow<table::Table> + 'a,
{
    type Raster = RasterState<'a, B>;
    type Render = RenderState<'a, B, T>;

    fn split(&'a mut self) -> (Self::Raster, Self::Render) {
        (
            RasterState { fg: &self.fg },
            RenderState {
                fg: &self.fg,
                bg: &mut self.bg,
                table: &self.table,
            },
        )
    }
}

impl<'a, B> Raster for RasterState<'a, B>
where
    B: AsMut<[u32]> + Send,
{
    fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        _: m4vga::priority::I0,
    ) {
        // Our image is slightly smaller than the display. Black the top and
        // bottom borders.
        if ln < 4 || ln > 595 {
            m4vga::rast::solid_color_fill(target, ctx, 800, 0);
            return;
        }

        let mut buf = self.fg.try_lock().expect("rast fg access");
        let buf = buf.as_mut();

        let ln = ln / SCALE;

        if ln < HALF_HEIGHT {
            m4vga::rast::direct::direct_color(
                ln,
                target,
                ctx,
                buf,
                BUFFER_STRIDE,
            );
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
}

impl<'a, B, T> Render for RenderState<'a, B, T>
where
    B: AsMut<[u32]> + Send,
    T: core::borrow::Borrow<table::Table>,
{
    fn render_frame(&mut self, frame: usize, _: m4vga::priority::Thread) {
        core::mem::swap(
            self.bg,
            &mut *self.fg.try_lock().expect("swap access"),
        );
        let bg = u32_as_u8_mut(self.bg.as_mut());
        m4vga::util::measurement::sig_d_set();
        self::render::render(self.table.borrow(), bg, frame);
        m4vga::util::measurement::sig_d_clear();
    }
}

fn u32_as_u8_mut(slice: &mut [u32]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            slice.len() * 4,
        )
    }
}
