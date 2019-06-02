#![no_std]

mod conway;

use core::borrow::Borrow;
use core::sync::atomic::AtomicUsize;
use rand::{Rng, SeedableRng};

use m4vga::util::rw_lock::ReadWriteLock;
use m4vga::Pixel;
use m4vga_fx_common::{Demo, Raster, Render};

pub struct State<B> {
    pub fg: ReadWriteLock<B>,
    pub bg: B,
    pub clut: AtomicUsize,
}

pub struct RasterState<'a, B> {
    fg: &'a ReadWriteLock<B>,
    clut: &'a AtomicUsize,
}

pub struct RenderState<'a, B> {
    fg: &'a ReadWriteLock<B>,
    bg: &'a mut B,
}

impl<B> State<B>
where
    B: AsMut<[u32]>,
{
    pub fn new(
        fg_buf: B,
        mut bg_buf: B,
        fg_color: Pixel,
        bg_color: Pixel,
    ) -> Self {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(11181981);
        for word in bg_buf.as_mut().iter_mut() {
            *word = rng.gen();
        }

        State {
            fg: ReadWriteLock::new(fg_buf),
            bg: bg_buf,
            clut: AtomicUsize::new(
                bg_color as usize | ((fg_color as usize) << 8),
            ),
        }
    }
}

impl<'a, B> Demo<'a> for State<B>
where
    B: AsMut<[u32]> + Borrow<[u32]> + Send + 'a,
{
    type Raster = RasterState<'a, B>;
    type Render = RenderState<'a, B>;

    fn split(&'a mut self) -> (Self::Raster, Self::Render) {
        (
            RasterState {
                fg: &self.fg,
                clut: &self.clut,
            },
            RenderState {
                fg: &self.fg,
                bg: &mut self.bg,
            },
        )
    }
}

impl<'a, B> Raster for RasterState<'a, B>
where
    B: Borrow<[u32]> + Send,
{
    fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        _: m4vga::priority::I0,
    ) {
        m4vga::util::measurement::sig_d_set();

        let fg = self.fg.try_lock().expect("fg unavail");
        let fg = (*fg).borrow();

        m4vga::util::measurement::sig_d_clear();

        let offset = ln * (800 / 32);
        m4vga::rast::bitmap_1::unpack(
            &fg[offset..offset + (800 / 32)],
            &self.clut,
            &mut target[0..800],
        );
        ctx.target_range = 0..800; // 800 pixels now valid
    }
}

impl<'a, B> Render for RenderState<'a, B>
where
    B: AsMut<[u32]> + Borrow<[u32]>,
{
    fn render_frame(&mut self, _: usize, _: m4vga::priority::Thread) {
        core::mem::swap(self.bg, &mut *self.fg.lock_mut());
        conway::step((*self.fg.lock()).borrow(), self.bg.as_mut());
    }
}
