#![no_std]
#![no_main]

#![allow(unused)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

mod armv7m;
mod stm32;

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

    unsafe {
        // Use lazy context stacking for FP, so that we can use FP from
        // interrupts but don't always incur an FP context save.
        cp.FPU.fpccr.write((1 << 31)  // automatic save
                           | (1 << 30)  // lazy save
                           );
    }

    let p = device::Peripherals::take().unwrap();

    let mut vga = vga::init(
        cp.NVIC,
        &mut cp.SCB,
        &p.FLASH,
        &p.DBG,
        p.RCC,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2,
    );

    vga.with_raster(
        |_, tgt, ctx| {
            let mut pixel = 0;
            for t in &mut tgt[0..800] {
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
