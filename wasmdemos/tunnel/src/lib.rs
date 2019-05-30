mod utils;

use wasm_bindgen::prelude::*;
use m4vga_fx_tunnel as fx;

#[wasm_bindgen]
pub struct Demo {
    framebuffer8: Vec<u8>,
    framebuffer32: Vec<u32>,
    table: fx::table::Table,
    frame: usize,
}

#[wasm_bindgen]
impl Demo {
    pub fn new() -> Self {
        // Good a place as any...
        self::utils::set_panic_hook();

        let mut table = [[fx::table::Entry::zero(); fx::table::TAB_WIDTH];
            fx::table::TAB_HEIGHT];
        fx::table::compute(&mut table);

        Demo {
            framebuffer8: vec![0b11_00_11; fx::WIDTH * fx::HEIGHT / 2],
            framebuffer32: vec![0xFF_00_FF_00; fx::WIDTH * fx::HEIGHT],
            table,
            frame: 0,
        }
    }

    pub fn framebuffer(&self) -> *const u32 {
        self.framebuffer32.as_ptr()
    }

    pub fn width(&self) -> u32 {
        fx::WIDTH as u32
    }

    pub fn height(&self) -> u32 {
        fx::HEIGHT as u32
    }

    pub fn step(&mut self) {
        fx::render::render(&self.table, &mut self.framebuffer8, self.frame);

        self.frame = (self.frame + 1) % 65536;

        // Top half of display.
        for (ln, target) in self.framebuffer32.chunks_mut(fx::WIDTH).enumerate()
        {
            if ln < (4 / 2) || ln > (595 / 2) {
                solid_color_fill(target, 400, 0);
                continue;
            }

            if ln < fx::HALF_HEIGHT {
                direct_color(ln, target, &self.framebuffer8, fx::BUFFER_STRIDE);
            } else {
                direct_color_mirror(
                    ln,
                    target,
                    &self.framebuffer8,
                    fx::BUFFER_STRIDE,
                    fx::HEIGHT,
                );
            }
        }
    }
}

fn solid_color_fill(target: &mut [u32], _width: usize, color8: u8) {
    let color = unpack_color8(color8);
    for t in target {
        *t = color
    }
}

fn direct_color(
    line_number: usize,
    target: &mut [u32],
    packed: &[u8],
    stride_words: usize,
) {
    let stride = stride_words * 4;
    let offset = line_number * stride;
    for (dest, src) in target.iter_mut().zip(&packed[offset..offset + stride]) {
        *dest = unpack_color8(*src);
    }
}

fn direct_color_mirror(
    line_number: usize,
    target: &mut [u32],
    packed: &[u8],
    stride_words: usize,
    height: usize,
) {
    let stride = stride_words * 4;
    let line_number = height - line_number - 1;
    let offset = line_number * stride;
    let source = packed[offset..offset + stride].iter().rev();
    for (dest, src) in target.iter_mut().zip(source) {
        *dest = unpack_color8(*src);
    }
}

fn unpack_color8(src: u8) -> u32 {
    // HACK: we're little-endian, so we want ABGR
    let r = (src as u32 & 0b11) << 6;
    let g = (src as u32 & 0b11_00) << (4 + 8);
    let b = (src as u32 & 0b11_00_00) << (2 + 16);
    0xFF_00_00_00 | r | g | b
}
