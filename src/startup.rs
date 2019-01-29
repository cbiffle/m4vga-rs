use cortex_m::asm;
use cortex_m_rt::pre_init;
use stm32f4;

use stm32f4::stm32f407 as device;

#[pre_init]
unsafe fn pre_init() {
    // GIANT WARNING LABEL
    //
    // This function runs before .data and .bss are initialized. Any access to a
    // `static` is undefined behavior!

    // Point VTOR to the Flash-resident copy of the vector table, instead of its
    // current alias at zero. Have to do this before remapping memory or any
    // exception will fail hard.
    //
    // TODO: the vector table needs to end up in RAM. The way I did this in C++
    // was to arrange the sections so the table got copied with the initialised
    // data image. Do that, or something less clever.
    let scb = &*cortex_m::peripheral::SCB::ptr();
    scb.vtor.write(0x0800_0000); // TODO magical address should come from symbol

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
