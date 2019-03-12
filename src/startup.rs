use cortex_m::asm;
use cortex_m_rt::pre_init;
use stm32f4;

use stm32f4::stm32f407 as device;

extern "C" {
    static __vector_table_in_flash: u8;
    static mut _local_data_start: u32;
    static mut _local_data_end: u32;
    static _local_data_init: u32;
    static mut _local_bss_start: u32;
    static mut _local_bss_end: u32;
    static mut _sram16_bss_start: u32;
    static mut _sram16_bss_end: u32;
}

#[pre_init]
unsafe fn pre_init() {
    //
    //     GIANT WARNING LABEL
    //
    // This function runs before .data and .bss are initialized. Any access to a
    // `static` is undefined behavior!
    //
    // Between that and this whole function being `unsafe`, we are basically
    // writing C.

    // The cortex_m crate does not grant everyone ambient authority to mess with
    // the SCB. This is to its *immense credit* in the general case... but we're
    // special, so:
    let scb = &*cortex_m::peripheral::SCB::ptr(); // tada

    // Enable ARMv7-M detailed fault reporting before doing anything
    // interesting. That way, if we screw something up below, we get (say) a Bus
    // Fault with useful metadata, instead of a meaningless Hard Fault.
    scb.shcrs.write(scb.shcrs.read() | (0b111 << 16));

    // VTOR points at address 0 at reset (i.e. right now). On STM32F4 the
    // address space at zero is a configurable alias for one of several possible
    // memories. Currently, it aliases Flash, which actually lives at a higher
    // address. We're going to switch address 0 to alias SRAM, so to keep a
    // valid vector table, we need to move VTOR to point to Flash's actual
    // non-aliased address.
    scb.vtor.write(&__vector_table_in_flash as *const _ as u32);

    // We need the SYSCFG peripheral, which is brought out of reset
    // automatically -- but its interface is not clocked by default, which makes
    // it hard to talk to. Fix that.
    (*device::RCC::ptr())
        .apb2enr
        .modify(|_, w| w.syscfgen().enabled());

    asm::dmb(); // ensure clock's a-runnin' before we try to write to it

    // Remap SRAM112 to address 0.
    (*device::SYSCFG::ptr())
        .memrm
        .write(|w| w.mem_mode().bits(0b11));

    // Now, please.
    asm::dsb();
    asm::isb();

    // Turn SYSCFG back off for good measure.
    (*device::RCC::ptr())
        .apb2enr
        .modify(|_, w| w.syscfgen().disabled());

    // ----------- it is now *slightly* safer to access statics ------------

    // Primary data and bss are uninitialized, but will be initialized shortly.
    // Initialize our discontiguous regions.
    r0::zero_bss(&mut _local_bss_start, &mut _local_bss_end);
    r0::zero_bss(&mut _sram16_bss_start, &mut _sram16_bss_end);
    r0::init_data(
        &mut _local_data_start,
        &mut _local_data_end,
        &_local_data_init,
    );

    // ----------- it is now safe to access statics ------------

    // r0 enables floating point for us, on Cortex-M4F. However, by default,
    // floating point is set up to work only in thread-mode code, and not in
    // interrupt handlers. We use a lot of floating point in interrupt handlers,
    // so we need to fix this. The key is to enable stacking of floating point
    // registers on interrupt entry. The Cortex-M4F has an Automatic FP Context
    // Save feature that we'll switch on.
    //
    // Because we're sensitive to interrupt latency, we don't want to do this
    // every time. In particular, the start-of-active-video ISR does not use
    // floating point, and it would be a shame to slow that one down.
    //
    // To avoid that, we also switch on Lazy FP Context Save. This reserves
    // space in the interrupt frame for the FP context, but delays actually
    // writing it until the ISR tries to use floating point.
    let fpccr_val = (1 << 31)  // automatic
                  | (1 << 30); // lazy
    (*cortex_m::peripheral::FPU::ptr()).fpccr.write(fpccr_val);
}
