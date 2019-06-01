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

/// Initializes a `State` from static context.
///
/// # Safety
///
/// This is safe as long as it's only called once.
pub unsafe fn init() -> super::State<&'static mut [u32], &'static table::Table> {
    let table = &mut TABLE;
    table::compute(table);
    let table = &*table;

    let fg = SpinLock::new(&mut BUF0 as &mut [u32]);
    let bg = &mut BUF1 as &mut [u32];
    
    super::State { fg, bg, table }
}
