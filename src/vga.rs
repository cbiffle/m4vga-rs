use stm32f4::stm32f407 as device;
use cortex_m::peripheral::scb::SystemHandler;

pub struct Band {
    rasterizer_idx: usize,
    line_count: usize,
    next_idx: usize,
}

pub struct Vga {
}

impl Vga {
}

fn init() {
    let mut cp = device::CorePeripherals::take().unwrap();
    let p = device::Peripherals::take().unwrap();

    // Turn on I/O compensation cell to reduce noise on power supply.
    p.RCC.apb2enr.modify(|_, w| w.syscfgen().enabled());
    // TODO: CMPCR seems to be modeled as read-only (?)
    //p.SYSCFG.cmpcr.modify(|_, w| w.cmp_pd().enabled());

    // Turn a bunch of stuff on.
    p.RCC.ahb1enr.modify(|_, w| w
                         .gpioben().enabled()
                         .gpioeen().enabled()
                         .dma2en().enabled());

    // DMA configuration.
    
    // Configure FIFO.
    p.DMA2.s5fcr.write(|w| w
                       .fth().quarter()
                       .dmdis().enabled()
                       .feie().disabled());
    // Enable the pixel-generation timer.
    p.RCC.apb2enr.modify(|_, w| w.tim1en().enabled());
    p.TIM1.psc.reset(); // Divide by 1 => PSC=0
    p.TIM1.cr1.write(|w| w.urs().counter_only());
    p.TIM1.dier.write(|w| w.ude().set_bit());

    // Configure interrupt priorities. This is safe because we haven't enabled
    // interrupts yet.
    unsafe {
        cp.NVIC.set_priority(device::Interrupt::TIM4, 0);
        cp.NVIC.set_priority(device::Interrupt::TIM3, 16);
        cp.SCB.set_priority(SystemHandler::SysTick, 0xFF);
    }

    // TODO more
}
