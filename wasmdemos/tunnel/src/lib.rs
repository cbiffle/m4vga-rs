mod utils;

use wasm_bindgen::prelude::*;
use m4vga_fx_tunnel as fx;
use m4vga::util::spin_lock::SpinLock;

#[wasm_bindgen]
pub struct Demo {
    fg: SpinLock<Vec<u32>>,
    bg: Vec<u32>,

    target: Vec<u32>,

    framebuffer: Vec<u32>,

    table: fx::table::Table,
    frame: usize,
}

const RED_X4: u32 = 0x03_03_03_03;
const BLUE_X4: u32 = 0x30_30_30_30;
const GREEN32: u32 = 0xFF_00_FF_00;

#[wasm_bindgen]
impl Demo {
    pub fn new() -> Self {
        // Good a place as any...
        self::utils::set_panic_hook();

        let mut table = [[fx::table::Entry::zero(); fx::table::TAB_WIDTH];
            fx::table::TAB_HEIGHT];
        fx::table::compute(&mut table);

        Demo {
            fg: SpinLock::new(vec![RED_X4; fx::BUFFER_WORDS]),
            bg: vec![RED_X4; fx::BUFFER_WORDS],

            target: vec![BLUE_X4; m4vga::rast::TARGET_BUFFER_SIZE / 4],

            framebuffer: vec![GREEN32; fx::NATIVE_WIDTH * fx::NATIVE_HEIGHT],
            table,
            frame: 0,
        }
    }

    pub fn framebuffer(&self) -> *const u32 {
        self.framebuffer.as_ptr()
    }

    pub fn width(&self) -> u32 {
        fx::NATIVE_WIDTH as u32
    }

    pub fn height(&self) -> u32 {
        fx::NATIVE_HEIGHT as u32
    }

    pub fn step(&mut self) {
        fx::render_frame(&mut self.bg, &self.fg, &self.table, self.frame);

        self.frame = (self.frame + 1) % 65536;

        let mut ctx = m4vga::rast::RasterCtx {
            cycles_per_pixel: 4,
            repeat_lines: 0,
            target_range: 0..0,
        };

        let target = m4vga::rast::TargetBuffer::from_array_mut(
            arrayref::array_mut_ref!(
                self.target.as_mut_slice(),
                0,
                m4vga::rast::TARGET_BUFFER_SIZE / 4
            ),
        );
        for (ln, target32) in
            self.framebuffer.chunks_mut(fx::NATIVE_WIDTH).enumerate()
        {
            if ctx.repeat_lines > 0 {
                ctx.repeat_lines -= 1;
            } else {
                ctx = m4vga::rast::RasterCtx {
                    cycles_per_pixel: 4,
                    repeat_lines: 0,
                    target_range: 0..0,
                };
                fx::raster_callback(ln, target, &mut ctx, &self.fg);
            }
            secondary_unpack(&ctx, target.as_words(), target32);
        }
    }
}

fn secondary_unpack(
    ctx: &m4vga::rast::RasterCtx,
    src: &[u32],
    dest: &mut [u32],
) {
    match ctx.cycles_per_pixel {
        // Full resolution.
        4 => {
            for (dest4, &src) in
                dest[ctx.target_range.clone()].chunks_mut(4).zip(src)
            {
                dest4[0] = unpack_color8(src as u8);
                dest4[1] = unpack_color8((src >> 8) as u8);
                dest4[2] = unpack_color8((src >> 16) as u8);
                dest4[3] = unpack_color8((src >> 24) as u8);
            }
        }
        // Horizontal pixel doubling.
        8 => {
            for (dest8, &src) in dest.chunks_mut(8).zip(src) {
                dest8[0] = unpack_color8(src as u8);
                dest8[1] = unpack_color8(src as u8);
                dest8[2] = unpack_color8((src >> 8) as u8);
                dest8[3] = unpack_color8((src >> 8) as u8);
                dest8[4] = unpack_color8((src >> 16) as u8);
                dest8[5] = unpack_color8((src >> 16) as u8);
                dest8[6] = unpack_color8((src >> 24) as u8);
                dest8[7] = unpack_color8((src >> 24) as u8);
            }
        }
        // Solid color fill.
        3200 => {
            assert_eq!(ctx.target_range, 0..1);
            let val = unpack_color8(src[0] as u8);
            for pixel in dest {
                *pixel = val;
            }
        },
        x => panic!("unsupported cycles_per_pixel: {}", x),
    }
}

fn unpack_color8(src: u8) -> u32 {
    // HACK: we're little-endian, so we want ABGR
    let r = (src as u32 & 0b11) << 6;
    let g = (src as u32 & 0b11_00) << (4 + 8);
    let b = (src as u32 & 0b11_00_00) << (2 + 16);
    0xFF_00_00_00 | r | g | b
}
