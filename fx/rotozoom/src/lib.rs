#![cfg_attr(target_os = "none", no_std)]

use core::borrow::Borrow;

use m4vga::util::race_buf::{RaceReader,RaceWriter,RaceBuffer};
use m4vga::rast::direct;
use m4vga_fx_common::{Demo, Raster, Render};

use math::{Augment, Mat3f, Matrix, Project, Vec2};

#[cfg(target_os = "none")]
use libm::F32Ext;

pub const NATIVE_WIDTH: usize = 800;
pub const NATIVE_HEIGHT: usize = 600;

const X_SCALE: usize = 2;
const Y_SCALE: usize = 2;
const WIDTH: usize = 800 / X_SCALE;
const HEIGHT: usize = 600 / Y_SCALE;
pub const HALF_HEIGHT: usize = HEIGHT / 2;

const COLS: f32 = WIDTH as f32;
const ROWS: f32 = HEIGHT as f32;

pub const BUFFER_STRIDE: usize = WIDTH / 4;

const ROT: f32 = 0.005;

pub type Row = [u32; BUFFER_STRIDE];

pub struct State<S> {
    pub race_buf: RaceBuffer<S, Row>,
    pub m: Mat3f,
    pub rot: Mat3f,
}

impl<S> State<S> {
    pub fn new(segments: [S; 2]) -> Self {
        State {
            race_buf: RaceBuffer::new(segments),
            m: Mat3f::identity(),
            rot: Mat3f::rotate(ROT),
        }
    }
}

pub struct RasterState<'a, S> {
    reader: RaceReader<'a, S, Row>,
}

pub struct RenderState<'a, S> {
    writer: RaceWriter<'a, S, Row>,
    m: &'a mut Mat3f,
    rot: &'a Mat3f,
}

impl<'a, S: 'a> Demo<'a> for State<S>
where S: Borrow<[Row]> + AsMut<[Row]>,
{
    type Raster = RasterState<'a, S>;
    type Render = RenderState<'a, S>;

    fn split(&'a mut self) -> (Self::Raster, Self::Render) {
        let (reader, writer) = self.race_buf.split();
        (
            RasterState {
                reader,
            },
            RenderState {
                writer,
                m: &mut self.m,
                rot: &self.rot,
            },
        )
    }
}

impl<'a, S> Raster for RasterState<'a, S>
where S: Borrow<[Row]>,
{
    fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        p: m4vga::priority::I0,
    ) {
        let buf = self.reader.take_line(ln / Y_SCALE, &p);
        ctx.cycles_per_pixel *= X_SCALE;
        ctx.repeat_lines = Y_SCALE - 1;
        direct::direct_color(0, target, ctx, buf, BUFFER_STRIDE);
    }
}

impl<'a, S> Render for RenderState<'a, S>
where S: Borrow<[Row]> + AsMut<[Row]>,
{
    fn render_frame(&mut self, frame: usize, thread: m4vga::priority::Thread) {
        self.writer.reset(&thread);

        m4vga::util::measurement::sig_d_set();
        let s = (frame as f32 / 50.).sin() * 0.7 + 1.;
        let tx = (frame as f32 / 100.).cos() * 100.;
        let ty = 0.;

        let m_ = *self.m * Mat3f::translate(tx, ty) * Mat3f::scale(s, s);

        let top_left =
            (m_ * Vec2(-COLS / 2., -ROWS / 2.).augment()).project();
        let top_right =
            (m_ * Vec2(COLS / 2., -ROWS / 2.).augment()).project();
        let bot_left =
            (m_ * Vec2(-COLS / 2., ROWS / 2.).augment()).project();

        let xi = (top_right - top_left) * (1. / COLS);
        let yi = (bot_left - top_left) * (1. / ROWS);
        let mut ybase = top_left;
        for _ in 0..HEIGHT {
            let mut buf = self.writer.generate_line(&thread);
            let buf = u32_as_u8_mut(&mut *buf);
            {
                let mut pos = ybase;
                for x in 0..WIDTH {
                    buf[x] = tex_fetch(pos.0, pos.1) as u8;
                    pos = pos + xi;
                }
            }
            ybase = ybase + yi;
        }

        *self.m = *self.m * *self.rot;

        m4vga::util::measurement::sig_d_clear();
    }
}

fn u32_as_u8_mut(slice: &mut [u32]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            slice.len() * 4,
        )
    }
}

fn tex_fetch(u: f32, v: f32) -> u32 {
    u as i32 as u32 ^ v as i32 as u32
}


