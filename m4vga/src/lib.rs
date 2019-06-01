#![no_std]

pub mod rast;
pub mod util;

pub mod priority;

/// Representation of a pixel in memory.
///
/// The driver consistently uses 8 bits per pixel. It is technically possible to
/// upgrade to 16, but performance is not great.
///
/// Moreover, many demos assume that only the bottom 6 bits are significant,
/// encoded as `0bBB_GG_RR`.
pub type Pixel = u8;

/// Maximum number of visible pixels in a scanline.
///
/// Timing limitations mean we can't really pull off modes above 800x600, so
/// we'll use this fact to size some data structures.
pub const MAX_PIXELS_PER_LINE: usize = 800;

cfg_if::cfg_if! {
    if #[cfg(target_os = "none")] {
        pub mod timing;

        // re-export driver bits
        mod driver;
        pub use driver::*;
    }
}
