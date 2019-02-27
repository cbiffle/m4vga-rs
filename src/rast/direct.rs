//! Direct-color rasterizer.

use crate::rast::{RasterCtx, TargetBuffer};

pub fn direct_color(
    line_number: usize,
    tgt: &mut TargetBuffer,
    ctx: &mut RasterCtx,
    buf: &[u32],
    stride: usize,
) {
    let offset = line_number * stride;
    crate::util::copy_words::copy_words(
        &buf[offset..offset + stride],
        &mut tgt.as_words_mut()[..stride],
    );
    ctx.target_range = 0..stride * 4;
}

pub fn direct_color_mirror(
    line_number: usize,
    tgt: &mut TargetBuffer,
    ctx: &mut RasterCtx,
    buf: &[u32],
    stride: usize,
    height: usize,
) {
    let line_number = height - line_number - 1;
    let offset = line_number * stride;
    let tgt = tgt.as_words_mut()[..stride].iter_mut();
    let src_rev = buf[offset..offset + stride].iter().rev();
    for (dst, src) in tgt.zip(src_rev) {
        *dst = src.swap_bytes()
    }
    ctx.target_range = 0..stride * 4;
}
