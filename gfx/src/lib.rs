//! Bitband bitmap graphics algorithms.
//!
//! This module is deliberately architecture-independent to allow for testing on
//! the host.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

pub mod bit;

use core::mem::swap;

#[derive(Debug)]
pub(crate) enum Direction {
    Horizontal,
    Vertical,
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
pub(crate) unsafe fn draw_line_unclipped_unchecked(
    mut out: *mut u32,
    dx: usize,
    dy: usize,
    d: Direction,
    width_px: usize,
    x_adv: isize,
) {
    let (dmajor, dminor) = match d {
        Direction::Horizontal => (dx, dy),
        _ => (dy, dx),
    };

    let (minor_step, major_step) = match d {
        Direction::Horizontal => (width_px as isize, x_adv),
        _ => (x_adv, width_px as isize),
    };

    let dminor2 = (dminor * 2) as isize;
    let dmajor2 = (dmajor * 2) as isize;
    let mut error = dminor2 - dmajor as isize;

    #[cfg(test)]
    {
        eprintln!("major_step: {}", major_step);
        eprintln!("minor_step: {}", minor_step);
    }

    *out = 1;

    for _ in 0..dmajor {
        if error >= 0 {
            out = out.offset(minor_step as isize);
            error -= dmajor2;
        }
        error += dminor2;
        out = out.offset(major_step as isize);
        #[cfg(test)]
        eprintln!("{:p}", out);
        *out = 1;
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
pub(crate) fn draw_line_unclipped(
    mut x0: usize,
    mut y0: usize,
    mut x1: usize,
    mut y1: usize,
    buf: &mut [u32],
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
    assert!(
        compute_offset(x0, y0, stride) < buf.len()
            && compute_offset(x1, y1, stride) < buf.len()
    );


    let dx = x1 as isize - x0 as isize; // may be negative
    let dy = y1 - y0; // nonnegative

    let out = &mut buf[compute_offset(x0, y0, stride)];

    let (dx, x_adv) = if dx > 0 {
        (dx as usize, 1)
    } else {
        (-dx as usize, -1)
    };
    let dir = if dx > dy {
        Direction::Horizontal
    } else {
        Direction::Vertical
    };

    unsafe {
        draw_line_unclipped_unchecked(out, dx, dy, dir, stride, x_adv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_line(buf: &[u32], stride: usize, pred: fn(usize, usize) -> bool) {
        for (i, p) in buf.iter().enumerate() {
            let (x, y) = (i % stride, i / stride);
            if pred(x, y) {
                assert_eq!(*p, 1, "Pixel at ({}, {}) should be set", x, y);
            } else {
                assert_eq!(*p, 0, "Pixel at ({}, {}) should not be set", x, y);
            }
        }
    }

    #[test]
    fn zero_length() {
        let mut buf = [0; 1];
        // stride shouldn't be used, pass something big:
        draw_line_unclipped(0, 0, 0, 0, &mut buf, 100);
        // A zero-length line should still set one pixel.
        check_line(&buf, 100, |_, _| true);
    }

    #[test]
    fn horizontal_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [0; 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 99, 0, &mut buf, 100);
        check_line(&buf, 100, |_, y| y == 0);
    }

    #[test]
    fn vertical_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [0; 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 0, 99, &mut buf, 100);
        check_line(&buf, 100, |x, _| x == 0);
    }

    #[test]
    fn diagonal_full() {
        // Buffer on the stack to make corruption *slightly* more obvious.
        let mut buf = [0; 100 * 100];
        // Should not crash:
        draw_line_unclipped(0, 0, 99, 99, &mut buf, 100);
        check_line(&buf, 100, |x, y| x == y);
    }

    #[test]
    fn zero_length_out_x() {
        // The _unclipped function does not check that your use of coordinates
        // is sane, only that they are in-bounds.
        let mut buf = [0; 10*10];
        draw_line_unclipped(10, 0, 10, 0, &mut buf, 10);
    }

    #[test]
    #[should_panic]
    fn zero_length_out_y() {
        let mut buf = [0; 10*10];
        draw_line_unclipped(0, 10, 0, 10, &mut buf, 10);
    }
}
