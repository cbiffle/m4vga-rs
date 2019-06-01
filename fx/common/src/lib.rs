#![no_std]

pub trait Demo<'a> {
    type Raster: Raster + 'a;
    type Render: Render + 'a;

    fn split(&'a mut self) -> (Self::Raster, Self::Render);
}

pub trait Raster {
    fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        _: m4vga::priority::I0,
    );
}

pub trait Render {
    fn render_frame(&mut self, frame: usize, _: m4vga::priority::Thread);
}
