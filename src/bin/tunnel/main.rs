//! Traditional "zooming down a tunnel" effect.

#![no_std]
#![no_main]

#[cfg(feature = "panic-itm")]
extern crate panic_itm;
#[cfg(feature = "panic-halt")]
extern crate panic_halt;

mod table;

use stm32f4;
use stm32f4::stm32f407::interrupt;
use m4vga::util::spin_lock::SpinLock;
use m4vga::rast::direct;
use table::Entry;

const NATIVE_WIDTH: usize = 800;
const NATIVE_HEIGHT: usize = 600;
const SCALE: usize = 2;

const WIDTH: usize = NATIVE_WIDTH / SCALE;
const HEIGHT: usize = NATIVE_HEIGHT / SCALE;
const HALF_WIDTH: usize = WIDTH / 2;
const HALF_HEIGHT: usize = HEIGHT / 2;

const BUFFER_SIZE: usize = WIDTH * HALF_HEIGHT;
const BUFFER_WORDS: usize = BUFFER_SIZE / 4;
const BUFFER_STRIDE: usize = WIDTH / 4;

static mut BUF0: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
#[link_section = ".local_ram"]
static mut BUF1: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];
#[no_mangle]
static mut TABLE: table::Table =
    [[Entry::zero(); table::TAB_WIDTH]; table::TAB_HEIGHT];


/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    let table = unsafe { &mut TABLE };
    table::compute(table);
    let table = &table;

    let fg = SpinLock::new(unsafe { &mut BUF0 });
    let mut bg = unsafe { &mut BUF1 };

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            |ln, tgt, ctx| {
                if ln < 4 || ln > 595 {
                    m4vga::rast::solid_color_fill(
                        tgt,
                        ctx,
                        800,
                        0,
                    );
                    return
                }
                let buf = fg.try_lock().expect("rast fg access");

                let ln = ln / SCALE;
                if ln < HALF_HEIGHT {
                    direct::direct_color(
                        ln,
                        tgt,
                        ctx,
                        *buf,
                        BUFFER_STRIDE,
                    );
                } else {
                    direct::direct_color_mirror(
                        ln,
                        tgt,
                        ctx,
                        *buf,
                        BUFFER_STRIDE,
                        HEIGHT,
                    );
                };
                ctx.cycles_per_pixel *= SCALE;
                ctx.repeat_lines = SCALE - 1;
            },
            // This closure contains the main loop of the program.
            |vga| {
                vga.video_on();
                let mut frame = 0;
                loop {
                    vga.sync_to_vblank();
                    core::mem::swap(&mut bg,
                                    &mut *fg.try_lock().expect("swap access"));
                    let bg = u32_as_u8_mut(bg);
                    m4vga::measurement::sig_d_set();
                    render(table, bg, frame);
                    m4vga::measurement::sig_d_clear();
                    frame = (frame + 1) % 65536; // prevent windup
                }
            })
}

fn render(table: &table::Table,
          fb: &mut [u8],
          frame: usize) {
    const DSPEED: f32 = 1.;
    const ASPEED: f32 = 0.2;

    let frame = frame as f32;

    // The distance we have traveled into the tunnel.
    let z = frame * DSPEED;
    // The angle of the tunnel's rotation.
    let a = frame * ASPEED;

    // Outer loops: iterate over each macroblock in the display, left-to-right,
    // top-to-bottom.  'y' and 'x' are in macroblock (table) coordinates.
    for y in 0 .. HALF_HEIGHT / table::SUB {
        // To process a macroblock, we need to look up the table entries at each
        // of its four corners.  When processing macroblocks left to right, the
        // right corners of a block are the left corners of its neighbor -- so
        // we can save table lookups by "shifting" the entries across.

        // Bootstrap the process by loading the leftmost two corners for this
        // row.
        let mut top_left = table[y][0];
        let mut bot_left = table[y + 1][0];

        for x in 0 .. HALF_WIDTH / table::SUB {
            // Load the two corners at the right side of the current block.
            let top_right = table[y][x + 1];
            let bot_right = table[y + 1][x + 1];

            // And now we fire up a stepwise bilinear interpolator in both
            // distance and angle.  To interpolate the table entry for a pixel
            // in the macroblock, we first linearly interpolate the values along
            // the left and right edges at its Y coordinate, and then
            // interpolate between them at its X coordinate.
            //
            // We do this stepwise by calculating the linear equation of both
            // distance and angle on both the left and right sides, given as a
            // value and a slope, or increment: (left, left_i) and (right,
            // right_i).  We'll update the position in-place, but the slopes are
            // constant.
            let mut left = top_left;
            let left_i = Entry {
                distance: (bot_left.distance - top_left.distance)
                    / table::SUB as f32,
                angle: (bot_left.angle - top_left.angle) / table::SUB as f32,
            };

            let mut right = top_right;
            let right_i = Entry {
                distance: (bot_right.distance - top_right.distance)
                    / table::SUB as f32,
                angle: (bot_right.angle - top_right.angle) / table::SUB as f32,
            };

            // Process pixel rows within the macroblock.  'sy' and 'sx' are in
            // pixel coordinates.
            for sy in y * table::SUB .. (y + 1) * table::SUB {
                // We'll need this term repeatedly below; precompute it.
                let inv_sy = HALF_HEIGHT - 1 - sy;

                // Fire up the second dimension of the bilinear interpolator,
                // this time moving from the value of 'left' to the value of
                // 'right'.
                let mut v = left;
                let i = Entry {
                    distance: (right.distance - left.distance)
                        / table::SUB as f32,
                    angle: (right.angle - left.angle) / table::SUB as f32,
                };

                for sx in x * table::SUB .. (x + 1) * table::SUB {
                    // Quadrant II (upper-left): apply trig identity to correct
                    // the angle value.
                    let a1 = -v.angle + table::TEX_PERIOD_A as f32 + a;
                    let p1 = color(v.distance, a1, v.distance + z);
                    fb[inv_sy * WIDTH + (WIDTH/2 - 1 - sx)] = p1 as u8;

                    // Quadrant I (upper-right): use the angle value as written.
                    let a2 = v.angle + a;
                    let p2 = color(v.distance, a2, v.distance + z);
                    fb[inv_sy * WIDTH + sx + WIDTH/2] = p2 as u8;

                    // Quadrants III/IV, of course, are handled through
                    // rasterization tricks, and not computed here.

                    // Advance the horizontal linear interpolator toward
                    // 'right'.
                    v = Entry {
                        distance: v.distance + i.distance,
                        angle: v.angle + i.angle,
                    };
                }

                // Advance the vertical linear interpolators toward 'bot_left'
                // and 'bot_right', respectively.
                left = Entry {
                    distance: left.distance + left_i.distance,
                    angle: left.angle + left_i.angle,
                };
                right = Entry {
                    distance: right.distance + right_i.distance,
                    angle: right.angle + right_i.angle,
                };
            }

            // Shift the right corners to become the new left corners.
            top_left = top_right;
            bot_left = bot_right;
        }
    }
}

#[cfg(not(feature = "no-shading"))]
fn color(distance: f32, fd: f32, fa: f32) -> u32 {
    shade(distance, tex_fetch(fd, fa))
}

#[cfg(feature = "no-shading")]
fn color(distance: f32, fd: f32, fa: f32) -> u32 {
    tex_fetch(fd, fa)
}

fn shade(distance: f32, pixel: u32) -> u32 {
    let sel = (distance / (table::TEX_REPEAT_D * 2) as f32) as u32;
    if sel < 4 {
        // sel is 0..4
        let sel = sel * 8; // sel is 0..32, shifts should not be UB
        (pixel >> (0x01010000_u32 >> sel))
            & (0x5555AAFF_u32 >> sel)
    } else {
        0
    }
}

#[cfg(not(feature = "alt-texture"))]
fn tex_fetch(x: f32, y: f32) -> u32 {
    x as u32 ^ y as u32
}

#[cfg(feature = "alt-texture")]
fn tex_fetch(x: f32, y: f32) -> u32 {
    (x * y).to_bits()
}

fn u32_as_u8_mut(slice: &mut [u32]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            slice.as_mut_ptr() as *mut u8,
            slice.len() * 4,
        )
    }
}

/// Wires up the PendSV handler expected by the driver.
#[cortex_m_rt::exception]
#[link_section = ".ramcode"]
fn PendSV() {
    m4vga::pendsv_raster_isr()
}

/// Wires up the TIM3 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM3() {
    m4vga::tim3_shock_isr()
}

/// Wires up the TIM4 handler expected by the driver.
#[interrupt]
#[link_section = ".ramcode"]
fn TIM4() {
    m4vga::tim4_horiz_isr()
}
