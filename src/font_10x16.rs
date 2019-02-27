//! A bitmapped ASCII font with 10x16 pixel characters.

#[derive(Clone)]
pub struct Font([u8; 4096]);

impl Font {
    /// View the font as an array of 16 glyph slices.
    pub fn as_glyph_slices(&self) -> &[[u8; 256]; 16] {
        // Safety: this is how the font is laid out internally. We'd represent
        // it that way in memory, too, except then `include_bytes!` wouldn't
        // work.
        unsafe { core::mem::transmute(&self.0) }
    }
}

/// Static image of the 10x16 font.
#[cfg_attr(feature = "ram-font", link_section = ".data")]
pub static FONT: Font = Font(*include_bytes!("font_10x16.bin"));
