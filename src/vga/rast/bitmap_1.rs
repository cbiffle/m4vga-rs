use core::sync::atomic::AtomicUsize;

use crate::vga::rast::Pixel;

extern {
    #[allow(improper_ctypes)]
    fn unpack_1bpp_impl(input_line: *const u32,
                        clut: *const AtomicUsize,
                        render_target: *mut Pixel,
                        words_in_input: usize);

    #[allow(improper_ctypes)]
    fn unpack_1bpp_overlay_impl(input_line: *const u32,
                                clut: *const AtomicUsize,
                                render_target: *mut Pixel,
                                words_in_input: usize,
                                background: *const u8);
}

/// Rasterize packed 1bpp pixel data using a color lookup table (CLUT).
///
/// `src` is a scanline of pixel data packed into `u32`s, where the least
/// significant bit of each `u32` is on the left.
///
/// `clut` is a reference to the CLUT, stored in the low two bytes of an
/// `AtomicUsize`. The least significant byte is the color used for 0 bits, and
/// the next byte for 1 bits. The top two bytes are unused.
///
/// `target` is the destination for unpacked raster output.
///
/// `src.len()` should be exactly `target.len() / 32`. Otherwise the results are
/// safe but undefined.
pub fn unpack(src: &[u32],
              clut: &AtomicUsize,
              target: &mut [u8]) {
    let words_in_input = core::cmp::min(
        src.len(),
        target.len() / 32,
    );
    unsafe {
        unpack_1bpp_impl(
            src.as_ptr(),
            clut,
            target.as_mut_ptr(),
            words_in_input,
        )
    }
}

/// Rasterize packed 1bpp pixel data using a foreground color and background
/// image. This overlays the results of unpacking (a la `unpack`) over the
/// full-color image scanline `background`, interpreting 0 bits in the input as
/// transparency.
pub fn unpack_overlay(src: &[u32],
              clut: &AtomicUsize,
              background: &[u8],
              target: &mut [u8]) {
    let words_in_input = core::cmp::min(
        core::cmp::min(src.len(), background.len()),
        target.len() / 32,
    );
    unsafe {
        unpack_1bpp_overlay_impl(
            src.as_ptr(),
            clut,
            target.as_mut_ptr(),
            words_in_input,
            background.as_ptr(),
        )
    }
}


