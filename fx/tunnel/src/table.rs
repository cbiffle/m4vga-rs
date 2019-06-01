//! Lookup table support.

use core::f32::consts::PI;

#[cfg(not(feature = "std"))]
use libm::F32Ext;

pub const SUB: usize = 4;

pub const TAB_WIDTH: usize = 800 / 2 / 2 / SUB + 1;
pub const TAB_HEIGHT: usize = 608 / 2 / 2 / SUB + 1; // round up

const TEX_HEIGHT: usize = 64;
const TEX_WIDTH: usize = 64;

pub const TEX_REPEAT_D: usize = 32;
pub const TEX_REPEAT_A: usize = 4;

pub const TEX_PERIOD_D: usize = TEX_REPEAT_D * TEX_HEIGHT;
pub const TEX_PERIOD_A: usize = TEX_REPEAT_A * TEX_WIDTH;

#[derive(Copy, Clone, Debug)]
pub struct Entry {
    pub distance: f32,
    pub angle: f32,
}

impl Entry {
    pub const fn zero() -> Entry {
        Entry {
            distance: 0.,
            angle: 0.,
        }
    }
    fn compute(x: usize, y: usize) -> Entry {
        let x = x as f32 + 0.5;
        let y = y as f32 + 0.5;
        Entry {
            distance: TEX_PERIOD_D as f32 / f32::sqrt(x * x + y * y),
            angle: TEX_PERIOD_A as f32 * 0.5 * (f32::atan2(y, x) / PI + 1.),
        }
    }
}

pub type Table = [[Entry; TAB_WIDTH]; TAB_HEIGHT];

pub fn compute(table: &mut Table) {
    for y in 0..TAB_HEIGHT {
        for x in 0..TAB_WIDTH {
            table[y][x] = Entry::compute(x * SUB, y * SUB)
        }
    }
}
