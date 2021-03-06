//! Bitband bitmap graphics algorithms.
//!
//! This module is deliberately architecture-independent to allow for testing on
//! the host.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

pub mod bit;

use bit::BandBit;
use core::mem::swap;

/// A 1-bit-per-pixel framebuffer represented with 32 bits packed into each
/// word.
#[derive(Debug)]
pub struct PackedBitBuffer<'a> {
    mem: &'a mut [u32],
    stride: usize,
}

/// A 1-bit-per-pixel framebuffer aliased in the bit-band region for efficient
/// access.
#[derive(Debug)]
pub struct BitBuffer<'a> {
    mem: &'a mut [BandBit],
    stride: usize,
}

impl<'a> PackedBitBuffer<'a> {
    pub fn new(mem: &'a mut [u32], stride: usize) -> Self {
        PackedBitBuffer { mem, stride }
    }

    pub fn as_word_slice(&mut self) -> &[u32] {
        self.mem
    }

    /// Borrows a packed buffer as its bit-band alias.
    ///
    /// # Panics
    ///
    /// If the buffer is not in the bit-band target region.
    pub fn as_bits(&mut self) -> BitBuffer {
        BitBuffer {
            mem: bit::as_bits_mut(self.mem),
            stride: self.stride * 32,
        }
    }

    /// Clears the contents of the buffer to zeroes.
    pub fn clear(&mut self) {
        for word in self.mem.iter_mut() {
            *word = 0
        }
    }
}

impl<'a> BitBuffer<'a> {
    pub fn draw_line_unclipped(
        &mut self,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) {
        draw_line_unclipped(x0, y0, x1, y1, self.mem, self.stride)
    }
}

trait YDirection {
    fn dmajor_dminor(dx: usize, dy: usize) -> (usize, usize);
    fn major_minor_step(stride: isize, x_adv: isize) -> (isize, isize);
}

enum Horizontal {}
enum Vertical {}

impl YDirection for Horizontal {
    fn dmajor_dminor(dx: usize, dy: usize) -> (usize, usize) {
        (dx, dy)
    }
    fn major_minor_step(stride: isize, x_adv: isize) -> (isize, isize) {
        (x_adv, stride)
    }
}

impl YDirection for Vertical {
    fn dmajor_dminor(dx: usize, dy: usize) -> (usize, usize) {
        (dy, dx)
    }
    fn major_minor_step(stride: isize, x_adv: isize) -> (isize, isize) {
        (stride, x_adv)
    }
}

trait XDirection {
    const X_ADV: isize;
}

enum Left {}
enum Right {}

impl XDirection for Left {
    const X_ADV: isize = -1;
}

impl XDirection for Right {
    const X_ADV: isize = 1;
}

/// Draws a line starting at bitband word `out` and progressing for `dx` and
/// `dy` pixels along either axis. Drawing along Y is always top to bottom; the
/// direction along X is determined by `x_adj`.
///
/// For a friendlier interface to this algorithm, see `draw_line_unclipped`.
///
/// # Safety
///
/// `out` will be offset by `dx*x_adj` and by `dy*width_px` to find the last
/// pixel in the line. All addresses along the line between the two points must
/// be in the bounds of the mutable buffer we're writing into.
unsafe fn draw_line_unclipped_unchecked<X: XDirection, Y: YDirection>(
    mut out: *mut BandBit,
    dx: usize,
    dy: usize,
    width_px: usize,
) {
    let (dmajor, dminor) = Y::dmajor_dminor(dx, dy);
    let (major_step, minor_step) =
        Y::major_minor_step(width_px as isize, X::X_ADV);

    let dminor2 = (dminor * 2) as isize;
    let dmajor2 = (dmajor * 2) as isize;
    let mut error = dminor2 - dmajor as isize;

    *out = BandBit::from(true);

    for _ in 0..dmajor {
        if error >= 0 {
            out = out.offset(minor_step as isize);
            error -= dmajor2;
        }
        error += dminor2;
        out = out.offset(major_step as isize);
        *out = BandBit::from(true);
    }
}

/// Draws a line from `(x0, y0)` to `(x1, y1)` by setting pixels, without
/// clipping.
///
/// `buf` is assumed to be a buffer within the bitband region, containing rows
/// of `stride` words. (It can also be in RAM, but we're going to write 1 to
/// each word as though it were in the bitband region, so make sure that result
/// is useful to you before reappropriating this.)
///
/// # Panics
///
/// If either coordinate falls outside the buffer.
pub fn draw_line_unclipped(
    mut x0: usize,
    mut y0: usize,
    mut x1: usize,
    mut y1: usize,
    buf: &mut [BandBit],
    stride: usize,
) {
    // Flip things as necessary to ensure that we draw horizontal or
    // top-to-bottom.
    if y0 > y1 {
        swap(&mut y0, &mut y1);
        swap(&mut x0, &mut x1);
    }

    // Bounds-check both ends of the rectangle we're drawing across.
    fn compute_offset(x: usize, y: usize, stride: usize) -> usize {
        // Force overflow checking.
        y.checked_mul(stride)
            .and_then(|row| row.checked_add(x))
            .unwrap()
    }

    let start_offset = compute_offset(x0, y0, stride);
    assert!(
        start_offset < buf.len() && compute_offset(x1, y1, stride) < buf.len()
    );

    let dx = x1 as isize - x0 as isize; // may be negative
    let dy = y1 - y0; // nonnegative

    let out = &mut buf[start_offset];

    if dx > 0 {
        let dx = dx as usize;
        if dx > dy {
            unsafe {
                draw_line_unclipped_unchecked::<Right, Horizontal>(
                    out, dx, dy, stride,
                )
            }
        } else {
            unsafe {
                draw_line_unclipped_unchecked::<Right, Vertical>(
                    out, dx, dy, stride,
                )
            }
        }
    } else {
        let dx = -dx as usize;
        if dx as usize > dy {
            unsafe {
                draw_line_unclipped_unchecked::<Left, Horizontal>(
                    out, dx, dy, stride,
                )
            }
        } else {
            unsafe {
                draw_line_unclipped_unchecked::<Left, Vertical>(
                    out, dx, dy, stride,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_line(
        buf: &[BandBit],
        stride: usize,
        pred: fn(usize, usize) -> bool,
    ) {
        for (i, p) in buf.iter().enumerate() {
            let (x, y) = (i % stride, i / stride);
            if pred(x, y) {
                assert_eq!(
                    bool::from(*p),
                    true,
                    "Pixel at ({}, {}) should be set",
                    x,
                    y
                );
            } else {
                assert_eq!(
                    bool::from(*p),
                    false,
                    "Pixel at ({}, {}) should not be set",
                    x,
                    y
                );
            }
        }
    }

    #[test]
    fn zero_length() {
        let mut buf = [BandBit::from(false); 1];
        // stride shouldn't be used, pass something big:
        draw_line_unclipped(0, 0, 0, 0, &mut buf, 100);
        // A zero-length line should still set one pixel.
        check_line(&buf, 100, |_, _| true);
    }

    #[test]
    fn horizontal_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [BandBit::from(false); 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 99, 0, &mut buf, 100);
        check_line(&buf, 100, |_, y| y == 0);
    }

    #[test]
    fn vertical_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [BandBit::from(false); 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 0, 99, &mut buf, 100);
        check_line(&buf, 100, |x, _| x == 0);
    }

    #[test]
    fn diagonal_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [BandBit::from(false); 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 99, 99, &mut buf, 100);
        check_line(&buf, 100, |x, y| x == y);
    }

    #[test]
    fn zero_length_out_x() {
        // The _unclipped function does not check that your use of coordinates
        // is sane, only that they are in-bounds.
        let mut buf = [BandBit::from(false); 10 * 10];
        draw_line_unclipped(10, 0, 10, 0, &mut buf, 10);
    }

    #[test]
    #[should_panic]
    fn zero_length_out_y() {
        let mut buf = [BandBit::from(false); 10 * 10];
        draw_line_unclipped(0, 10, 0, 10, &mut buf, 10);
    }
}
