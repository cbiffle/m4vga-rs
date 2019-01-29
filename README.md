# m4vga-rs

I'm gradually porting [my C++ graphics demos][1] into [Rust][2].

## Motivation

My demos are not programs you run on your PC. They run on a Cortex-M4
microcontroller that has no video hardware. How do they produce video, then? By
careful programming.

Everything's produced by software, and I've got 4 CPU cycles per pixel -- and
there isn't enough RAM for a framebuffer. Stringent requirements like this put
us squarely in the traditional domain of C and assembly language. So why am I
writing this in a memory-safe functional-ish programming language?

This exercise is half technical, half ideological.

- On the technical side, I suspected that Rust was finally up to the task, and
  wanted to see how much of the demos and video driver could be expressed in
  safe code.

- On the ideological side, I think writing C++ in 2019 is negligent from a
  security/reliability perspective, and so I'm porting my remaining codebases
  away from the language.

## Current Status and Comparisons

Basic functionality is working: there are demos in [`src/bin`][3].

The Rust demos wind up being substantially simpler (to write) than the C++
demos, because of some API changes I was able to make thanks to Rust's ownership
rules. Inspired by [scoped threads in crossbeam][5], we can safely pass a
closure to be called from ISRs that *shares stack-allocated state* with the main
program loop. It is possible to do this in C++, but you really shouldn't because
the result would be covered in [footguns][8]. Concretely: compare the
`xor_pattern` demo [in Rust][9] and [in C++][10].

The Rust code currently has a larger Flash footprint: 20kiB vs 4.5kiB. But this
is misleading. From inspection of the binary, the difference is almost entirely
due to `panic!` messages: string literals and formatting support. In C++ I used
tricks to remove these from the binary; the same tricks could be applied to
Rust. (Currently I get nice panic reports through the debugger's ITM interface,
which I'm enjoying.)

There is relatively little unsafe code in the graphics driver -- and much of the
current unsafe code is due to API bugs in the upstream `stm32` crate I rely on.
In C++ I rewrote the entire world to get around bugs in vendor libraries; the
same method could be applied here to reduce `unsafe` substantially.

I had to jump through some hoops to achieve this. In particular, to prove the
absence of data races in peripheral interactions, the peripherals are passed
between thread-mode and ISRs using spinlocks. This is more code on the page, and
a bit more code in the binary. (Because peripherals are modeled as [zero sized
types][4], it's not actually that much more work at runtime.)

On the other hand: *I can prove the absence of data races with my ISRs.* Getting
this to work revealed a bunch of hidden data races in the C++ codebase, all of
which are fixed in the Rust.

## Building it

You will need:

- Rust 1.32 (or possibly later)
- OpenOCD 0.10
- ARM GDB 8.1 (ish)

Install a cross-compiler, if you haven't already:

```shell
$ rustup target add thumbv7em-none-eabihf
```

And build it:

```shell
$ cargo build --release
```

(Debug builds also work, but release builds are much smaller.)

As for actually using it, I'm going to defer to [the Rust Embedded book][6] and
[my C++ instructions][7].

[1]: https://github.com/cbiffle/m4vgalib-demos
[2]: https://rust-lang.org
[3]: src/bin
[4]: https://doc.rust-lang.org/nomicon/exotic-sizes.html
[5]: https://docs.rs/crossbeam/0.7.1/crossbeam/thread/
[6]: https://rust-embedded.github.io/book
[7]: https://github.com/cbiffle/m4vgalib-demos/blob/master/README.mkdn#connections
[8]: https://en.wiktionary.org/wiki/footgun
[9]: src/bin/xor_pattern/main.rs
[10]: https://github.com/cbiffle/m4vgalib-demos/tree/master/demo/xor_pattern
