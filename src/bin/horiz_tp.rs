#![no_std]
#![no_main]

extern crate panic_itm;

use cortex_m::iprintln;
use cortex_m_rt::{entry, exception};
use stm32f4;

use stm32f4::stm32f407 as device;
use stm32f4::stm32f407::interrupt;

use m4vga_rs::vga;

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

    let vga = vga::init(
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
    );

    let mut vga = vga.configure_timing(&m4vga_rs::vga::timing::SVGA_800_600);

    iprintln!(&mut cp.ITM.stim[0], "clocks configured, starting rasterization");
    
    vga.with_raster(
        |_, tgt, ctx| {
            let mut pixel = 0xFF;
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

#[exception]
fn PendSV() {
    m4vga_rs::vga::bg_rast::maintain_raster_isr()
}

#[interrupt]
fn TIM3() {
    m4vga_rs::vga::shock::shock_absorber_isr()
}

#[interrupt]
fn TIM4() {
    m4vga_rs::vga::hstate::hstate_isr()
}
