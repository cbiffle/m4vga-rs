//! Read-Write Spinlocks.
//!
//! A `ReadWriteLock` is like a `SpinLock`, but with separate read and write
//! modes. One thread can lock for mutation at any given time, *or* any number
//! of threads can lock for reading, but not both.
//!
//! The implementation is based closely on `RefCell` but using atomic memory
//! updates.

use core::sync::atomic::{AtomicIsize, Ordering};
use core::cell::UnsafeCell;

pub struct ReadWriteLock<T: ?Sized> {
    status: AtomicIsize,
    value: UnsafeCell<T>,
}

unsafe impl<T: ?Sized> Sync for ReadWriteLock<T> {}

impl<T> ReadWriteLock<T> {
    pub const fn new(value: T) -> Self {
        ReadWriteLock {
            status: AtomicIsize::new(0),
            value: UnsafeCell::new(value),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum TryLockError {
    /// A conflicting type of guard exists for the lock, so the operation will
    /// not succeed until that guard is released.
    Unavailable,
    /// The operation failed due to a preemption / race, but will likely succeed
    /// if retried.
    Race,
}

#[derive(Copy, Clone, Debug)]
pub enum TryLockMutError {
    /// A conflicting type of guard exists for the lock, so the operation will
    /// not succeed until that guard is released.
    Unavailable,
}

impl<T: ?Sized> ReadWriteLock<T> {
    pub fn try_lock(&self) -> Result<Guard<T>, TryLockError> {
        // Observe our current status. If it does not allow a read guard to be
        // created, abort.
        let status = self.status.load(Ordering::Acquire);
        if writing(status) || status == isize::max_value() {
            return Err(TryLockError::Unavailable)
        }

        // Attempt to atomically increment the status to record a new read
        // guard. If this succeeds, we've locked it. If this fails, some
        // higher-priority task has come through and locked it during the last
        // few instructions.
        self.status.compare_exchange(
            status,
            status + 1,
            Ordering::Release,
            Ordering::Relaxed
        ).map(|_| Guard {
            borrow: Borrow(&self.status),
            value: unsafe { &*self.value.get() },
        }).map_err(|s| if writing(s) || s == isize::max_value() {
            TryLockError::Unavailable
        } else {
            TryLockError::Race
        })
    }

    /// Locks `self` for reading, spinning forever if necessary.
    pub fn lock(&self) -> Guard<T> {
        loop {
            match self.try_lock() {
                Ok(guard) => return guard,
                Err(_) => continue,
            }
        }
    }

    pub fn try_lock_mut(&self) -> Result<GuardMut<T>, TryLockMutError> {
        let status = self.status.load(Ordering::Acquire);
        if status != 0 {
            return Err(TryLockMutError::Unavailable)
        }

        self.status.compare_exchange(
            status,
            status - 1,
            Ordering::Release,
            Ordering::Relaxed
        ).map(|_| GuardMut {
            borrow: BorrowMut(&self.status),
            value: unsafe { &mut *self.value.get() },
        }).map_err(|_| TryLockMutError::Unavailable)
    }

    /// Locks `self` for mutation, spinning forever if necessary.
    pub fn lock_mut(&self) -> GuardMut<T> {
        loop {
            match self.try_lock_mut() {
                Ok(guard) => return guard,
                Err(_) => continue,
            }
        }
    }

}

/// Smart pointer type representing a read lock on a `ReadWriteLock`.
pub struct Guard<'a, T: ?Sized> {
    borrow: Borrow<'a>,
    value: &'a T,
}

impl<'a, T: ?Sized> Guard<'a, T> {
    pub fn map<U>(orig: Guard<'a, T>,
                  f: impl FnOnce(&T) -> &U)
        -> Guard<'a, U>
    {
        let Guard { borrow, value } = orig;
        Guard { borrow, value: f(value) }
    }
}

impl<'a, T: ?Sized> core::ops::Deref for Guard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

struct Borrow<'a>(&'a AtomicIsize);

impl<'a> Drop for Borrow<'a> {
    fn drop(&mut self) {
        let prev = self.0.fetch_sub(1, Ordering::Release);
        debug_assert!(reading(prev))
    }
}

pub struct GuardMut<'a, T: ?Sized> {
    borrow: BorrowMut<'a>,
    value: &'a mut T,
}

impl<'a, T: ?Sized> GuardMut<'a, T> {
    pub fn map<U>(orig: GuardMut<'a, T>,
                  f: impl FnOnce(&mut T) -> &mut U)
        -> GuardMut<'a, U>
    {
        let GuardMut { borrow, value } = orig;
        GuardMut { borrow, value: f(value) }
    }
}

impl<'a, T: ?Sized> core::ops::Deref for GuardMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T: ?Sized> core::ops::DerefMut for GuardMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

struct BorrowMut<'a>(&'a AtomicIsize);

impl<'a> Drop for BorrowMut<'a> {
    fn drop(&mut self) {
        let prev = self.0.fetch_add(1, Ordering::Release);
        debug_assert!(writing(prev))
    }
}

fn writing(x: isize) -> bool { x < 0 }
fn reading(x: isize) -> bool { x > 0 }
