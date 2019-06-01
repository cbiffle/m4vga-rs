//! Bare-metal support routines, still hardware-dependent.
//!
//! This is responsible for things like allocating static buffers, coordinating
//! spinlocks, and calling assembly rasterizer routines, none of which are
//! relevant to the hosted version.

use m4vga::util::spin_lock::SpinLock;

use super::table;

static mut BUF0: [u32; super::BUFFER_WORDS] = [0; super::BUFFER_WORDS];

#[link_section = ".local_bss"]
static mut BUF1: [u32; super::BUFFER_WORDS] = [0; super::BUFFER_WORDS];

#[no_mangle]
static mut TABLE: table::Table =
    [[table::Entry::zero(); table::TAB_WIDTH]; table::TAB_HEIGHT];

pub struct State<'d> {
    fg: SpinLock<&'d mut [u32; super::BUFFER_WORDS]>,
    bg: Option<&'d mut [u32; super::BUFFER_WORDS]>,
    table: &'d table::Table,
}

pub struct RasterState<'a, 'd> {
    fg: &'a SpinLock<&'d mut [u32; super::BUFFER_WORDS]>,
}

pub struct RenderState<'a, 'd> {
    fg: &'a SpinLock<&'d mut [u32; super::BUFFER_WORDS]>,
    bg: &'d mut [u32; super::BUFFER_WORDS],
    table: &'d table::Table,
}

/// Initializes a `State` from static context.
///
/// # Safety
///
/// This is safe as long as it's only called once.
pub unsafe fn init() -> State<'static> {
    let table = &mut TABLE;
    table::compute(table);
    let table = &*table;

    let fg = SpinLock::new(&mut BUF0);
    let bg = Some(&mut BUF1);
    
    State { fg, bg, table }
}

impl<'d> State<'d> {
    pub fn split(&mut self) -> (RasterState<'_, 'd>, RenderState<'_, 'd>) {
        (
            RasterState {
                fg: &self.fg,
            },
            RenderState {
                fg: &self.fg,
                bg: self.bg.take().unwrap(),
                table: self.table,
            },
        )
    }
}

impl<'a, 'd> RasterState<'a, 'd> {
    pub fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        _: m4vga::priority::I0,
    ) {
        super::raster_callback(
            ln,
            target,
            ctx,
            self.fg,
        )
    }
        
}

impl<'a, 'd> RenderState<'a, 'd> {
    pub fn render_loop(&mut self, vga: &mut m4vga::Vga<m4vga::Live>) -> ! {
        vga.video_on();
        let mut frame = 0;
        loop {
            vga.sync_to_vblank();
            super::render_frame(
                &mut self.bg,
                self.fg,
                self.table,
                frame,
            );
            frame = (frame + 1) % 65536;
        }
    }
}
