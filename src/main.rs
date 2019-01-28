#![no_std]
#![no_main]

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
#[allow(unused)] // TODO
mod font_10x16;

use cortex_m::asm;
use cortex_m_rt::{entry, pre_init, exception};
use stm32f4;

use stm32f4::stm32f407 as device;
use stm32f4::stm32f407::interrupt;

#[pre_init]
unsafe fn pre_init() {
    // GIANT WARNING LABEL
    //
    // This function runs before .data and .bss are initialized. Any access to a
    // `static` is undefined behavior!

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

#[allow(unused_parens)] // TODO bug in cortex_m_rt
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
        p.FLASH,
        &p.DBG,
        p.RCC,
        p.GPIOB,
        p.GPIOE,
        p.TIM1,
        p.TIM3,
        p.TIM4,
        p.DMA2,
    ).configure_timing(&SVGA_800_600);

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

static SVGA_800_600: vga::Timing = vga::Timing {
    clock_config: stm32::ClockConfig {
        crystal_hz: 8000000.0,// external crystal Hz
        crystal_divisor: 4,   // divide down to 2Mhz
        vco_multiplier: 160,  // multiply up to 320MHz VCO
        // divide by 2 for 160MHz CPU clock
        general_divisor: device::rcc::pllcfgr::PLLPW::DIV2,
        pll48_divisor: 7,     // divide by 7 for 48MHz-ish SDIO clock
        // divide CPU clock by 1 for 160MHz AHB clock
        ahb_divisor: device::rcc::cfgr::HPREW::DIV1,
        // divide CPU clock by 4 for 40MHz APB1 clock.
        apb1_divisor: device::rcc::cfgr::PPRE2W::DIV4,
        // divide CPU clock by 2 for 80MHz APB2 clock.
        apb2_divisor: device::rcc::cfgr::PPRE2W::DIV2,

        // 5 wait states for 160MHz at 3.3V.
        flash_latency: device::flash::acr::LATENCYW::WS5,
    },

    add_cycles_per_pixel: 0,

    line_pixels      : 1056,
    sync_pixels      : 128,
    back_porch_pixels: 88,
    video_lead       : 19,
    video_pixels     : 800,
    hsync_polarity   : vga::Polarity::Positive,

    vsync_start_line: 1,
    vsync_end_line  : 1 + 4,
    video_start_line: 1 + 4 + 23,
    video_end_line  : 1 + 4 + 23 + 600,
    vsync_polarity  : vga::Polarity::Positive,
};

#[exception]
fn PendSV() {
    vga::bg_rast::maintain_raster_isr()
}

#[interrupt]
fn TIM3() {
    vga::shock::shock_absorber_isr()
}

#[interrupt]
fn TIM4() {
    vga::hstate::hstate_isr()
}


