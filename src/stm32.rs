//! Augmented STM32 operations

use stm32f4::stm32f407 as device;

/// For reasons I cannot fathom, the stm32 `Interrupt` type is neither `Copy` or
/// `Clone`.
///
/// There I fixed it.
pub fn copy_interrupt(i: &device::Interrupt) -> device::Interrupt {
    // hold my beer
    unsafe { core::ptr::read(i) }
}
