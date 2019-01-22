#![no_std]
#![no_main]

#![allow(unused)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

mod util;
mod copy_words;
mod arena;
mod rast;
mod vga;
mod font_10x16;

use cortex_m::asm;
use cortex_m_rt::entry;
use stm32f4;

#[entry]
fn main() -> ! {
    asm::nop(); // To not have main optimize to abort in release mode, remove when you add code

    loop {
        // your code goes here
    }
}
