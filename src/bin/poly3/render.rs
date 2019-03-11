use core::mem;
use core::ops::Range;

use arrayvec::ArrayVec;

use m4vga::math::Vec3i;

pub const MAX_TRIS: usize = 16;
pub const MAX_STATES: usize = MAX_TRIS;

/// Description of a triangle relative to a vertex buffer.
#[derive(Copy, Clone, Debug)]
pub struct Tri {
    pub vertex_indices: [usize; 3],
    pub color: u8,
}

/// State machine for drawing a triangle.
///
/// The triangle is defined in screen-space by its left and right edges, top,
/// and height. In practice, this means it's actually a trapezoid, but in
/// practice the edges will intersect at the top or bottom.
///
/// ```text
/// top_y --->  -------------           \
///       left /              ` right   | height
///      edge /                 `  edge |
///          `--------------------`     /
/// ```
///
/// Either the top or bottom of the triangle is an axis-aligned edge. Real
/// triangles don't necessarily sit neatly on a scanline like this; a triangle
/// like the following requires *two* state machines to render:
///
/// ```text
///    |\
///    | \
///    |  \
///   - - - - split here
///    |  /
///    | /
///    |/
/// ```
#[derive(Copy, Clone, Debug, Default)]
pub struct TriState {
    /// Top scanline included in the triangle.
    pub top_y: usize,
    /// Last scanline included in this triangle. May equal `top_y`.
    pub last_y: usize,
    /// Line describing the triangle's left edge.
    left: Line,
    /// Line describing the triangle's right edge.
    right: Line,
    /// Color of triangle.
    color: u8,
}

impl TriState {
    pub fn evaluate(&self, scanline: usize) -> Range<usize> {
        let scanline = scanline as f32;
        self.left.evaluate(scanline)..self.right.evaluate(scanline)
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Line {
    slope: f32,
    intercept: f32,
}

impl Line {
    pub fn between(p0: &Vec3i, p1: &Vec3i) -> Line {
        let p0_0 = p0.0 as f32;
        let p0_1 = p0.1 as f32;
        let slope = (p1.0 as f32 - p0_0) / (p1.1 as f32 - p0_1);
        Line {
            slope,
            intercept: p0_0 - p0_1 * slope,
        }
    }

    pub fn evaluate(&self, x: f32) -> usize {
        (self.slope * x + self.intercept) as usize
    }
}

#[derive(Copy, Clone, Debug)]
struct StateIndex(u8);

impl StateIndex {
    pub fn checked(idx: usize) -> Option<StateIndex> {
        if idx < MAX_STATES {
            Some(StateIndex(idx as u8))
        } else {
            None
        }
    }

    pub fn index<T>(self, array: &[T; MAX_STATES]) -> &T {
        // Safety: StateIndex is guaranteed in range.
        unsafe { array.get_unchecked(self.0 as usize) }
    }

    pub fn index_mut<T>(self, array: &mut [T; MAX_STATES]) -> &mut T {
        // Safety: StateIndex is guaranteed in range.
        unsafe { array.get_unchecked_mut(self.0 as usize) }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Raster {
    /// A triangle state machine for each camera-facing triangle in this frame.
    ///
    /// Note: this always contains an entry for each potential state, but some
    /// may contain garbage. Only the entries indexed by the index arrays are
    /// guaranteed valid.
    tris: [TriState; MAX_STATES],
    /// Indices of pending triangles, sorted by descending Y.
    ///
    /// To find starting triangles, inspect `last` and `pop` it while it refers
    /// to triangles that start on this scanline.
    ///
    /// Invariant: each index in this vector must be unique.
    pending: ArrayVec<[StateIndex; MAX_STATES]>,
    /// Indices of active triangles in no particular order.
    ///
    /// Invariant: each index in this vector must be unique.
    active: ArrayVec<[StateIndex; MAX_STATES]>,
}

impl Raster {
    /// Resets the raster context and prepares to render triangle state machines
    /// for the triangles described by the index buffer `tris` and vertex buffer
    /// `vertices`.
    pub fn reset(&mut self, tris: &[Tri], vertices: &[Vec3i]) {
        self.pending.clear();
        self.active.clear();

        let mut states = 0;
        for tri in tris {
            let tri_ref = TriRef::normalize(
                &vertices[tri.vertex_indices[0]],
                &vertices[tri.vertex_indices[1]],
                &vertices[tri.vertex_indices[2]],
            );
            if let Some(tri_ref) = tri_ref {
                let (top, bot) = tri_ref.to_states(tri.color);
                self.tris[states] = top;
                self.pending.push(StateIndex::checked(states).unwrap());
                states += 1;
                if let Some(bot) = bot {
                    self.tris[states] = bot;
                    self.pending.push(StateIndex::checked(states).unwrap());
                    states += 1;
                }
            }
        }

        let tris = &self.tris;
        self.pending.sort_unstable_by(|i, j| {
            j.index(tris).top_y.cmp(&i.index(tris).top_y)
        });
    }

    pub fn step<F>(&mut self, scanline: usize, mut body: F)
        where F: FnMut(Range<usize>, u8),
    {
        // Move any tris that start on this scanline from pending to active.
        // Because the tris are sorted descending by top_y, the relevant ones
        // will be in a suffix of self.pending.
        while let Some(i) = self.pending.last().cloned() {
            if i.index(&self.tris).top_y == scanline {
                self.pending.pop();
                self.active.push(i);
            } else {
                break;
            }
        }

        // Process the pixel range for each active tri, stepping it forward.
        for i in &self.active {
            let tri = i.index_mut(&mut self.tris);
            let color = tri.color;
            body(tri.evaluate(scanline), color);
        }

        // Retire tris that are ending.
        // TODO: using retain here makes us O(n) because it preserves order.
        let tris = &self.tris;
        self.active.retain(|i| i.index(tris).last_y != scanline);
    }
}

/// The screen-space projected vertices of a triangle.
///
/// Vertices are by-reference because they live in a vertex buffer.
pub struct TriRef<'a>(&'a Vec3i, &'a Vec3i, &'a Vec3i);

impl<'a> TriRef<'a> {
    /// Makes a new `TriRef` in normalized vertex order. If the triangle is
    /// edge-on to the viewer, and would generate no pixels, returns `None`.
    ///
    /// `v0`, `v1`, `v2` should be a clockwise circuit around the triangle.
    ///
    /// The triangle should be camera-facing.
    pub fn normalize(mut v0: &'a Vec3i, mut v1: &'a Vec3i, mut v2: &'a Vec3i) -> Option<Self> {
        // Reject edge-on triangles. Simplifies the rest of our math.
        if v0.1 == v1.1 && v1.1 == v2.1 { return None; }

        if v2.1 <= v0.1 {
            // Rotate clockwise until normalized.
            while v2.1 <= v0.1 {
                mem::swap(&mut v2, &mut v0);
                mem::swap(&mut v0, &mut v1);
            }
        } else if v1.1 < v0.1 {
            // Rotate counter-clockwise.
            while v1.1 < v0.1 {
                mem::swap(&mut v0, &mut v1);
                mem::swap(&mut v1, &mut v2);
            }
        } 

        Some(TriRef(v0, v1, v2))
    }

    fn to_states(self, color: u8) -> (TriState, Option<TriState>) {
        let top_y = (self.0).1;

        if (self.1).1 == top_y {
            // 0 -> 1 forms a flat edge. Only one triangle is required, with
            // edges 0 -> 2, 1 -> 2.
            let top = TriState {
                top_y: top_y as usize,
                last_y: ((self.2).1 - 1) as usize,
                left: Line::between(self.0, self.2),
                right: Line::between(self.1, self.2),
                color,
            };
            (top, None)
        } else if (self.1).1 == (self.2).1 {
            // 1 -> 2 forms a flat (bottom) edge. Only one triangle is required,
            // with edges 0 -> 2, 0 -> 1
            let top = TriState {
                top_y: top_y as usize,
                last_y: ((self.2).1 - 1) as usize,
                left: Line::between(self.0, self.2),
                right: Line::between(self.0, self.1),
                color,
            };
            (top, None)
        } else if (self.1).1 < (self.2).1 {
            // Two triangles, break is at self.1.1
            let top = TriState {
                top_y: top_y as usize,
                last_y: ((self.1).1 - 1) as usize,
                left: Line::between(self.0, self.2),
                right: Line::between(self.0, self.1),
                color,
            };
            let bot = TriState {
                top_y: (self.1).1 as usize,
                last_y: ((self.2).1 - 1) as usize,
                left: Line::between(self.0, self.2),
                right: Line::between(self.1, self.2),
                color,
            };
            (top, Some(bot))
        } else {
            // Two triangles, break is at self.2.1
            let top = TriState {
                top_y: top_y as usize,
                last_y: ((self.2).1 - 1) as usize,
                left: Line::between(self.0, self.2),
                right: Line::between(self.0, self.1),
                color,
            };
            let bot = TriState {
                top_y: (self.2).1 as usize,
                last_y: ((self.1).1 - 1) as usize,
                left: Line::between(self.2, self.1),
                right: Line::between(self.0, self.1),
                color,
            };
            (top, Some(bot))
        }
    }            
}
