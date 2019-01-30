//! Performance measurement support using GPIOs.
//!
//! This totally circumvents all hardware ownership.

use stm32f4::stm32f407 as device;

/// Sets up the measurement subsystem.
///
/// # Safety
///
/// This is safe *as long as* it's not preempted. If interrupts are enabled, and
/// interrupts attempt to configure either RCC or GPIOC, their updates may be
/// reverted. Call this from early in `main` and you're good.
pub unsafe fn init() {
    let rcc = &*device::RCC::ptr();
    let gpioc = &*device::GPIOC::ptr();

    rcc.ahb1enr.modify(|_, w| w.gpiocen().set_bit());

    gpioc.pupdr.modify(|_, w| w
                       .pupdr8().floating()
                       .pupdr9().floating()
                       .pupdr10().floating()
                       .pupdr11().floating()
                       );
    gpioc.ospeedr.modify(|_, w| w
                         .ospeedr8().very_high_speed()
                         .ospeedr9().very_high_speed()
                         .ospeedr10().very_high_speed()
                         .ospeedr11().very_high_speed()
                         );
    gpioc.moder.modify(|_, w| w
                       .moder8().output()
                       .moder9().output()
                       .moder10().output()
                       .moder11().output()
                       )
}

fn write_gpioc_bsrr<F>(op: F)
where F: FnOnce(&mut device::gpioi::bsrr::W) -> &mut device::gpioi::bsrr::W
{
    // Safety: writes to this register are atomic and idempotent.
    unsafe { &*device::GPIOC::ptr() }.bsrr.write(op);
}

pub fn sig_a_set() {
    write_gpioc_bsrr(|w| w.bs8().set_bit());
}

pub fn sig_a_clear() {
    write_gpioc_bsrr(|w| w.br8().set_bit());
}

pub fn sig_b_set() {
    write_gpioc_bsrr(|w| w.bs9().set_bit());
}

pub fn sig_b_clear() {
    write_gpioc_bsrr(|w| w.br9().set_bit());
}

pub fn sig_c_set() {
    write_gpioc_bsrr(|w| w.bs10().set_bit());
}

pub fn sig_c_clear() {
    write_gpioc_bsrr(|w| w.br10().set_bit());
}

pub fn sig_d_set() {
    write_gpioc_bsrr(|w| w.bs11().set_bit());
}

pub fn sig_d_clear() {
    write_gpioc_bsrr(|w| w.br11().set_bit());
}


