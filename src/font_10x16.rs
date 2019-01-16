/// Wrapper around font image to force word alignment.
#[repr(align(4))]
pub struct Font([u8; 4096]);

/// Static image of the 10x16 font.
pub static FONT: Font = Font(*include_bytes!("font_10x16.bin"));
