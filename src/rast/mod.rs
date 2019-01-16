pub mod bitmap_1;
pub mod text_10x16;

pub type Pixel = u8;

pub trait Rasterize: Sync {
    fn rasterize(&mut self,
                 cycles_per_pixel: u32,
                 line_number: usize,
                 target: &mut [Pixel])
        -> RasterInfo;
}

#[derive(Copy, Clone, Debug, Default)]
pub struct RasterInfo {
    offset: i32,
    length: usize,
    cycles_per_pixel: u32,
    repeat_lines: u32,
}
