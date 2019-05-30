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

pub struct State {
    fg: SpinLock<&'static mut [u32; super::BUFFER_WORDS]>,
    bg: Option<&'static mut [u32; super::BUFFER_WORDS]>,
    table: &'static table::Table,
}

pub struct RasterState<'a> {
    fg: &'a SpinLock<&'static mut [u32; super::BUFFER_WORDS]>,
}

pub struct RenderState<'a> {
    fg: &'a SpinLock<&'static mut [u32; super::BUFFER_WORDS]>,
    bg: &'static mut [u32; super::BUFFER_WORDS],
    table: &'static table::Table,
}

/// Initializes a `State` from static context.
///
/// # Safety
///
/// This is safe as long as it's only called once.
pub unsafe fn init() -> State {
    let table = &mut TABLE;
    table::compute(table);
    let table = &*table;

    let fg = SpinLock::new(&mut BUF0);
    let bg = Some(&mut BUF1);
    
    State { fg, bg, table }
}

impl State {
    pub fn split(&mut self) -> (RasterState, RenderState) {
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

impl<'a> RasterState<'a> {
    pub fn raster_callback(
        &mut self,
        ln: usize,
        target: &mut m4vga::rast::TargetBuffer,
        ctx: &mut m4vga::rast::RasterCtx,
        _: m4vga::priority::I0,
    ) {
        // Our image is slightly smaller than the display. Black the top and
        // bottom borders.
        if ln < 4 || ln > 595 {
            m4vga::rast::solid_color_fill(target, ctx, 800, 0);
            return;
        }

        let buf = self.fg.try_lock().expect("rast fg access");

        let ln = ln / super::SCALE;

        if ln < super::HALF_HEIGHT {
            m4vga::rast::direct::direct_color(ln, target, ctx, *buf, super::BUFFER_STRIDE);
        } else {
            m4vga::rast::direct::direct_color_mirror(ln, target, ctx, *buf, super::BUFFER_STRIDE, super::HEIGHT);
        }

        ctx.cycles_per_pixel *= super::SCALE;
        ctx.repeat_lines = super::SCALE - 1;
    }
        
}

impl<'a> RenderState<'a> {
    pub fn render_loop(&mut self, vga: &mut m4vga::Vga<m4vga::Live>) -> ! {
        vga.video_on();
        let mut frame = 0;
        loop {
            vga.sync_to_vblank();
            core::mem::swap(
                &mut self.bg,
                &mut *self.fg.try_lock().expect("swap access"),
            );
            let bg = u32_as_u8_mut(self.bg);
            m4vga::util::measurement::sig_d_set();
            super::render::render(self.table, bg, frame);
            m4vga::util::measurement::sig_d_clear();
            frame = (frame + 1) % 65536;
        }
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
