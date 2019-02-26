//! Cortex-M bit-banding support.
//!
//! Some Cortex-M processors have a very unusual feature called "bit-banding."
//! There is a section of the processor address space, the "bit-band alias
//! region," which represents a *magnified view* of another section (called the
//! bit-band "target" section here). The LSB of each word in the alias region
//! maps to a single bit in the target region. This means you can read and write
//! individual bits as though they were words, which can speed up some
//! algorithms.
//!
//! This module provides support for one of the two bit-band regions on the
//! Cortex-M3 and M4 (the SRAM one).

/// Represents a word in memory that aliases a bit in memory, in the bit-band
/// region. These words are unusual because only their LSB is implemented.
///
/// This is basically a `bool`, in that it can only take on the values 0 or 1,
/// but it has the alignment and size of a `u32`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
#[repr(transparent)]
pub struct BandBit(u32);

impl BandBit {
    pub fn set(&mut self) {
        self.0 = 1
    }

    pub fn clear(&mut self) {
        self.0 = 0
    }
}

impl From<BandBit> for bool {
    fn from(b: BandBit) -> Self {
        b.0 != 0
    }
}

impl From<bool> for BandBit {
    fn from(b: bool) -> Self {
        BandBit(b as u32)
    }
}

/// Trait implemented by types where it is safe to mess with their bitwise
/// representation, e.g. integers.
///
/// Types that do *not* meet this criterion: most enums, pointers, bools, most
/// user types.
///
/// # Safety
///
/// To implement this trait safely, you must ensure that the type you're
/// implementing it for *really is valid* for any possible bitwise
/// representation. In practice, this means it's a `#[repr(transparent)]`
/// wrapper around another `BitSafe` type, or is `#[repr(C)]`; other cases are
/// harder to determine.
pub unsafe trait BitSafe: Copy {}

const BB_TARGET_BASE_ADDR: usize = 0x2000_0000;
const BB_TARGET_SIZE_BYTES: usize = 0x0200_0000;
const BB_ALIAS_BASE_ADDR: usize = BB_TARGET_BASE_ADDR + BB_TARGET_SIZE_BYTES;

/// Projects a slice of `BitSafe` values as `BandBit`s representing their
/// individual bits. Changes to the `BandBit` affect the original, and thus the
/// input slice is borrowed while the bit slice exists.
///
/// The input slice must fit entirely within the bit-band target region; the
/// output slice will fit entirely within the bit-band alias region. You can use
/// `is_bit_band_target` to check this.
///
/// # Panics
///
/// If the slice is not within the bit-band target region.
pub fn as_bits_mut<T: BitSafe>(slice: &mut [T]) -> &mut [BandBit] {
    let addr = slice.as_mut_ptr() as usize;
    assert!(
        addr >= BB_TARGET_BASE_ADDR && addr < BB_ALIAS_BASE_ADDR,
        "Base address of slice not in bit-band region: {:08X}",
        addr,
    );
    let size_bytes = slice.len() * core::mem::size_of::<T>();

    let end_addr = addr.checked_add(size_bytes).unwrap();
    assert!(
        end_addr < BB_ALIAS_BASE_ADDR,
        "End address of slice not in bit-band region: {:08X}",
        end_addr
    );

    let alias_addr = BB_ALIAS_BASE_ADDR + (addr - BB_TARGET_BASE_ADDR) * 32;
    let alias_len = (size_bytes * 32) / core::mem::size_of::<T>();

    unsafe {
        core::slice::from_raw_parts_mut(alias_addr as *mut BandBit, alias_len)
    }
}

pub fn is_bit_band_target<T: BitSafe>(slice: &[T]) -> bool {
    let addr = slice.as_ptr() as usize;
    let size_bytes = slice.len() * core::mem::size_of::<T>();
    let end_addr = addr.checked_add(size_bytes).unwrap();

    addr >= BB_TARGET_BASE_ADDR
        && addr < BB_ALIAS_BASE_ADDR
        && end_addr < BB_ALIAS_BASE_ADDR
}

unsafe impl BitSafe for u8 {}
unsafe impl BitSafe for u16 {}
unsafe impl BitSafe for u32 {}
unsafe impl BitSafe for u64 {}
unsafe impl BitSafe for u128 {}
unsafe impl BitSafe for usize {}
unsafe impl BitSafe for i8 {}
unsafe impl BitSafe for i16 {}
unsafe impl BitSafe for i32 {}
unsafe impl BitSafe for i64 {}
unsafe impl BitSafe for i128 {}
unsafe impl BitSafe for isize {}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_bb<T: BitSafe>(addr: usize, count: usize) -> (usize, usize) {
        // The host very likely does not implement bitbanding. We'll test it
        // anyway using some seriously unsafe stuff, but used safely.

        // Mock up some fake data in the bit band target region.
        let fake_target_slice: &mut [T] =
            unsafe { core::slice::from_raw_parts_mut(addr as *mut T, count) };

        // Project it into the bit band region
        let band_slice = as_bits_mut(fake_target_slice);
        // Now smash it into integers so we can't accidentally dereference
        // it.
        (band_slice.as_ptr() as usize, band_slice.len())
    }

    #[test]
    fn basic_u32_projection() {
        let (band_addr, band_len) = project_bb::<u32>(BB_TARGET_BASE_ADDR, 12);

        assert_eq!(band_addr, BB_ALIAS_BASE_ADDR);
        assert_eq!(band_len, 12 * 32);
    }

    #[test]
    fn u32_projection_at_offset() {
        let (band_addr, band_len) =
            project_bb::<u32>(BB_TARGET_BASE_ADDR + 96, 12);

        assert_eq!(band_addr, BB_ALIAS_BASE_ADDR + 96 * 32);
        assert_eq!(band_len, 12 * 32);
    }

    #[test]
    fn basic_u8_projection() {
        let (band_addr, band_len) = project_bb::<u8>(BB_TARGET_BASE_ADDR, 12);

        assert_eq!(band_addr, BB_ALIAS_BASE_ADDR);
        assert_eq!(band_len, 12 * 32);
    }

    #[test]
    fn u8_projection_at_offset() {
        let (band_addr, band_len) =
            project_bb::<u8>(BB_TARGET_BASE_ADDR + 3, 12);

        assert_eq!(band_addr, BB_ALIAS_BASE_ADDR + (3 * 8 * 4));
        assert_eq!(band_len, 12 * 32);
    }

    #[test]
    #[should_panic]
    fn start_addr_too_low() {
        project_bb::<u8>(BB_TARGET_BASE_ADDR - 1, 12);
    }

    #[test]
    #[should_panic]
    fn start_addr_too_high() {
        project_bb::<u8>(BB_ALIAS_BASE_ADDR, 1);
    }

    #[test]
    #[should_panic]
    fn end_addr_too_low() {
        project_bb::<u8>(BB_TARGET_BASE_ADDR - 19, 19);
    }

    #[test]
    #[should_panic]
    fn end_addr_too_high() {
        project_bb::<u8>(BB_TARGET_BASE_ADDR, BB_TARGET_SIZE_BYTES + 1);
    }

}
