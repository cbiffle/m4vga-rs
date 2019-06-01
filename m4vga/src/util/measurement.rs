//! Performance measurement support using GPIOs, compiled out unless the
//! `measurement` feature is set.
//!
//! Because this is intended as a debug facility, this totally circumvents all
//! hardware ownership. If your application is using the measurement output pins
//! (C8-C11) for anything... weird stuff ensues.
//!
//! The mapping of API signals to pins is currently:
//!
//! - A: C8
//! - B: C9
//! - C: C10
//! - D: C11
//!
//! # Simulation
//!
//! Measurement signals are currently disabled in simulation, i.e. the
//! `measurement` feature does nothing. This might change later.

/// Sets up the measurement subsystem.
///
/// Note: if the `measurement` feature is enabled, this will power on GPIOC and
/// configure pins 8-11 as outputs.
///
/// # Safety
///
/// This is safe *as long as* it's not preempted. If interrupts are enabled, and
/// interrupts attempt to configure either RCC or GPIOC, their updates may be
/// reverted. Call this from early in `main` and you're good.
pub unsafe fn init() {
    #[cfg(all(feature = "measurement", target_os = "none"))]
    {
        use stm32f4::stm32f407 as device;
        let rcc = &*device::RCC::ptr();
        let gpioc = &*device::GPIOC::ptr();

        rcc.ahb1enr.modify(|_, w| w.gpiocen().set_bit());

        gpioc.pupdr.modify(|_, w| {
            w.pupdr8()
                .floating()
                .pupdr9()
                .floating()
                .pupdr10()
                .floating()
                .pupdr11()
                .floating()
        });
        gpioc.ospeedr.modify(|_, w| {
            w.ospeedr8()
                .very_high_speed()
                .ospeedr9()
                .very_high_speed()
                .ospeedr10()
                .very_high_speed()
                .ospeedr11()
                .very_high_speed()
        });
        gpioc.moder.modify(|_, w| {
            w.moder8()
                .output()
                .moder9()
                .output()
                .moder10()
                .output()
                .moder11()
                .output()
        })
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "none", feature = "measurement"))] {
        use stm32f4::stm32f407 as device;

        fn write_gpioc_bsrr<F>(op: F)
        where
            F: FnOnce(&mut device::gpioi::bsrr::W) -> &mut device::gpioi::bsrr::W,
        {
            // Safety: writes to this register are atomic and idempotent.
            unsafe { &*device::GPIOC::ptr() }.bsrr.write(op);
        }
    }
}

/// Set measurement signal A.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_a_set() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.bs8().set_bit());
}

/// Clear measurement signal A.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_a_clear() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.br8().set_bit());
}

/// Set measurement signal B.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_b_set() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.bs9().set_bit());
}

/// Clear measurement signal B.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_b_clear() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.br9().set_bit());
}

/// Set measurement signal C.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_c_set() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.bs10().set_bit());
}

/// Clear measurement signal C.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_c_clear() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.br10().set_bit());
}

/// Set measurement signal D.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_d_set() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.bs11().set_bit());
}

/// Clear measurement signal D.
///
/// If the `measurement` feature is not set, this is a no-op.
pub fn sig_d_clear() {
    #[cfg(all(target_os = "none", feature = "measurement"))]
    write_gpioc_bsrr(|w| w.br11().set_bit());
}
