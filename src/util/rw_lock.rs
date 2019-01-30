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
    /// Attempts to lock `self` for reading.
    ///
    /// If this succeeds, the returned `Guard<T>` can be used to access the
    /// contents of the lock, with the guarantee that they will not be mutated.
    ///
    /// This can fail for two reasons, encoded in the values of `TryLockError`.
    ///
    /// 1. If some other code holds a `GuardMut<T>` on `self`, the contents may
    ///    change and so we can't get our lock. In this case, the error is
    ///    `TryLockError::Unavailable`.
    ///
    /// 2. If a higher-priority task (i.e. an ISR) preempts us and takes out a
    ///    lock while this function is executing, we have to try again. In this
    ///    case, the error is `TryLockError::Race`.
    ///
    /// These cases are distinguished because, if you're confident that code is
    /// not going to get preempted, calling `try_lock` and treating the
    /// (allegedly impossible) `Race` case as an error is significantly cheaper
    /// than including a retry loop.
    pub fn try_lock(&self) -> Result<Guard<T>, TryLockError> {
        // Observe our current status. If it does not allow a read guard to be
        // created, abort.
        let status = self.status.load(Ordering::Acquire);
        if read_unavail(status) {
            return Err(TryLockError::Unavailable)
        }

        // Attempt to atomically increment the status to record a new read
        // guard. If this succeeds, we've locked it. If this fails, some
        // higher-priority task has come through during the last few
        // instructions -- indicate a race.
        self.status.compare_exchange_weak(
            status,
            status + 1,
            Ordering::Release,
            Ordering::Relaxed
        ).map(|_| Guard {
            borrow: Borrow(&self.status),
            value: unsafe { &*self.value.get() },
        }).map_err(|_| TryLockError::Race)
    }

    /// Locks `self` for reading, retrying on races but failing on actual
    /// contention.
    pub fn lock_uncontended(&self) -> Result<Guard<T>, ()> {
        loop {
            match self.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Race) => continue,
                Err(_) => return Err(())
            }
        }
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

/// Checks if we *can't* take out a read lock given a status word. We can't take
/// out a read lock if:
///
/// 1. The status is negative (i.e. write locks exist).
/// 2. The status would overflow -- it is `isize::max_value()`.
///
/// This implementation exploits the fact that `isize::max_value()` and the
/// negative numbers form a single contiguous range, if you interpret them as
/// unsigned. This shaves several instructions off the test.
fn read_unavail(x: isize) -> bool {
    x as usize >= isize::max_value() as usize
}
