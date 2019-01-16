//! Text renderer using 10x16 pixel character cells.

use crate::arena::{self, Arena};
use crate::rast::{Rasterize, RasterInfo, Pixel};

const GLYPH_COLS: usize = 10;
const GLYPH_ROWS: usize = 16;
const FONT_CHARS: usize = 256;

extern {
    fn unpack_text_10p_attributed_impl(input_line: *const u32,
                                       font: *const u8,
                                       render_target: *const Pixel,
                                       cols_in_input: usize);
}

pub struct Text10x16<'arena> {
    /// Number of text columns in the display.
    cols: usize,
    /// Number of text rows in the display.
    rows: usize,
    /// Display line number of the top line of this rasterizer's field. Can also
    /// be used for scroll effects.
    top_line: usize,
    /// ???
    hide_right: bool,
    /// ???
    x_adj: i32,
    /// Font buffer. The rasterizer owns a copy of the font to ensure that it
    /// gets copied out of ROM, where accesses would be slower.
    font: arena::Box<'arena, [u8]>,
    /// "Framebuffer" holding attributed characters.
    fb: arena::Box<'arena, [u32]>, // TODO: better type than u32
}

impl<'arena> Text10x16<'arena> {
    pub fn new_in(arena: &'arena Arena,
                  font: &[u8; GLYPH_ROWS * FONT_CHARS],
                  width: usize,
                  height: usize,
                  top_line: usize,
                  hide_right: bool)
        -> Self
    {
        let cols = (width + (GLYPH_COLS - 1)) / GLYPH_COLS;
        let rows = (height + (GLYPH_ROWS - 1)) / GLYPH_ROWS;

        Text10x16 {
            cols,
            rows,
            top_line,
            hide_right,
            x_adj: 0,
            font: arena.alloc_slice_copy(font).unwrap(),
            fb: arena.alloc_slice_default(cols * rows).unwrap(),
        }
    }
}

impl<'arena> Rasterize for Text10x16<'arena> {
    fn rasterize(&mut self,
                 cycles_per_pixel: u32,
                 line_number: usize,
                 target: &mut [Pixel])
        -> RasterInfo
    {
        let line_number = line_number - self.top_line;

        let text_row = line_number / GLYPH_ROWS;
        let row_in_glyph = line_number % GLYPH_ROWS;
        if text_row >= self.rows {
            return RasterInfo { cycles_per_pixel, ..RasterInfo::default() }
        }

        debug_assert!(text_row < self.rows);

        let src = {
            let offset = text_row * self.cols;
            &self.fb[offset .. offset + self.cols]
        };
        let font = {
            let offset = row_in_glyph * FONT_CHARS;
            &self.font[offset .. offset + FONT_CHARS]
        };

        // If the x_adj is positive, stretch the first character's background
        // into the left margin.
        if self.x_adj > 0 {
            let bg = (src[0] >> 8) as Pixel;
            for t in &mut target[0..self.x_adj as usize] {
                *t = bg
            }
        } else if self.x_adj < 0 {
            // Fill in the right margin.
            let bg = (src[self.cols - 1] >> 8) as Pixel;
            for t in &mut target[self.cols * GLYPH_COLS - (-self.x_adj as usize) ..] {
                *t = bg
            }
        }

        unsafe {
            unpack_text_10p_attributed_impl(
                // We know src points to at least 'cols' worth of valid data, so
                // this will not result in any out-of-range accesses:
                src.as_ptr(),
                // We likewise know that there are FONT_CHARS worth of font
                // bytes.
                font.as_ptr(),
                // TODO: the way x_adj was implemented in C++ is safe-ish if you
                // squint, but relies on some detailed knowledge of buffer
                // memory layouts. We need to find a better way. Until then,
                // smooth text scrolling doesn't work.
                target.as_ptr(),
                self.cols);
        }

        RasterInfo {
            length: self.cols + GLYPH_COLS
                - if self.hide_right { GLYPH_COLS } else { 0 },
            cycles_per_pixel,
            ..RasterInfo::default()
        }
    }
}
