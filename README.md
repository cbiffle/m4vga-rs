# m4vga-rs

The `m4vga` crate provides SuperVGA-quality 60fps graphics from the STM32F407
microcontroller. The observant reader will note that the STM32F407 has no video
hardware, so how does `m4vga` get video out of it?

*Magic.*

This is a rewrite of the C++ library [`m4vgalib`][11], plus ports of my
[collection of `m4vgalib` demos][1]. It is still very much a work in progress.

## Why this is interesting

Mostly because it's really hard. I've got four CPU cycles *per pixel* to work
with, and any variation in timing will corrupt the display.

## Building it

You will need an STM32F407-based board to run this on; I use the
STM32F4-Discovery because it's *really cheap.* Hook it up to a VGA connector
according to [my instructions for C++][7].

I recommend following the setup chapters from the [Rust Embedded][6] book. In
particular, you need to have [Rust][2] and you need to make Rust aware of the
cross compilation target we're using here:

```shell
$ rustup target add thumbv7em-none-eabihf
```

Now you should be able to compile everything by entering:

```shell
$ cargo build --release
```

This will deposit several demo binaries in
`target/thumbv7em-none-eabihf/release/`.

And if you start `openocd` (tested with version 0.10) in this directory, it will
pick up the `openocd.cfg` file automagically, and (from a separate terminal) you
can flash one of the demos by typing:

```shell
$ cargo run --release --bin horiz_tp
```

(All of this is tested only on Linux.)

## Motivation

I wrote `m4vgalib` and the attendant demos as an exercise in hard-real-time
programming. I wanted to see how far I could push C++, so I avoided assembly
language except for certain key routines.

Now I want to see how far I can push Rust -- specifically, safe Rust. See,
despite having written C++ as my day job for many years, I'm aware that most of
the common security/reliability bugs we see in software today are a result of
flaws in the C and C++ languages. Rust fixes essentially all of them. So I've
been keeping an eye on it for a while now. More reliable software with less
work? Yes please.

My graphics demos are so resource-constrained, and so timing-sensitive, that
they fall squarely into the traditional domain of assembly and C -- a domain
that has been well-defended for years. Can I build the same thing using a
memory-safe language? Could I use the additional brain-space that I'm *not*
spending on remembering C++'s initialization order rules (for example) to make a
better system with more features?

The answer so far seems to be yes.

## Musings

Read on for my summary thoughts on the port.

### On `nostd`

The *single best* thing about Rust for bare-metal programming is the `nostd`
ecosystem.

C++ has a monolithic standard library with an amazing set of cool stuff in it.
However, the library is written for a "normal" C++ execution environment, which
for our purposes means two things:

1. There is a heap, and it's okay to allocate/free whenever.
2. Exceptions are turned on.

In most high-reliability, hard-real-time embedded environments, neither
statement is true. We eschew heaps because of the potential for exhaustion and
fragmentation; we eschew exceptions because the performance of unwinding code is
unpredictable and vendor-dependent. (And also for religious reasons in some
cases, but not in my case.)

Now, there are *parts* of the C++ standard library that you can use safely in a
no-heap, no-exceptions environment. Header-only libraries like `type_traits` are
probably fine. Simple primitive types like `atomic` are ... probably fine.

I keep saying "probably" because the C++ standard does not specify (reliably)
which operations are guaranteed not to throw or allocate. (Though they're
gradually working on the former.) As a result, it's really easy to
*accidentally* introduce a heap dependency, or an API that can't indicate
failure when exceptions are disabled, *by accident.*

The Rust standard library is also large and varied, but -- critically -- it's
divided into two parts, `std` and `core`. `std` is like the C++ equivalent.
`core` underpins `std` and *does not allocate or unwind the stack*.

This is a tiny design decision with huge implications:

1. By setting the `#[nostd]` attribute on a crate, you're opting out of the
   default dependency on `std`. Any attempt to use a feature from `std` is now a
   compile time error -- but you can still use `core`.

2. You can trust *other* crates to do the same, so you can use third-party
   libraries safely. Many crates are either `nostd` by default, or can have it
   enabled at build time.

3. `core` is small enough that porting it to a new platform is easy --
   significantly easier, in fact, than porting `newlib`, the standard-bearer for
   portable embedded C libraries.

For `m4vgalib` I rewrote almost all my dependencies to get a system that
wouldn't throw or allocate. In Rust, I don't have to do that!


### On API design

Rust's ownership rules produce a sort of bizarro-world of API design.

- Some (uncommon, but reasonable) API designs won't make it past the borrow
  checker. (In nearly every case, these are APIs that would have sported large
  "how to use safely" comments in other languaes.)

- Some API patterns that are grossly unsafe or unwise in other languages are
  routine in Rust because of e.g. lifetime checking.

As an example of the latter: it is common, and safe, to loan out stack-allocated
data structures *to other threads* with no runtime checks. (See: [scoped threads
in crossbeam][5].) I implemented the same thing for loaning data to ISRs in
`m4vga`, and it *changed everything.* Most of my abstractions dissolved
immediately.

Concrete example: `m4vgalib` (C++) lets applications provide custom
*rasterizers* that are invoked to generate pixel data. They are subclasses of
the `Rasterizer` library class, which sports a single virtual member function
(called -- wait for it -- `rasterize`). You register a `Rasterizer` with the
driver by putting a pointer to it into a table. Once registered, the
`Rasterizer` will have its `rasterize` function called from an interrupt handler
once per scanline.

You, the application author, have some responsibilities to use this API safely:

1. The `Rasterizer` object needs to hang around until you're done with it -- it
   might be `static` or it might be allocated from a carefully-managed arena.
   Otherwise, the ISR will try to use dangling pointers, and that's bad.

2. While the `Rasterizer` object is accessible by the ISR, it can be entered at
   *basically any time* by code running at interrupt priority. Because we can't
   disable interrupts without distorting the display, this means that your
   application code that shares state with the `Rasterizer` (say, a drawing
   loop) needs to be written carefully to avoid data races. Commonly, this means
   double-buffering with a `std::atomic<bool>` flip signal...and some squinting
   and care to avoid accessing other state incorrectly.

3. Before disposing of the `Rasterizer` object, you must un-register it with the
   driver. This prevents an ISR from dereferencing its dangling pointer, which,
   again, would be bad.

I recreated the C++ API verbatim in Rust, and immediately started to run into
ownership issues.

- "Okay, here's a `Raster` trait and an implementation thereof."
- "Hm. How can I pass a reference to this to an interrupt handler? The rules
  around `static` state seem to basically prevent that."
- "Okay, I've built an abstraction (`IRef`) to enable that; only it turns out I
  didn't actually want to *give* the `Rasterizer` to the ISR, because I want to
  draw into its background buffer and make other state changes."
- "If I split the `Rasterizer` into two parts and give *one* to the ISR, how do
  I communicate between them when it comes time to flip buffers? Do I need to
  pepper my code with `Cell` to do interior mutability?"
- "This feels a lot like the problem that [scoped threads][5] solves."
- "..."

Taking inspiration from crossbeam, I added code to loan a *closure*, rather than
an object, to the ISR.  Closures are fundamentally different, from an API
perspective, because they can capture local state easily -- and that capture is
visible to the borrow checker, to avoid races or dangling pointers.

In the end, the Rust API wound up being *very* different: there is no
`Rasterizer` trait, only functions. 

```rust
vga.with_raster(
    // The raster callback is invoked on every horizontal retrace to
    // provide new pixels.
    |line, tgt, ctx| do_stuff(),
    // The scope callback is executed to run application logic. As soon as
    // it returns, the raster callback is revoked from the ISR.
    |vga| loop {
      // drawing loop here
    })
```

This makes the problem of sharing state trivial: have the state in scope when
you declare these closures, and share it using normal Rust techniques.

Delightfully, if you try to use a method of state sharing that isn't safe in a
preemptive environment -- like, say, `Cell` -- *you get a compile error* because
the raster callback requires types that implement `Send`.

Basically, this API is really pleasant to use, much simpler than the original,
and *really hard* to misuse (unless you deliberately break it with `unsafe`). I
like it.

As of C++11, C++ has closures with captures. You could almost implement this
same API in `m4vgalib`. But I wouldn't, because...

- **It wouldn't be robust.** Capturing stack structures by reference creates a
  real risk that you'll accidentally leak the reference into a larger scope,
  e.g. by storing it in a global or member field of a long-lived object. Plus,
  C++'s type system doesn't have any notion of thread-safety, so nothing would
  stop you from sharing a non-threadsafe structure with the ISR. It's all
  [footguns][8].

- **It might require allocations.** In Rust, the ISR invokes the closure
  generically through the `FnMut` trait that closures implement. In C++, there
  is no direct equivalent; `std::function` is as close as it gets, but it
  requires a heap allocation, which we can't do.

### On binary size

Apples-to-apples, the Rust ports of my demos are larger than their C++
equivalents, in terms of Flash footprint. I've been studying this to see whether
it's (1) inherent, (2) current bugs, or (3) something I'm personally doing
wrong.

A naive comparison casts Rust in a bad light: running `arm-none-eabi-size` on
the current versions of `horiz_tp` written in each language, we get:

     text          data     bss     dec     hex filename
     4463            16  179688  184167   2cf67 cpp/horiz_tp
    21010            92  180872  201974   314f6 rust/horiz_tp

21kiB of code vs 4kiB seems like a big change (though it's worth noting that
I have 512kiB of Flash to work with). But that's misleading:

- 6,763 bytes of that are *string literals*. Why do I have so many string
  literals? From `panic!` messages. I actually had this problem in C++ at one
  point, where I use `assert` liberally. The C++ binary cheats by dropping these
  strings from the final binary.

- 8,519 bytes of that are formatting-related code for generating `panic!`
  messages. I wind up pulling in much of `core::fmt`.

If you subtract that out, the binaries are nearly the same size.  Interestingly,
the sizes are close *despite* the C++ demo being compiled mostly `-Os` (optimize
for size), and the Rust demo `-O2` (optimize for speed).

I'd like to find a way to remove support for panic messages from the binary, and
I bet it exists. However, it's worth noting that having human-readable panic
messages that can appear in an attached debugger (thanks to the `panic_itm`
crate) is *immensely helpful.*

### On memory safety

I'm currently using `unsafe` in 53 places. *None of them are for performance
reasons.*

The majority of `unsafe` code (**29 instances**) is related to a class of API
deficits in the `stm32f4` device interface crate I'm using. It treats any field
in a register for which it doesn't have defined valid bit patterns as
potentially unsafe...  and then fails to define most of the register fields I'm
using. Not sure why. I imagine this can be fixed. (I've already upstreamed part
of the fix.)

After that, the leading causes are situations that are *inherently* unsafe.
These are the reason that `unsafe` exists. In these cases the right solution is
to wrap the code in a neat, safe API (and I have):

- Calling into assembly code: 5
- Getting exclusive references to shared mutable global data: 5
- `UnsafeCell` access within custom mutex-like types: 4
- `unsafe impl Sync` on custom mutex-like types: 3
- Setting up the CPU (e.g. memory mapping, floating point, fault reporting,
  interrupt priorities): 2
- Doing something scary with `core::mem::transmute` to implement an inter-thread
  reference sharing primitive: 2
- Setting up the DMA controller: 1

This leaves two `unsafe` uses that can likely be fixed:

- Taking a very lazy shortcut with `core::mem::transmute` that can probably be
  improved: 1
- Deliberately aliasing a `[u32]` as `[u8]` (something that should be in the
  standard library): 1

By contrast: `m4vgalib` contains **10,692 lines of unsafe code.** That is, every
C++ statement that I wrote. Auditing 53 lines, all of which can be found by
`grep`, is much easier.

#### Bounds checking

I can't bring up memory safety without someone taking a potshot at Rust's bounds
checking for arrays. Since `m4vga` demands pretty high performance, I've been
auditing the machine code produced by `rustc`.

In the performance critical parts of the code, bounds checks were either
*already eliminated at compile time,* or could be eliminated by a simple
refactoring of the code.

The demos spend effectively no time evaluating bounds checks.

### On safety from data races

Most of the actual thinking that I had to do during the port -- as opposed to
mechanically translating C++ code into Rust -- had to do with ownership and
races.

(This won't surprise anyone who remembers learning Rust.)

`m4vga` is a prioritized preemptive multi-tasking system: it runs application
code at the processor's Thread priority, and interrupts it with a collection of
three interrupt service routines.

And, to keep things interesting, they all share data with each other. There's
potential for all manner of interesting data races. (And believe me, most of
them happened during the development of the C++ codebase.)

The C++ code uses a data race mitigation strategy that I call *convince yourself
it works once and then hope it never breaks.* (I can use a snarky name like that
because I'm talking about work *I did.*) In a couple of places I used
`std::atomic` (or my own intrinsics, before those stabilized), and in others I
relied on the Cortex-M performing atomic aligned writes and crossed my fingers.

I could certainly use the same strategy in Rust by employing `unsafe` code. But
that's boring.

Instead, I figured out which pieces of data were shared between which tasks,
grouped them, and wrapped them with custom bare-metal mutex primitives. Whenever
a thread or ISR wants to access data, it locks it, performs the access, and
unlocks it. This costs a few cycles more than the C++ "hold my beer" approach,
but that hasn't been an issue even in the latency-sensitive parts of the code.

Because of Rust's ownership and thread-safety rules, you can *only* share data
between threads and ISRs if it's packaged in one of these thread-safe
containers. If you add some new data and attempt to share it without protecting
it, your code will simply not compile. This means *I don't have to think about
data races* except when I'm hacking the internals of a locking primitive, so I
can think about other things instead.

On lock contention, we `panic!`. This is a hard-real-time system; if data isn't
available on the cycle we need it, the display is going to distort and there's
no point in continuing. Late data is wrong data, after all. Using Rust's
`panic!` facility has the pleasant side effect of printing a human-readable
error message on my debugger (thanks to the `panic_itm` crate).

So far two interesting side effects have come up:

1. Having to think about task interactions has led to a much better factoring of
   the driver code, which was initially laid out like the C++ code.

2. I found an actual bug *that also exists in the C++ code*. There was a subtle
   data race between rasterization and the start-of-active-video ISR. I caught
   it and fixed it in the Rust. I haven't yet updated the C++ (because meh).

[1]: https://github.com/cbiffle/m4vgalib-demos
[2]: https://rust-lang.org
[5]: https://docs.rs/crossbeam/0.7.1/crossbeam/thread/
[6]: https://rust-embedded.github.io/book
[7]: https://github.com/cbiffle/m4vgalib-demos/blob/master/README.mkdn#connections
[8]: https://en.wiktionary.org/wiki/footgun
[11]: https://github.com/cbiffle/m4vgalib
