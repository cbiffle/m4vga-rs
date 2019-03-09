use std::collections::HashMap;
use std::collections::HashSet;
use std::io::{self, Read, Seek, Write};

use byteorder::{LittleEndian, ReadBytesExt};
use ordered_float::OrderedFloat;

const Q: f32 = 10_000.;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Point3([OrderedFloat<f32>; 3]);

impl Point3 {
    /// Forces the point onto a quantized grid, to merge points that are nearby
    /// but not exactly identical.
    fn quantized(self) -> Self {
        fn q(v: OrderedFloat<f32>) -> OrderedFloat<f32> {
            OrderedFloat((v.0 * Q).round() / Q)
        }

        Point3([q(self.0[0]), q(self.0[1]), q(self.0[2])])
    }

    fn read_from(mut input: impl Read) -> io::Result<Self> {
        Ok(Point3([
            OrderedFloat(input.read_f32::<LittleEndian>()?),
            OrderedFloat(input.read_f32::<LittleEndian>()?),
            OrderedFloat(input.read_f32::<LittleEndian>()?),
        ]))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
struct Edge(usize, usize);

impl Edge {
    fn normalize(self) -> Self {
        if self.0 < self.1 {
            self
        } else {
            Edge(self.1, self.0)
        }
    }

    fn is_trivial(&self) -> bool {
        self.0 == self.1
    }
}

fn munge(mut input: impl Read + Seek) -> io::Result<Munged> {
    // skip header
    input.seek(io::SeekFrom::Current(80))?;
    let tri_count = input.read_u32::<LittleEndian>()?;
    eprintln!("tri_count = {}", tri_count);

    let mut unique_points: HashMap<Point3, usize> = HashMap::default();
    let mut ordered_points = vec![];
    let mut unique_edges: HashSet<Edge> = HashSet::default();
    let mut trivial_edges = 0;
    let mut duplicate_edges = 0;

    for _ in 0..tri_count {
        // Skip normal vector.
        input.seek(io::SeekFrom::Current(3 * 4))?;

        let mut indices = [0; 3];
        for index in indices.iter_mut() {
            let mut p = Point3::read_from(&mut input)?.quantized();
            p.0[2].0 -= 20.; // legacy Z shift, TODO
            *index = *unique_points.entry(p).or_insert_with(|| {
                let n = ordered_points.len();
                ordered_points.push(p);
                n
            });
        }

        input.seek(io::SeekFrom::Current(2))?;

        let edges = [
            Edge(indices[0], indices[1]),
            Edge(indices[1], indices[2]),
            Edge(indices[2], indices[0]),
        ];

        for edge in &edges {
            let edge = edge.normalize();
            if edge.is_trivial() {
                trivial_edges += 1;
                continue;
            }

            if !unique_edges.insert(edge) {
                duplicate_edges += 1;
                continue;
            }
        }
    }

    eprintln!("points.len: {}", ordered_points.len());
    eprintln!("edges.len: {}", unique_edges.len());
    eprintln!("trivial_edges: {}", trivial_edges);
    eprintln!("duplicate_edges: {}", duplicate_edges);

    Ok(Munged {
        trivial_edges,
        duplicate_edges,
        points: ordered_points,
        edges: unique_edges.into_iter().collect(),
    })
}

struct Munged {
    pub trivial_edges: usize,
    pub duplicate_edges: usize,
    pub points: Vec<Point3>,
    pub edges: Vec<Edge>,
}

pub fn generate(
    input: impl Read + Seek,
    mut output: impl Write,
) -> io::Result<()> {
    let mut munged = munge(input)?;

    writeln!(output, "use m4vga::math::{{Vec3, Vec3f}};")?;

    writeln!(
        output,
        "pub const VERTEX_COUNT: usize = {};",
        munged.points.len()
    )?;
    writeln!(output, "pub static VERTICES: [Vec3f; VERTEX_COUNT] = [")?;
    for p in munged.points {
        writeln!(
            output,
            "    Vec3({}f32, {}f32, {}f32),",
            p.0[0], p.0[1], p.0[2]
        )?;
    }
    writeln!(output, "];")?;

    writeln!(
        output,
        "pub static EDGES: [(u16, u16); {}] = [",
        munged.edges.len()
    )?;
    munged.edges.sort_unstable();
    for Edge(start, end) in munged.edges {
        writeln!(output, "    ({}, {}),", start, end)?;
    }
    writeln!(output, "];")?;

    Ok(())
}
