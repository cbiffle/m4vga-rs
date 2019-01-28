#![allow(unused)] // TODO: this whole module is stubbed out

use core::cell::Cell;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::marker::PhantomData;

pub struct Arena {
    base: *mut u8,
    limit: *mut u8,
    next: Cell<*mut u8>,
}

impl Arena {
    pub unsafe fn from_pointers(base: *mut u8, limit: *mut u8) -> Self {
        assert!(limit >= base);
        Arena {
            base,
            limit,
            next: Cell::new(base),
        }
    }

    pub fn reset(&mut self) {
        self.next.set(self.base)
    }

    pub fn alloc_default<T: Default>(&self)
        -> Option<Box<T>>
    {
        unimplemented!()
    }

    pub fn alloc<T>(&self, value: T)
        -> Option<Box<T>>
    {
        unimplemented!()
    }

    pub fn alloc_slice_default<T: Default>(&self, size: usize)
        -> Option<Box<[T]>>
    {
        unimplemented!()
    }

    pub fn alloc_slice_copy<T: Copy>(&self, src: &[T])
        -> Option<Box<[T]>>
    {
        unimplemented!()
    }

}

#[derive(Debug)]
pub struct Box<'arena, T: ?Sized>(NonNull<T>, PhantomData<&'arena mut T>);

unsafe impl<'arena, T: ?Sized + Sync> Sync for Box<'arena, T> {}

impl<'arena, T: ?Sized> Deref for Box<'arena, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<'arena, T: ?Sized> DerefMut for Box<'arena, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

impl<'arena, T: ?Sized> Drop for Box<'arena, T> {
    fn drop(&mut self) {
        unsafe {
            ptr::drop_in_place(self.0.as_ptr())
        }
    }
}
