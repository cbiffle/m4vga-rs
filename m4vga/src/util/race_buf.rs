use core::borrow::Borrow;
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
///
/// # Parameters
///
/// `S` is the segment type, which must implement `Borrow<[R]>` and
/// `AsMut<[R]>`. In contexts where storage is allocated statically, this is
/// probably `&'static mut R`.
///
/// `R` is the row (scanline) type, typically an array.
///
/// # Use of priority tokens
///
/// Users of a `RaceBuffer` must provide *priority tokens* to most API calls.
/// Specifically,
///
/// - Scanlines can only be *written* from thread mode, outside of any
///   interrupt.
/// - Scanlines can only be *read* from interrupt handlers.
/// - A scanline accessed in an interrupt handler cannot be stored outside the
///   execution of that interrupt handler.
///
/// This simplifies the implementation, by ensuring that read operations cannot
/// be preempted by write operations.
pub struct RaceBuffer<S, R: 'static> {
    segments: [S; 2],
    write_mark: AtomicUsize,
    _marker: PhantomData<&'static mut R>,
}

impl<S, R: 'static> RaceBuffer<S, R> {
    /// Creates a `RaceBuffer` from two static bands of rows.
    ///
    /// The bands need not be the same length.
    pub fn new(segments: [S; 2]) -> Self {
        RaceBuffer {
            segments,
            write_mark: 0.into(),
            _marker: PhantomData,
        }
    }

    /// Generates a `RaceReader` and `RaceWriter` for this buffer, which can
    /// then be distributed to the renderer and rasterizer.
    pub fn split(&mut self) -> (RaceReader<S, R>, RaceWriter<S, R>) {
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

/// Pulls rendered scanlines from a `RaceBuffer`.
pub struct RaceReader<'a, S, R: 'static> {
    buf: NonNull<RaceBuffer<S, R>>,
    _life: PhantomData<&'a ()>,
}

unsafe impl<'a, S, R: 'static> Send for RaceReader<'a, S, R> {}

impl<'a, S, R: 'static> RaceReader<'a, S, R>
where
    S: Borrow<[R]>,
{
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    fn boundary(&self) -> usize {
        unsafe { &self.buf.as_ref().segments[0] }.borrow().len()
    }

    /// Gets a reference to a scanline, identified by `line_number`.
    ///
    /// If the renderer has not finished with this scanline, we have found a
    /// dynamic data race; `take_line` will `panic`.
    ///
    /// The caller is required to provide an interrupt priority token `P`,
    /// proving that they are calling from interrupt context. This ensures that
    /// this operation is atomic with respect to the `RaceWriter`, which must be
    /// used *outside* an interrupt.
    pub fn take_line<'r, P>(&'r mut self, line_number: usize, _: &'r P) -> &'r R
    where
        P: priority::InterruptPriority,
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
                &self.buf.as_ref().segments[seg_index].borrow()[line_number]
            }
        } else {
            panic!(
                "tearing: scanout reached {} but rendering only {}",
                line_number, rendered
            );
        }
    }
}

/// Vends unrendered scanlines and tracks when they're completed.
pub struct RaceWriter<'a, S, R: 'static> {
    buf: NonNull<RaceBuffer<S, R>>,
    _life: PhantomData<&'a ()>,
}

impl<'a, S, R: 'static> RaceWriter<'a, S, R>
where
    S: Borrow<[R]> + AsMut<[R]>,
{
    fn load_writer_progress(&self) -> usize {
        unsafe { &self.buf.as_ref().write_mark }.load(Ordering::Relaxed)
    }

    fn boundary(&self) -> usize {
        unsafe { &self.buf.as_ref().segments[0] }.borrow().len()
    }

    /// Gets the next scanline for rendering.
    ///
    /// The scanline is returned as a `GenGuard` smart pointer. This works like
    /// a `&mut`, and will mark the scanline as completed when it's dropped.
    ///
    /// The caller is required to provide a `Thread` priority token,
    /// demonstrating that they are *not* attempting to use this API from an
    /// ISR. This allows the implementation to be somewhat simpler.
    ///
    /// # Panics
    ///
    /// If the next scanline would run off the end of the final framebuffer
    /// band.
    pub fn generate_line(&mut self, _: &priority::Thread) -> GenGuard<R> {
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
            data: &mut buf.segments[seg_index].as_mut()[line_number],
            _not_sync_send: PhantomData,
        }
    }

    /// Resets the buffer to empty to begin a new frame.
    ///
    /// After this call,
    ///
    /// - The next line handed out by `generate_line` will be scanline zero.
    /// - No lines will be available to the `RaceReader`.
    ///
    /// The caller is required to provide a `Thread` priority token, which
    /// proves that this call is not executing concurrently with the
    /// `RaceReader` (which can only be used from ISRs). This simplifies the
    /// implementation.
    pub fn reset(&mut self, _: &priority::Thread) {
        unsafe { self.buf.as_ref() }
            .write_mark
            .store(0, Ordering::Relaxed)
    }
}

pub struct GenGuard<'a, R> {
    counter: &'a AtomicUsize,
    data: &'a mut R,
    /// Conservatively prevent this smart pointer from being moved into an ISR,
    /// because I haven't thought through the implications of doing so.
    _not_sync_send: PhantomData<*mut R>,
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
