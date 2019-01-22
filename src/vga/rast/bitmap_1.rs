use core::sync::atomic::{Ordering, AtomicBool, AtomicUsize};
use core::mem;
use core::ptr::null_mut;

use crate::copy_words::copy_words;
use crate::vga::rast::{Pixel, RasterCtx};
use crate::arena::{self, Arena};

extern {
    #[allow(improper_ctypes)]
    fn unpack_1bpp_impl(input_line: *const u32,
                        clut: *const AtomicUsize,
                        render_target: *mut Pixel,
                        words_in_input: u32);

    #[allow(improper_ctypes)]
    fn unpack_1bpp_overlay_impl(input_line: *const u32,
                                clut: *const AtomicUsize,
                                render_target: *mut Pixel,
                                words_in_input: u32,
                                background: *const u8);
}

#[derive(Debug)]
pub struct Bitmap1<'arena> {
    lines: usize,
    words_per_line: usize,
    top_line: usize,
    flip_pended: AtomicBool,
    clut: AtomicUsize,
    fb0: arena::Box<'arena, [u32]>,
    fb1: arena::Box<'arena, [u32]>,
    use_fb1: AtomicBool,
    background: Option<&'arena [Pixel]>,
}

impl<'arena> Bitmap1<'arena> {
    pub fn new_in(arena: &'arena Arena,
                  width: usize,
                  height: usize,
                  top_line: usize,
                  background: Option<&'arena [Pixel]>)
        -> Self
    {
        let words_per_line = width / 32;
        Bitmap1 {
            lines: height,
            words_per_line,
            top_line,
            flip_pended: false.into(),
            clut: 0xFF00.into(),
            fb0: arena.alloc_slice_default(words_per_line * height).unwrap(),
            fb1: arena.alloc_slice_default(words_per_line * height).unwrap(),
            use_fb1: false.into(),
            background,
        }
    }

    pub fn pend_flip(&self) {
        self.flip_pended.store(true, Ordering::Relaxed)
    }

    fn flip_now(&self) {
        self.use_fb1.fetch_xor(true, Ordering::Release);
    }

    pub fn set_fg_color(&self, c: Pixel) {
        self.update_clut(0x00FF, (c as usize) << 8)
    }

    pub fn set_bg_color(&self, c: Pixel) {
        self.update_clut(0xFF00, c as usize)
    }

    fn update_clut(&self, mask: usize, value: usize) {
        let mut old = self.clut.load(Ordering::Relaxed);
        loop {
            let new = (old & mask) | value;
            match self.clut.compare_exchange_weak(old,
                                                  new,
                                                  Ordering::Relaxed,
                                                  Ordering::Relaxed) {
                Ok(_) => break,
                Err(x) => old = x,
            }
        }
    }

    pub fn copy_bg_to_fg(&mut self) {
        if self.use_fb1.load(Ordering::Relaxed) {
            copy_words(&self.fb0, &mut self.fb1)
        } else {
            copy_words(&self.fb1, &mut self.fb0)
        }
    }

    fn rasterize(&self,
                 line_number: usize,
                 ctx: &mut RasterCtx) {
        let line_number = line_number.wrapping_sub(self.top_line);

        if line_number == 0 {
            if self.flip_pended.swap(false, Ordering::Relaxed) {
                self.flip_now()
            }
        }

        if line_number >= self.lines {
            // leave target_range empty, producing black output
            return
        }

        let front = if self.use_fb1.load(Ordering::Acquire) {
            &self.fb1
        } else {
            &self.fb0
        };

        if let Some(bg) = self.background {
            unsafe {
                unpack_1bpp_overlay_impl(
                    &front[self.words_per_line * line_number],
                    &self.clut,
                    ctx.target.as_mut_ptr(),
                    self.words_per_line as u32,
                    &bg[0],
                );
            }
        } else {
            unsafe {
                unpack_1bpp_impl(
                    &front[self.words_per_line * line_number],
                    &self.clut,
                    ctx.target.as_mut_ptr(),
                    self.words_per_line as u32,
                );
            }
        }

        ctx.target_range = 0 .. self.words_per_line * 32;
    }
}

fn test(vga: &mut crate::vga::Vga<crate::vga::Idle>) {
    let mut arena = unsafe {
        Arena::from_pointers(null_mut(), null_mut())
    };
    let bitmap = arena.alloc(Bitmap1::new_in(&arena, 800, 600, 0, None))
        .unwrap();

    vga.with_raster(
        |ln, ctx| bitmap.rasterize(ln, ctx),
        |vga| {
            // do the stuff
        },
    );

    drop(bitmap);

    arena.reset();
}
