//! A very fast routine for moving data around.

/// Copies words (type `u32`) from `source` to `dest` -- really, really quickly.
///
/// This uses an optimized assembly language copy routine that asymptotically
/// approaches 2 CPU cycles per word transferred, as the transfer gets longer.
/// At the buffer sizes we use in this library, it works out to about 2.5 cyc/w
/// empirically.
///
/// This is nearly twice as fast as the DMA controller. If you've got a faster
/// technique I would love to borrow it. ;-)
///
/// # Panics
///
/// If the slices are not the same length.
pub fn copy_words(source: &[u32], dest: &mut [u32]) {
    // In the common case where source and dest are visibly the same length
    // (because they're both sliced using the same bounds) this check reliably
    // dissolves.
    assert!(source.len() == dest.len());

    // Safety: if they're the same len, we'll remain in-bounds.
    unsafe { copy_words_impl(source.as_ptr(), dest.as_mut_ptr(), dest.len()) }
}

extern "C" {
    fn copy_words_impl(source: *const u32, dest: *mut u32, count: usize);
}
