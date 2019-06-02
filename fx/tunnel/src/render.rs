use super::table;
use super::{HALF_HEIGHT, HALF_WIDTH, WIDTH};

pub fn render(table: &table::Table, fb: &mut [u8], frame: usize) {
    const DSPEED: f32 = 1.;
    const ASPEED: f32 = 0.2;

    let frame = frame as f32;

    // Hey look, it's a rare case where I have to optimize bounds checking!
    // This routine originally operated upon a fixed-length array, ensuring that
    // bounds checking for predictable indices (like those generated in the loop
    // below) got compiled out. I switched it to a dynamic slice during the
    // portability sprint...and lost 30fps on the microcontroller.
    //
    // Why?
    //
    // Because I had asked it to be slower. Well, not in so few words, but: each
    // index into `fb` below is a bounds-check. The algorithm as written says
    // "get as much of this done as you can, until you panic at the end of fb."
    // That isn't useful, or what I intended, so the following line moves the
    // bounds check to the top of the loop. Back to 60fps.
    let fb = &mut fb[..super::BUFFER_WORDS * 4];

    // The distance we have traveled into the tunnel.
    let z = frame * DSPEED;
    // The angle of the tunnel's rotation.
    let a = frame * ASPEED;

    // Outer loops: iterate over each macroblock in the display, left-to-right,
    // top-to-bottom.  'y' and 'x' are in macroblock (table) coordinates.
    for y in 0..HALF_HEIGHT / table::SUB {
        // To process a macroblock, we need to look up the table entries at each
        // of its four corners.  When processing macroblocks left to right, the
        // right corners of a block are the left corners of its neighbor -- so
        // we can save table lookups by "shifting" the entries across.

        // Bootstrap the process by loading the leftmost two corners for this
        // row.
        let mut top_left = table[y][0];
        let mut bot_left = table[y + 1][0];

        for x in 0..HALF_WIDTH / table::SUB {
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
            let left_i = table::Entry {
                distance: (bot_left.distance - top_left.distance)
                    / table::SUB as f32,
                angle: (bot_left.angle - top_left.angle) / table::SUB as f32,
            };

            let mut right = top_right;
            let right_i = table::Entry {
                distance: (bot_right.distance - top_right.distance)
                    / table::SUB as f32,
                angle: (bot_right.angle - top_right.angle) / table::SUB as f32,
            };

            // Process pixel rows within the macroblock.  'sy' and 'sx' are in
            // pixel coordinates.
            for sy in y * table::SUB..(y + 1) * table::SUB {
                // We'll need this term repeatedly below; precompute it.
                let inv_sy = HALF_HEIGHT - 1 - sy;

                // Fire up the second dimension of the bilinear interpolator,
                // this time moving from the value of 'left' to the value of
                // 'right'.
                let mut v = left;
                let i = table::Entry {
                    distance: (right.distance - left.distance)
                        / table::SUB as f32,
                    angle: (right.angle - left.angle) / table::SUB as f32,
                };

                for sx in x * table::SUB..(x + 1) * table::SUB {
                    // Quadrant II (upper-left): apply trig identity to correct
                    // the angle value.
                    let a1 = -v.angle + table::TEX_PERIOD_A as f32 + a;
                    let p1 = color(v.distance, a1, v.distance + z);
                    fb[inv_sy * WIDTH + (WIDTH / 2 - 1 - sx)] = p1 as u8;

                    // Quadrant I (upper-right): use the angle value as written.
                    let a2 = v.angle + a;
                    let p2 = color(v.distance, a2, v.distance + z);
                    fb[inv_sy * WIDTH + sx + WIDTH / 2] = p2 as u8;

                    // Quadrants III/IV, of course, are handled through
                    // rasterization tricks, and not computed here.

                    // Advance the horizontal linear interpolator toward
                    // 'right'.
                    v = table::Entry {
                        distance: v.distance + i.distance,
                        angle: v.angle + i.angle,
                    };
                }

                // Advance the vertical linear interpolators toward 'bot_left'
                // and 'bot_right', respectively.
                left = table::Entry {
                    distance: left.distance + left_i.distance,
                    angle: left.angle + left_i.angle,
                };
                right = table::Entry {
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
        (pixel >> (0x01010000_u32 >> sel)) & (0x5555AAFF_u32 >> sel)
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
