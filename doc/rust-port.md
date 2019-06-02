# Musings on porting to Rust

This is a collection of my notes on porting [m4vgalib][1] and my collection of [C++
demos][2] to Rust.

## Executive summary

- The Rust tools and library ecosystem are fantastic.

- Writing in Rust freed up my brain, so I could put energy into optimizing
  things and adding features instead of watching for undefined behavior.

- Rust's safety features, such as bounds checking, don't hurt this
  performance-sensitive application in the least.

- This port revealed significant but subtle bugs in the C++ code, because the
  compiler wouldn't accept them in Rust.

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

## On `no_std`

The *single best* thing about Rust for bare-metal programming is the `no_std`
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

1. By setting the `#[no_std]` attribute on a crate, you're opting out of the
   default dependency on `std`. Any attempt to use a feature from `std` is now a
   compile time error -- but you can still use `core`.

2. You can trust *other* crates to do the same, so you can use third-party
   libraries safely. Many crates are either `no_std` by default, or can have it
   enabled at build time.

3. `core` is small enough that porting it to a new platform is easy --
   significantly easier, in fact, than porting `newlib`, the standard-bearer for
   portable embedded C libraries.

For `m4vgalib` I rewrote almost all my dependencies to get a system that
wouldn't throw or allocate. In Rust, I don't have to do that!


## On API design

Rust's ownership rules produce a sort of bizarro-world of API design.

- Some (uncommon, but reasonable) API designs won't make it past the borrow
  checker. (In nearly every case, these are APIs that would have sported large
  "how to use safely" comments in other languaes.)

- Some API patterns that are grossly unsafe or unwise in other languages are
  routine in Rust because of e.g. lifetime checking.

As an example of the latter: it is common, and safe, to loan out stack-allocated
data structures *to other threads* with no runtime checks. (See: [scoped threads
in crossbeam][3].) I implemented the same thing for loaning data to ISRs in
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
- "This feels a lot like the problem that [scoped threads][3] solves."
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
  [footguns][4].

- **It might require allocations.** In Rust, the ISR invokes the closure
  generically through the `FnMut` trait that closures implement. In C++, there
  is no direct equivalent; `std::function` is as close as it gets, but it
  requires a heap allocation, which we can't do.

## On binary size

Rust has a reputation for producing larger binaries than C++ -- a reputation
that is largely undeserved.

If you run a release build and run `size`, you will find binaries that are
larger than their C++ equivalents. For example, here's a comparison of
`horiz_tp` written in each language:

     text          data     bss     dec     hex filename
     4463            16  179688  184167   2cf67 cpp/horiz_tp
    21010            92  180872  201974   314f6 rust/horiz_tp

This comparison is *misleading*. The C++ codebase goes to some length to avoid
including extraneous material in Flash -- in particular, it compiles out all
assert messages.

I don't have Rust configured this way by default, because I like getting panic
messages through my debugger when I mess up. But this means each binary contains
all the panic strings, plus all the message formatting code. If you would like
to produce smaller binaries, and are willing to sacrifice panic messages, you
need to build with a different feature set:

    $ cargo build --release --no-default-features --features panic-halt

In this mode, the binaries are much smaller:

    text    data     bss     dec     hex filename
    4366     104  180860  185330   2d3f2 horiz_tp
    4404     104  180796  185304   2d3d8 xor_pattern
    6688     104  180152  186944   2da40 conway

In fact, *the binaries are 3-9% smaller than in C++,* despite compiling the C++
with `-Os` and the Rust with (the equivalent of) `-O3`.

## On memory safety

I'm currently using `unsafe` in 35 places. *None of them are for Rust-specific
performance reasons.* (I say "Rust-specific" because some of them are calling
into assembly routines, which definitely exist for performance reasons, but are
identical in C++.)

The majority of `unsafe` code (**13 instances**) is related to a class of API
deficits in the `stm32f4` device interface crate I'm using. It treats any field
in a register for which it doesn't have defined valid bit patterns as
potentially unsafe...  and then fails to define most of the register fields I'm
using. Not sure why. I imagine this can be fixed. (I've already upstreamed part
of the fix.)

After that, the leading causes are situations that are *inherently* unsafe.
These are the reason that `unsafe` exists. In these cases the right solution is
to wrap the code in a neat, safe API (and I have):

- 5 cases: Getting exclusive references to shared mutable global data, which is
  super racy unless you're careful.
- 4 cases: Calling into assembly code, which can do literally whatever it wants
  and so must be handled carefully.
- 4 cases: Managing the DMA controller, which is basically a peripheral for
  doing unsafe memory things.
- 3 cases: Implementing custom mutex-like types.
- 2 cases: Setting up the CPU and hardware environment.
- 2 cases: Doing something scary with `core::mem::transmute` to implement an
  inter-thread reference sharing primitive:

This leaves two `unsafe` uses that can likely be fixed:

- Taking a very lazy shortcut with `core::mem::transmute` that can probably be
  improved.
- Deliberately aliasing a `[u32]` as `[u8]` (something that should be in the
  standard library).

By contrast: `m4vgalib` contains **10,692 lines of unsafe code.** That is, every
C++ statement that I wrote. Reviewing all possible sources of pointer-related
bugs by reading *35 lines* -- all of which can be found by `grep` -- is much
easier than reviewing over 10k lines of C++.

### Bounds checking

I can't bring up memory safety without someone taking a potshot at Rust's bounds
checking for arrays. Since `m4vga` demands pretty high performance, I've been
auditing the machine code produced by `rustc`.

In the performance critical parts of the code, bounds checks were either
*already eliminated at compile time,* or could be eliminated by a simple
refactoring of the code.

The demos spend effectively no time evaluating bounds checks.

## On safety from data races

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

[1]: https://github.com/cbiffle/m4vgalib
[2]: https://github.com/cbiffle/m4vgalib-demos
[3]: https://docs.rs/crossbeam/0.7.1/crossbeam/thread/
[4]: https://en.wiktionary.org/wiki/footgun
