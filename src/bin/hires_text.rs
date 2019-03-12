//! Attributed text display.
//!
//! This shows a mostly static screen of colored text, with some dynamic
//! elements for fun.
//!
//! # Theory of operation
//!
//! We allocate a static buffer, `TEXT_BUF`, to hold attributed text. Our
//! rasterizer expects that buffer to contain values of type `AChar`, for
//! *attributed char*.
//!
//! Because we want to update the text from the application loop, but read it
//! during scanout, we enclose the buffer in a `SpinLock`. Before updating the
//! text in the application loop, we `sync_to_vblank` to ensure that we're not
//! racing scanout.
//!
//! At startup, at the top of `main`, the demo fills the text buffer with text.
//! It then activates the display driver, giving it a raster callback and a main
//! loop.
//!
//! The raster callback uses `m4vga::rast::text_10x16` to draw the top 592 lines
//! of the display, using the standard font, and then `m4vga::rast::solid_color`
//! to draw the partial last line. During the rendering of the text part of the
//! display, it locks `TEXT_BUF` on every horizontal retrace -- this is pretty
//! cheap, and won't race the application loop because we only call the raster
//! callback outside of the vertical blanking interval.
//!
//! The application loop calls `sync_to_vblank` every iteration and then writes
//! an updated frame number into the `TEXT_BUF`. Note that we can use `write!`
//! here despite `no_std`; we don't have to write our own numeric formatting
//! code, which is great.

#![no_std]
#![no_main]

#[cfg(feature = "panic-halt")]
extern crate panic_halt;
#[cfg(feature = "panic-itm")]
extern crate panic_itm;

use stm32f4;
use stm32f4::stm32f407::interrupt;

use font_10x16;
use m4vga::rast::text_10x16::{self, AChar};
use m4vga::util::spin_lock::SpinLock;

const COLS: usize = 80;
const ROWS: usize = 37;

const WHITE: u8 = 0b11_11_11;
const BLACK: u8 = 0b00_00_00;
const DK_GRAY: u8 = 0b01_01_01;
const RED: u8 = 0b00_00_11;
const BLUE: u8 = 0b11_00_00;

static TEXT_BUF: SpinLock<[AChar; COLS * ROWS]> =
    SpinLock::new([AChar::from_ascii_char(0); COLS * ROWS]);

/// Demo entry point. Responsible for starting up the display driver and
/// providing callbacks.
#[allow(unused_parens)] // TODO bug in cortex_m_rt
#[cortex_m_rt::entry]
fn main() -> ! {
    {
        // Type some stuff into the buffer.
        let mut c = TEXT_BUF.try_lock().unwrap();
        let mut c = Cursor::new(&mut *c);
        c.fg = WHITE;
        c.bg = DK_GRAY;
        c.puts(b"800x600 Attributed Text Demo\n");
        c.bg = BLACK;
        c.puts(b"10x16 point characters in an 80x37 grid, with ");
        c.fg = RED;
        c.puts(b"foreground");
        c.fg = WHITE;
        c.puts(b" and ");
        c.bg = BLUE;
        c.puts(b"background");
        c.bg = BLACK;
        c.puts(b" colors.\n");
        c.bg = 0b10_00_00;
        c.puts(
            br#"
       Lorem ipsum dolor sit amet, consectetur adipiscing elit. Nam ut
       tellus quam. Cras ornare facilisis sollicitudin. Quisque quis
       imperdiet mauris. Proin malesuada nibh dolor, eu luctus mauris
       ultricies vitae. Interdum et malesuada fames ac ante ipsum primis
       in faucibus. Aenean tincidunt viverra ultricies. Quisque rutrum
       vehicula pulvinar.

       Etiam commodo dui quis nibh dignissim laoreet. Aenean erat justo,
       hendrerit ac adipiscing tempus, suscipit quis dui. Vestibulum ante
       ipsum primis in faucibus orci luctus et ultrices posuere cubilia
       Curae; Proin tempus bibendum ultricies. Etiam sit amet arcu quis
       diam dictum suscipit eu nec odio. Donec cursus hendrerit porttitor.
       Suspendisse ornare, diam vitae faucibus dictum, leo enim vestibulum
       neque, id tempor tellus sem pretium lectus. Maecenas nunc nisl,
       aliquam non quam at, vulputate lacinia dolor. Vestibulum nisi orci,
       viverra ut neque semper, imperdiet laoreet ligula. Nullam venenatis
       orci eget nibh egestas, sit amet sollicitudin erat cursus.

       Nullam id ornare tellus, vel porta lectus. Suspendisse pretium leo
       enim, vel elementum nibh feugiat non. Etiam non vulputate quam, sit
       amet semper ante. In fermentum imperdiet sem non consectetur. Donec
       egestas, massa a fermentum viverra, lectus augue hendrerit odio,
       vitae molestie nibh nunc ut metus. Nulla commodo, lacus nec
       interdum dignissim, libero dolor consequat mi, non euismod velit
       sem nec dui. Praesent ligula turpis, auctor non purus eu,
       adipiscing pellentesque felis."#,
        );
        c.putc(b'\n');
    }

    // Give the driver its hardware resources...
    m4vga::take_hardware()
        // ...select a display timing...
        .configure_timing(&m4vga::timing::SVGA_800_600)
        // ... and provide a raster callback.
        .with_raster(
            // The raster callback is invoked on every horizontal retrace to
            // provide new pixels.
            |ln, tgt, ctx, _| {
                if ln < 592 {
                    text_10x16::unpack(
                        &*TEXT_BUF.try_lock().expect("rast buf access"),
                        font_10x16::FONT.as_glyph_slices(),
                        &mut **tgt,
                        ln,
                        COLS,
                    );
                    ctx.target_range = 0..COLS * text_10x16::GLYPH_COLS;
                } else {
                    // There's a partial 38th line visible on the display.
                    // Trying to display it will panic by going out of range on
                    // the 80x37 buffer. Instead, we'll just black it out:
                    m4vga::rast::solid_color_fill(tgt, ctx, 800, 0);
                    // Save some CPU while we're at it by not invoking the
                    // callback again this frame.
                    ctx.repeat_lines = 600 - 592;
                }
            },
            // This closure contains the main loop of the program.
            |vga| {
                // Enable outputs. The driver doesn't do this for you in case
                // you want to set up some graphics before doing so.
                vga.video_on();
                let mut frame_no = 0;
                // Spin forever!
                loop {
                    use core::fmt::Write;

                    vga.sync_to_vblank();
                    let mut buf = TEXT_BUF.try_lock().expect("app buf access");
                    let mut c = Cursor::new(&mut *buf);
                    c.goto(36, 0);
                    c.bg = 0;
                    c.fg = 0b00_11_00;
                    write!(&mut c, "Welcome to frame {}", frame_no).unwrap();
                    frame_no += 1;
                }
            },
        )
}

/// A simple cursor wrapping a text buffer. Provides terminal-style operations.
struct Cursor<'a> {
    buf: &'a mut [AChar; COLS * ROWS],
    row: usize,
    col: usize,
    fg: m4vga::Pixel,
    bg: m4vga::Pixel,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a mut [AChar; COLS * ROWS]) -> Self {
        Cursor {
            buf,
            row: 0,
            col: 0,
            fg: 0xFF,
            bg: 0b100000,
        }
    }

    /// Types a character terminal-style and advances the cursor. `'\n'` is
    /// interpreted as carriage return plus line feed.
    pub fn putc(&mut self, c: u8) {
        match c {
            b'\n' => {
                let pos = self.row * COLS + self.col;
                let end_of_line = (pos + (COLS - 1)) / COLS * COLS;
                for p in &mut self.buf[pos..end_of_line] {
                    *p = AChar::from_ascii_char(b' ')
                        .with_foreground(self.fg)
                        .with_background(self.bg)
                }
                self.col = 0;
                self.row += 1;
            }
            _ => {
                self.buf[self.row * COLS + self.col] =
                    AChar::from_ascii_char(c)
                        .with_foreground(self.fg)
                        .with_background(self.bg);
                self.col += 1;
                if self.col == COLS {
                    self.col = 0;
                    self.row += 1;
                }
            }
        }
    }

    /// Types each character from an ASCII slice.
    pub fn puts(&mut self, s: &[u8]) {
        for c in s {
            self.putc(*c)
        }
    }

    /// Repositions the cursor.
    pub fn goto(&mut self, row: usize, col: usize) {
        assert!(row < ROWS);
        assert!(col < COLS);
        self.row = row;
        self.col = col;
    }
}

/// Allows use of a `Cursor` in formatting and `write!`.
impl<'a> core::fmt::Write for Cursor<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            let c = c as u32;
            self.putc(c as u8);
        }

        Ok(())
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
