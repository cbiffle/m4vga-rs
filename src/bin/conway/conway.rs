
/// This implementation operates on units of 32 bits.
type Unit = u32;

const BITS: usize = 32;

/// Result of a bit-parallel addition operation.
#[derive(Copy, Clone, Debug)]
struct AddResult {
    sum: Unit,
    carry: Unit,
}

/// Bit-parallel half-adder.
fn half_add(a: Unit, b: Unit) -> AddResult {
    AddResult {
        sum: a ^ b,
        carry: a & b,
    }
}

/// Bit-parallel full-adder.
fn full_add(a: Unit, b: Unit, c: Unit) -> AddResult {
    let r0 = half_add(a, b);
    let r1 = half_add(r0.sum, c);
    AddResult { sum: r1.sum, carry: r0.carry | r1.carry }
}

fn col_step(above: &[Unit; 3],
            current: &[Unit; 3],
            below: &[Unit; 3])
    -> Unit
{
    // Compute row-wise influence sums.  This produces 96 2-bit sums
    // (represented as three pairs of 32-vectors) giving the number of live
    // cells in the 1D Moore neighborhood around each position.
    let a_inf = full_add((above[1] << 1) | (above[0] >> (BITS - 1)),
                         above[1],
                         (above[1] >> 1) | (above[2] << (BITS - 1)));
    let c_inf = half_add((current[1] << 1) | (current[0] >> (BITS - 1)),
                         /* middle bits of current[1] don't count */
                         (current[1] >> 1) | (current[2] << (BITS - 1)));
    let b_inf = full_add((below[1] << 1) | (below[0] >> (BITS - 1)),
                         below[1],
                         (below[1] >> 1) | (below[2] << (BITS - 1)));

  // Sum the row-wise sums into a two-dimensional Moore neighborhood population
  // count.  Such a count can overflow into four bits, but we don't care: Conway
  // has the same result for 8/9 and 0/1 (the cell is cleared in both cases).
  //
  // Thus, we don't need a four-bit addition.  Instead, we just retain the
  // carry output from the two intermediate additions and use it as a mask.
  let next0 = full_add(a_inf.sum, c_inf.sum, b_inf.sum);
  let next1a = full_add(a_inf.carry, next0.carry, b_inf.carry);
  let next1b = half_add(c_inf.carry, next1a.sum);

  // Apply Niemiec's optimization: OR the current cell state vector into the
  // 9-cell neighborhoold population count to derive the new state cheaply.  The
  // cell is set iff its three-bit sum is 0b011.
  (next0.sum | current[1])
       & next1b.sum
       & !next1a.carry
       & !next1b.carry
}

// TODO: I'm fixing this at 800x600 for now to make the indexing operations
// cheaper. Revisit.
type Buffer = [Unit; 800 * 600 / BITS];
const WIDTH: usize = 800 / 32;
const HEIGHT: usize = 600;

/// Advance the automaton.
///  - current_map is the framebuffer (or equivalent bitmap) holding the current
///    state.
///  - next_map is a framebuffer (bitmap) that will be filled in.
pub fn step(current_map: &Buffer,
            next_map: &mut Buffer) {
    // We keep sliding windows of state in these arrays.
    let mut above   = [0; 3];
    let mut current = [0; 3];
    let mut below   = [0; 3];

    // Bootstrap for first column of first row.
    current[2] = current_map[0];
    below[2] = current_map[WIDTH];

    fn adv(name: &mut [Unit; 3], next: Unit) {
        name[0] = name[1];
        name[1] = name[2];
        name[2] = next
    }

    // First row, wherein above[x] = 0, less final column
    for x in 0 .. (WIDTH - 1) {
        adv(&mut current, current_map[x + 1]);
        adv(&mut below,   current_map[WIDTH + x + 1]);
        next_map[x] = col_step(&above, &current, &below);
    }

    // Final column of first row, wherein we cannot fetch next values.
    adv(&mut current, 0);
    adv(&mut below, 0);
    next_map[WIDTH - 1] = col_step(&above, &current, &below);

    // Remaining rows except the last.
    for y in 1 .. (HEIGHT - 1) {
        let offset = y * WIDTH;

        // Bootstrap row like we did for row 1.
        above[0] = 0;
        above[1] = 0;
        current[0] = 0;
        current[1] = 0;
        below[0] = 0;
        below[1] = 0;

        above[2] = current_map[offset - WIDTH];
        current[2] = current_map[offset];
        below[2] = current_map[offset + WIDTH];

        for x in 0 .. (WIDTH - 1) {
            adv(&mut above, current_map[offset - WIDTH + x + 1]);
            adv(&mut current, current_map[offset + x + 1]);
            adv(&mut below, current_map[offset + WIDTH + x + 1]);
            next_map[offset + x] = col_step(&above, &current, &below);
        }

        // Last column.
        adv(&mut above, 0);
        adv(&mut current, 0);
        adv(&mut below, 0);
        next_map[offset + WIDTH - 1] = col_step(&above, &current, &below);
    }

    // Final row, wherein below[x] = 0.
    let offset = WIDTH * (HEIGHT - 1);
    above[0] = 0;
    above[1] = 0;
    current[0] = 0;
    current[1] = 0;
    below = [0; 3];

    above[2] = current_map[offset - WIDTH];
    current[2] = current_map[offset];

    for x in 0 .. (WIDTH - 1) {
        adv(&mut above, current_map[offset - WIDTH + x + 1]);
        adv(&mut current, current_map[offset + x + 1]);
        next_map[offset + x] = col_step(&above, &current, &below);
    }

    // Final column
    adv(&mut above, 0);
    adv(&mut current, 0);
    next_map[offset + WIDTH - 1] = col_step(&above, &current, &below);
}
