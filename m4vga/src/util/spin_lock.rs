//! Bare metal spinlocks using atomic memory operations.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Protects a `T` using a spinlock to ensure that it can't be accessed
/// concurrently or reentrantly.
///
/// `SpinLock` is a lot like `Mutex` from the standard library, but in a greatly
/// simplified form intended for bare metal use. In particular, `SpinLock`
/// cannot block threads in the traditional polite manner; instead, all locking
/// is best-effort and may fail. (If you really need to get a lock: spin.)
///
/// This is intended for sharing resources between application code and
/// interrupt handlers, but works fine between application threads, too.
#[derive(Debug)]
pub struct SpinLock<T: ?Sized> {
    locked: AtomicBool,
    contents: UnsafeCell<T>,
}

unsafe impl<T: Send + ?Sized> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(contents: T) -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
            contents: UnsafeCell::new(contents),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SpinLockError {
    Contended,
}

impl<T: ?Sized + Send> SpinLock<T> {
    pub fn try_lock(&self) -> Result<SpinLockGuard<T>, SpinLockError> {
        if self.locked.swap(true, Ordering::Acquire) {
            // Old value of `true` implies the cell was already locked.
            Err(SpinLockError::Contended)
        } else {
            // Old value of `false` means we have locked the cell!
            //
            // We can safely observe the contents of the cell now, because no
            // other thread could have observed the same false->true transition.
            // We return a single mutable reference. If it is dropped, the cell
            // will unlock, but not before -- until then, all attempts to
            // `try_lock` will fail.
            Ok(SpinLockGuard {
                locked: LockBorrow(&self.locked),
                // Safety: we've locked, so we can generate an exclusive
                // reference.
                contents: unsafe { &mut *self.contents.get() },
            })
        }
    }

    pub fn lock(&self) -> SpinLockGuard<T> {
        loop {
            match self.try_lock() {
                Ok(guard) => return guard,
                Err(_) => continue,
            }
        }
    }
}

#[must_use = "if dropped, the spinlock will immediately unlock"]
#[derive(Debug)]
pub struct SpinLockGuard<'a, T: ?Sized> {
    locked: LockBorrow<'a>,
    contents: &'a mut T,
}

/// A reference to the `SpinLock` lock flag that releases it when dropped. This
/// type is distinct from `SpinLockGuard` so that the latter can be consumed and
/// reconstructed by `map` -- something that is not allowed for `Drop` types.
#[derive(Debug)]
struct LockBorrow<'a>(&'a AtomicBool);

impl<'a, T: ?Sized> SpinLockGuard<'a, T> {
    /// Replaces a guard of `T` with a guard of some portion of `T`. This is
    /// essentially a projection operation. The original guard is lost.
    pub fn map<U>(
        orig: SpinLockGuard<'a, T>,
        f: impl FnOnce(&mut T) -> &mut U,
    ) -> SpinLockGuard<'a, U> {
        let SpinLockGuard { locked, contents } = orig;
        SpinLockGuard {
            locked,
            contents: f(contents),
        }
    }
}

impl<'a, T: ?Sized> core::ops::Deref for SpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.contents
    }
}

impl<'a, T: ?Sized> core::ops::DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.contents
    }
}

impl<'a> Drop for LockBorrow<'a> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
