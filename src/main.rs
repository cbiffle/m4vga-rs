#![no_std]
#![no_main]

#![allow(unused)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

mod util;
mod copy_words;
mod arena;
mod vga;
mod font_10x16;

use cortex_m::asm;
use cortex_m_rt::{entry, pre_init};
use stm32f4;

use stm32f4::stm32f407 as device;

#[pre_init]
unsafe fn pre_init() {
    // GIANT WARNING LABEL
    //
    // This function runs before .data and .bss are initialized. Any access to a
    // `static` is undefined behavior!

    use core::sync::atomic::{fence, Ordering};

    // Turn on power to the SYSCFG peripheral, which is a prerequisite to what
    // we actually want to do.
    let rcc = &*device::RCC::ptr();
    rcc.apb2enr.modify(|_, w| w.syscfgen().enabled());

    asm::dsb(); // ensure power's up before we try to write to it

    // Remap SRAM112 to address 0.
    let syscfg = &*device::SYSCFG::ptr();
    syscfg.memrm.write(|w| w.mem_mode().bits(0b11));

    // Now, please.
    asm::dsb();
    asm::isb();
}

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::peripheral::Peripherals::take().unwrap();

    {
        // Enable faults, so they don't immediately escalate to HardFault.
        let shcsr = cp.SCB.shcrs.read();
        unsafe { cp.SCB.shcrs.write(shcsr | (0b111 << 16)) }
    }

    let p = device::Peripherals::take().unwrap();

    let mut vga = vga::init(
        &mut cp,
        &p.RCC,
        &p.FLASH,
        &p.DBG,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2,
    );

    vga.with_raster(
        |_, ctx| {
            let mut pixel = 0;
            for t in &mut ctx.target[0..800] {
                *t = pixel;
                pixel ^= 0xFF;
            }
            ctx.target_range = 0..800;
            ctx.repeat_lines = 599;
        },
        |vga| {
            vga.video_on();
            loop {}
        },
    )
}
