//! Type-level representation of execution priorities.
//!
//! All the priority types are zero-sized tokens. When the driver invokes a user
//! interrupt hook, it will pull an appropriate priority token out of thin air
//! and hand it to the hook. This gives the hook the ability to take certain
//! actions that would otherwise be off-limits.

use core::marker::PhantomData;

// Marker type used to cause things to stop being Sync/Send.
type NotSyncOrSend = PhantomData<*mut ()>;

/// Lowest priority driver interrupt, used for rasterization.
#[derive(Copy, Clone)]
pub struct I0(NotSyncOrSend);
/// Highest priority driver interrupt, used for hblank.
#[derive(Copy, Clone)]
pub struct I1(NotSyncOrSend);
/// Thread mode execution occurs outside any interrupt handler.
#[derive(Copy, Clone)]
pub struct Thread(NotSyncOrSend);

impl I0 {
    pub(crate) unsafe fn new() -> Self {
        I0(PhantomData)
    }
}

/*
TODO: re-enable when we start supporting hblank hooks
impl I1 {
    pub(crate) unsafe fn new() -> Self {
        I1(PhantomData)
    }
}
*/

impl Thread {
    pub(crate) unsafe fn new() -> Self {
        Thread(PhantomData)
    }
}

#[cfg(target_os = "none")]
impl Thread {
    /// Returns a `Thread` token only if called from thread priority.
    pub fn new_checked() -> Option<Self> {
        // TODO: read this from xPSR if cortex_m starts providing it. It's
        // currently gated by inline_asm, but the bits we're after aren't frame
        // dependent so out-of-line would be fine.

        // Safety: reads of the ICSR are safe.
        let icsr = unsafe { &(*cortex_m::peripheral::SCB::ptr()).icsr }.read();
        if icsr & 0xFF == 0 {
            Some(unsafe { Self::new() })
        } else {
            None
        }
    }
}

/// Indicates that a type represents an interrupt priority level.
pub trait InterruptPriority {}

impl InterruptPriority for I0 {}
impl InterruptPriority for I1 {}
