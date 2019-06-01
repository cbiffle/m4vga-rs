//! Utility code; candidates for factoring out.

cfg_if::cfg_if! {
    if #[cfg(target_os = "none")] {
        pub mod armv7m;
        pub mod startup;
        pub mod stm32;
    }
}

pub mod copy_words;
pub mod measurement;
pub mod race_buf;
pub mod rw_lock;
pub mod spin_lock;
