#![no_std]

pub mod armv7m;
pub mod stm32;
mod startup;

mod util;
mod copy_words;
mod arena;
pub mod vga;
#[allow(unused)] // TODO
mod font_10x16;
