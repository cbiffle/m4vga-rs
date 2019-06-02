mod utils;

use m4vga::util::spin_lock::SpinLock;
use wasm_bindgen::prelude::*;

use m4vga_fx_common::{Demo, Raster, Render};
use m4vga_fx_conway as conway;
use m4vga_fx_rotozoom as roto;
use m4vga_fx_tunnel as tunnel;

const FIXED_WIDTH: usize = 800;
const FIXED_HEIGHT: usize = 600;

const RED_X4: u32 = 0x03_03_03_03;
const BLUE_X4: u32 = 0x30_30_30_30;
const GREEN32: u32 = 0xFF_00_FF_00;

#[wasm_bindgen]
pub fn width() -> usize {
    FIXED_WIDTH
}

#[wasm_bindgen]
pub fn height() -> usize {
    FIXED_HEIGHT
}

pub struct Sim<S> {
    state: S,

    target: Vec<u32>,
    framebuffer: Vec<u32>,
    frame: usize,
}

impl<S> From<S> for Sim<S> {
    fn from(state: S) -> Self {
        Self {
            state,
            target: vec![BLUE_X4; m4vga::rast::TARGET_BUFFER_SIZE / 4],
            framebuffer: vec![GREEN32; FIXED_WIDTH * FIXED_HEIGHT],
            frame: 0,
        }
    }
}

impl<S> Sim<S> {
    pub fn framebuffer(&self) -> *const u32 {
        self.framebuffer.as_ptr()
    }
}

impl<'a, S> Sim<S>
where
    S: Demo<'a>,
{
    pub fn step(&'a mut self) {
        // Safety: wasm is not a concurrent environment right now, so preemption
        // is not an issue.
        let i_priority = unsafe { m4vga::priority::I0::new() };
        let t_priority = m4vga::priority::Thread::new_checked().unwrap();

        let (mut raster, mut render) = self.state.split();

        render.render_frame(self.frame, t_priority);
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
            self.framebuffer.chunks_mut(FIXED_WIDTH).enumerate()
        {
            if ctx.repeat_lines > 0 {
                ctx.repeat_lines -= 1;
            } else {
                ctx = m4vga::rast::RasterCtx {
                    cycles_per_pixel: 4,
                    repeat_lines: 0,
                    target_range: 0..0,
                };
                raster.raster_callback(ln, target, &mut ctx, i_priority);
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
        }
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

////////////////////////////////////////////////////////////////////////////////

#[wasm_bindgen]
pub struct Tunnel(Sim<tunnel::State<Vec<u32>, Box<tunnel::table::Table>>>);

#[wasm_bindgen]
impl Tunnel {
    pub fn new() -> Self {
        // Good a place as any...
        self::utils::set_panic_hook();

        let mut table = Box::new(
            [[tunnel::table::Entry::zero(); tunnel::table::TAB_WIDTH];
                tunnel::table::TAB_HEIGHT],
        );
        tunnel::table::compute(&mut table);

        Tunnel(
            tunnel::State {
                fg: SpinLock::new(vec![RED_X4; tunnel::BUFFER_WORDS]),
                bg: vec![RED_X4; tunnel::BUFFER_WORDS],
                table,
            }
            .into(),
        )
    }

    pub fn framebuffer(&self) -> *const u32 {
        self.0.framebuffer()
    }

    pub fn step(&mut self) {
        self.0.step()
    }
}

////////////////////////////////////////////////////////////////////////////////

#[wasm_bindgen]
pub struct Rotozoom(Sim<roto::State<Vec<roto::Row>>>);

#[wasm_bindgen]
impl Rotozoom {
    pub fn new() -> Self {
        // Good a place as any...
        self::utils::set_panic_hook();

        let mut table = Box::new(
            [[tunnel::table::Entry::zero(); tunnel::table::TAB_WIDTH];
                tunnel::table::TAB_HEIGHT],
        );
        tunnel::table::compute(&mut table);

        Rotozoom(
            roto::State::new([
                vec![[0; roto::BUFFER_STRIDE]; roto::HALF_HEIGHT],
                vec![[0; roto::BUFFER_STRIDE]; roto::HALF_HEIGHT],
            ])
            .into(),
        )
    }

    pub fn framebuffer(&self) -> *const u32 {
        self.0.framebuffer()
    }

    pub fn step(&mut self) {
        self.0.step()
    }
}

////////////////////////////////////////////////////////////////////////////////

#[wasm_bindgen]
pub struct Conway(Sim<conway::State<Vec<u32>>>);

#[wasm_bindgen]
impl Conway {
    pub fn new() -> Self {
        // Good a place as any...
        self::utils::set_panic_hook();

        Conway(Sim::from(conway::State::new(
            vec![0; 800 * 600 / 32],
            vec![0; 800 * 600 / 32],
            0b11_11_11,
            0b00_00_00,
        )))
    }

    pub fn framebuffer(&self) -> *const u32 {
        self.0.framebuffer()
    }

    pub fn step(&mut self) {
        self.0.step()
    }
}
