//! Augmented ARMv7M operations
//!
//! # Interrupt management
//!
//! The `enable_irq`, `disable_irq`, and `clear_pending_irq` functions provide
//! enhanced atomic interrupt status management. The equivalent operations from
//! the `cortex_m` crate do not guarantee atomicity. For example, when disabling
//! an interrupt, the interrupt *can still fire* after `disable` returns,
//! because `disable` does not use the correct memory barrier instructions.
//! `disable_irq` fixes this, and so for the other functions in this module.
//!
//! The methods used are derived from the ARM document *ARM Cortex-M Programming
//! Guide to Memory Barrier Instructions*.

/// Enables an interrupt with enhanced guarantees: the interrupt is enabled by
/// the time the function returns. This means that, if the interrupt is pended,
/// priority masks and caller interrupt priority allowing, the ISR will have had
/// an opportunity to execute by the time this function returns.
///
/// If the interrupt was already enabled, this is a no-op.
pub fn enable_irq(nvic: &mut cortex_m::peripheral::NVIC,
                  i: impl cortex_m::interrupt::Nr) {
    nvic.enable(i);
    cortex_m::asm::dmb();
    cortex_m::asm::isb();
}

/// Disables an interrupt with enhanced guarantees: the interrupt is disabled by
/// the time the function returns. This means that, starting at the first
/// instruction after a call to `disable_irq`, execution cannot be preempted by
/// this interrupt.
///
/// If the interrupt was already disabled, this is a no-op.
pub fn disable_irq(nvic: &mut cortex_m::peripheral::NVIC,
                   i: impl cortex_m::interrupt::Nr) {
    nvic.disable(i);
    cortex_m::asm::dmb();
    cortex_m::asm::isb();
}

/// Ensures that an interrupt is not pending. If hardware continues generating
/// IRQs, the interrupt may immediately start pending again.
pub fn clear_pending_irq(i: impl cortex_m::interrupt::Nr) {
    cortex_m::peripheral::NVIC::unpend(i);
    // These barriers are arguably overkill, but *shrug*
    cortex_m::asm::dmb();
    cortex_m::asm::isb();
}
