//! Text rasterizer using 10x16 pixel cells.

use crate::Pixel;

pub const GLYPH_COLS: usize = 10;
pub const GLYPH_ROWS: usize = 16;

/// An attributed character cell.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct AChar(u32);

impl AChar {
    // NOTE: it is very important that this representation stay in sync with the
    // one used by the assembly code.

    pub const fn from_ascii_char(c: u8) -> Self {
        AChar(c as u32)
    }

    /// Extracts the ASCII character value of this cell.
    pub const fn ascii_char(self) -> u8 {
        self.0 as u8
    }

    /// Extracts the `char` value of this cell.
    pub const fn char(self) -> char {
        self.ascii_char() as char
    }

    /// Extracts the foreground color.
    pub const fn foreground(self) -> Pixel {
        (self.0 >> 16) as u8
    }

    /// Extracts the background color.
    pub const fn background(self) -> Pixel {
        (self.0 >> 8) as u8
    }

    pub const fn with_foreground(self, color: Pixel) -> Self {
        AChar((self.0 & !0xFF_00_00) | ((color as u32) << 16))
    }

    pub const fn with_background(self, color: Pixel) -> Self {
        AChar((self.0 & !0x00_FF_00) | ((color as u32) << 8))
    }

    pub const fn with_ascii_char(self, c: u8) -> Self {
        AChar((self.0 & !0x00_00_FF) | (c as u32))
    }
}

/// Raw text unpacking function. See `unpack` for something more pleasant.
///
/// Unpacks a row of attributed characters from `src` into the pixel buffer
/// `target`, using the font lookup table `font_slice`.
///
/// `font_slice` contains one byte per possible character, which will be used as
/// the leftmost eight pixels of the 10-pixel character cell.
///
/// # Panics
///
/// If `target` is not exactly `src.len() * GLYPH_COLS` bytes in length.
pub fn unpack_raw(src: &[AChar],
                  font_slice: &[u8; 256],
                  target: &mut [Pixel]) {
    assert_eq!(src.len() * GLYPH_COLS, target.len());
    unsafe {
        unpack_text_10p_attributed_impl(
            src.as_ptr(),
            font_slice.as_ptr(),
            target.as_mut_ptr(),
            src.len(),
        );
    }
}

/// Unpacks one scanline of an attributed character grid into a pixel buffer.
///
/// `src` is a slice of attributed characters, treated as consisting of rows of
/// `cols` characters each.
///
/// `font` is a bitmapped font consisting of 16 rows for each of 256 possible
/// characters.
///
/// `target` is a pixel buffer which must be at least `cols * GLYPH_COLS` bytes
/// in length.
///
/// `line_number` is the number of the current scanline, counting from the top
/// of the text display.
///
/// `cols` is the number of text, not pixel, columns in the display.
///
/// # Tips and Tricks
///
/// This interface is deceptively simple.
///
/// To implement a text display taking up only part of the screen -- perhaps
/// with another rasterizer handling the rest -- alter `line_number` by
/// subtracting the top line of the text region.
///
/// To implement smooth vertical scrolling through a larger-than-required `src`
/// slice, add the pixel offset to `line_number`.
///
/// To implement smooth *horizontal* scrolling,
///
/// 1. Set `cols` to one greater than you need. (Likely, in this case, `src`
///    contains only a single line of text.)
/// 2. Render as normal.
/// 3. Adjust `RenderCtx::target_range`: slide it to the right by up to 10
///    pixels to effect scrolling.
pub fn unpack(src: &[AChar],
              font: &[[u8; 256]; 16],
              target: &mut [Pixel],
              line_number: usize,
              cols: usize) {
    let text_row = line_number / GLYPH_ROWS;
    let glyph_row = line_number % GLYPH_ROWS;
    let pixel_width = cols * GLYPH_COLS;

    let offset = text_row * cols;
    let font_slice = &font[glyph_row];

    unpack_raw(
        &src[offset .. offset + cols],
        font_slice,
        &mut target[..pixel_width],
    )
}

extern {
    fn unpack_text_10p_attributed_impl(input_line: *const AChar,
                                       font: *const u8,
                                       target: *mut Pixel,
                                       cols_in_input: usize);
}
