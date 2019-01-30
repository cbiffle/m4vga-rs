//! The "shock absorber" ISR.
//!
//! This exists to minimize jitter in the latency between our start-of-active
//! timer going off, and the hstate ISR executing. In the wild, this latency is
//! affected by...
//!
//! 1. Wait-stated bus transactions, particularly fetches from Flash.
//! 2. Multi-word burst transactions, such as unaligned accesses or bitband
//!    writes.
//! 3. Tail chaining -- if the timer goes off near the end of another ISR, the
//!    processor will jump directly from one to the other, *reducing* latency.
//!
//! We work around this with the shock absorber. Its job is to fire a few cycles
//! before we expect the actual interrupt, and idle the CPU. This ensures that
//! the CPU and bus are quiet when the interrupt fires.

use stm32f4::stm32f407 as device;
use crate::acquire_hw;
use crate::util::spin_lock::SpinLock;

pub static SHOCK_TIMER: SpinLock<Option<device::TIM3>> = SpinLock::new(None);

pub const SHOCK_ABSORBER_SHIFT_CYCLES: u32 = 20;

/// Entry point for the shock absorber ISR.
///
/// Note: this is `etl_stm32f4xx_tim3_handler` in the C++.
pub fn shock_absorber_isr() {
    // Acknowledge IRQ so it doesn't re-occur.
    acquire_hw(&SHOCK_TIMER)
        .sr.modify(|_, w| w.cc2if().clear_bit());
    // Idle the CPU until an interrupt arrives.
    cortex_m::asm::wfi()
}


