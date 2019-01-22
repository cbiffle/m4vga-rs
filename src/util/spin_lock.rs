use core::sync::atomic::{Ordering, AtomicBool};
use core::cell::UnsafeCell;

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

impl<T> SpinLock<T> {
    pub const fn new(contents: T) -> Self {
        SpinLock {
            locked: AtomicBool::new(false),
            contents: UnsafeCell::new(contents),
        }
    }
}

impl<T: ?Sized + Send> SpinLock<T> {
    pub fn try_lock(&self) -> Option<SpinLockGuard<T>> {
        if self.locked.swap(true, Ordering::Acquire) {
            // Old value of `true` implies the cell was already locked.
            None
        } else {
            // Old value of `false` means we have locked the cell!
            //
            // We can safely observe the contents of the cell now, because no
            // other thread could have observed the same false->true transition.
            // We return a single mutable reference. If it is dropped, the cell
            // will unlock, but not before -- until then, all attempts to
            // `try_lock` will fail.
            Some(SpinLockGuard {
                locked: &self.locked,
                contents: unsafe { &mut *self.contents.get() },
            })
        }
    }
}

#[must_use = "if dropped, the spinlock will immediately unlock"]
#[derive(Debug)]
pub struct SpinLockGuard<'a, T: ?Sized> {
    locked: &'a AtomicBool,
    contents: &'a mut T,
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

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        let old = self.locked.swap(false, Ordering::Release);
        debug_assert!(old)
    }
}
