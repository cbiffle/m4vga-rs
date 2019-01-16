extern {
    fn copy_words_impl(source: *const u32,
                       dest: *mut u32,
                       count: usize);
}

pub fn copy_words(source: &[u32], dest: &mut [u32]) {
    assert!(source.len() == dest.len());
    unsafe {
        copy_words_impl(source.as_ptr(), dest.as_mut_ptr(), dest.len())
    }
}
