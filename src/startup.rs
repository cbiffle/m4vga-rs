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

    // Before doing anything interesting, enable ARMv7-M detailed fault
    // reporting. That way if we screw something up below, we get (say) a Bus
    // Fault with useful metadata, instead of a meaningless Hard Fault.
    scb.shcrs.write(scb.shcrs.read() | (0b111 << 16));

    // VTOR points at address 0 at reset (i.e. right now). Currently this
    // references flash, but we're about to remap memory to place SRAM there.
    // And so, we need to change VTOR to point at the stable address of the
    // vector table in Flash.
    scb.vtor.write(&__vector_table_in_flash as *const _ as u32);

    // At reset, SYSCFG is out of reset but its interface is not being clocked.
    // Fix that.
    (*device::RCC::ptr())
        .apb2enr.modify(|_, w| w.syscfgen().enabled());

    asm::dmb(); // ensure clock's a-runnin' before we try to write to it

    // Remap SRAM112 to address 0.
    (*device::SYSCFG::ptr())
        .memrm.write(|w| w.mem_mode().bits(0b11));

    // Now, please. For all we know we're about to return into it!
    asm::dsb();
    asm::isb();

    // ----------- it is now slightly safer to access statics ------------

    // r0 has already enabled floating point, but we use floating point in ISRs.
    // To make this work, we need to enable automatic FP context save, so that
    // the thread-level FP state won't get corrupted. However, doing this
    // naively increases interrupt latency by *tens of cycles*. To avoid paying
    // this penalty for ISRs that don't use FP, we also enable *lazy* context
    // save, which waits until the first FPU instruction.
    (*cortex_m::peripheral::FPU::ptr())
        .fpccr.write((1 << 31)      // automatic
                     | (1 << 30)    // lazy
                     );

    // Turn SYSCFG back off for good measure.
    (*device::RCC::ptr())
        .apb2enr.modify(|_, w| w.syscfgen().disabled());

    // Primary data and bss are uninitialized, but will be initialized shortly.
    // Initialize our discontiguous regions.
    r0::zero_bss(&mut _local_bss_start, &mut _local_bss_end);
    r0::zero_bss(&mut _sram16_bss_start, &mut _sram16_bss_end);
    r0::init_data(
        &mut _local_data_start,
        &mut _local_data_end,
        &_local_data_init,
    );
}
