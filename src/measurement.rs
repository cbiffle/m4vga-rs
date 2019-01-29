//! Performance measurement support using GPIOs.
//!
//! This totally circumvents all hardware ownership.

use stm32f4::stm32f407 as device;

pub fn init() {
    let rcc = unsafe { &*device::RCC::ptr() };
    let gpioc = unsafe { &*device::GPIOC::ptr() };

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

pub fn sig_a_set() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.bs8().set_bit());
}

pub fn sig_a_clear() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.br8().set_bit());
}

pub fn sig_b_set() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.bs9().set_bit());
}

pub fn sig_b_clear() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.br9().set_bit());
}

pub fn sig_c_set() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.bs10().set_bit());
}

pub fn sig_c_clear() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.br10().set_bit());
}

pub fn sig_d_set() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.bs11().set_bit());
}

pub fn sig_d_clear() {
    let gpioc = unsafe { &*device::GPIOC::ptr() };

    gpioc.bsrr.write(|w| w.br11().set_bit());
}


