use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::priority;

/// A specialized framebuffer structure with two features:
///
/// 1. The framebuffer is split into two parts, because the whole thing won't
///    fit into any single RAM.
/// 2. It checks for aliasing on a scanline granularity so rendering can race
///    scanout more aggressively.
pub struct RaceBuffer<R: 'static> {
    segments: [&'static mut [R]; 2],
    write_mark: AtomicUsize,
}

impl<R: 'static> RaceBuffer<R> {
    pub fn new(segments: [&'static mut [R]; 2])
        -> Self
    {
        RaceBuffer {
            segments,
            write_mark: 0.into(),
        }
    }

    pub fn split(&mut self) -> (RaceReader<R>, RaceWriter<R>) {
        (
            RaceReader {
                // Safety: this is an &mut, it cannot be null
                buf: unsafe { NonNull::new_unchecked(self) },
                _life: PhantomData,
            },
            RaceWriter {
                // Safety: this is an &mut, it cannot be null
                buf: unsafe { NonNull::new_unchecked(self) },
                _life: PhantomData,
            },
        )
    }
}

pub struct RaceReader<'a, R: 'static> {
    buf: NonNull<RaceBuffer<R>>,
    _life: PhantomData<&'a ()>,
}

unsafe impl<'a, R: 'static> Send for RaceReader<'a, R> {}

impl<'a, R: 'static> RaceReader<'a, R> {
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    fn boundary(&self) -> usize {
        unsafe { &self.buf.as_ref().segments[0] }.len()
    }

    pub fn take_line<'r, P>(&'r mut self,
                            line_number: usize,
                            _: &'r P) -> &'r R
        where P: priority::InterruptPriority,
    {
        let rendered = self.load_writer_progress();
        let boundary = self.boundary();
        if line_number < rendered {
            let (seg_index, line_number) = if line_number < boundary {
                (0, line_number)
            } else {
                (1, line_number - boundary)
            };

            // Safety: the RaceWriter will only vend mutable references to lines
            // above `rendered`.
            unsafe {
                &self.buf.as_ref().segments[seg_index][line_number]
            }
        } else {
            panic!("tearing: scanout reached {} but rendering only {}",
                   line_number, rendered);
        }
    }
}

pub struct RaceWriter<'a, R: 'static> {
    buf: NonNull<RaceBuffer<R>>,
    _life: PhantomData<&'a ()>,
}

impl<'a, R: 'static> RaceWriter<'a, R> {
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    fn boundary(&self) -> usize {
        unsafe { &self.buf.as_ref().segments[0] }.len()
    }

    pub fn generate_line(&mut self,
                         _: &priority::Thread) -> GenGuard<R> {
        let line_number = self.load_writer_progress();
        let boundary = self.boundary();
        let (seg_index, line_number) = if line_number < boundary {
            (0, line_number)
        } else {
            (1, line_number - boundary)
        };
        let buf = unsafe { self.buf.as_mut() };
        GenGuard {
            counter: &buf.write_mark,
            data: &mut buf.segments[seg_index][line_number],
        }
    }

    pub fn reset(&mut self, _: &priority::Thread) {
        unsafe { self.buf.as_ref() }.write_mark.store(0, Ordering::Relaxed)
    }
}

pub struct GenGuard<'a, R> {
    counter: &'a AtomicUsize,
    data: &'a mut R,
}

impl<'a, R> Drop for GenGuard<'a, R> {
    fn drop(&mut self) {
        self.counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl<'a, R> core::ops::Deref for GenGuard<'a, R> {
    type Target = R;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, R> core::ops::DerefMut for GenGuard<'a, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}
