# Typestate pattern

`Vga` uses the concept of "typestates:" customizing a type (here, the driver
handle) with additional information about its state, which in turn determines
what operations are legal.

To me, there are three reasons this pattern is interesting:

1. It's a simple and powerful technique for making APIs that are easy to use and
   understand.

2. It's pretty common in Rust libraries, often in subtle forms.

3. *It simply isn't possible* in any other mainstream programming language, as
   far as I'm aware. I'm particular, I'm pretty sure it can't be used in C++.

## Overview

The basic idea is this.

We have a state machine -- here, the display driver. There are operations you
can perform that don't change state (like synchronizing to vblank), and then
there are transition operations (like configuring a particular display timing).

To encode the states, we add a type parameter to our type -- so `Vga` is
actually `Vga<S>`, where `S` represents its state. We define three states that
the driver can be in:

- `Idle` (nothing is happening)
- `Sync` (sync generated, monitor should turn on and display black)
- `Live` (video generated)

These states are implemented as *types*. Because they are simply type-level
tags, it's not useful to instantiate them, and so they're often implemented as
[zero-variant enums][1], like so:

```rust
pub enum Idle {}
pub enum Sync {}
pub enum Live {}
```

## State-specific operations

We can define operations that are legal in *any* state the usual way, with an
`impl` block that's generic in `S`:

```rust
/// In any state...
impl<S> Vga<S> {
    pub fn available_always(&self) { ... }
}
```

We can also define operations that are legal in only one state, because Rust
lets us define `impl` blocks that specialize on a type parameter:

```rust
/// Operations that are only available when `Live`.
impl Vga<Live> {
    pub fn available_only_when_live(&self) -> u32 { ... }
}
```

This is already useful. Traditionally, if you use an operation in the wrong
state, we would handle it in one of the following ways:

- We could panic, or (in other languages) throw. This would ensure the mistake
  gets caught, but not until runtime -- and it implies runtime checking code
  wasting cycles and bytes.

- We could return an error code (or a Rust `Result`). This has all the problems
  of the first option, with added boilerplate to check the return code *even if
  you know you're using the API correctly*. (Not to mention that, depending on
  the language, you might be able to forget to check the return code entirely!
  Rust has `#[must_use]`, and C++17 has finally added `[[nodiscard]]`, but other
  than that such compiler checks are rare.)

With the typestate pattern, **using the API in the wrong state becomes a compile
error.** No runtime checks are required, and there's no risk we'll miss the
mistake.

Plus, the distinction is pretty clear in the `rustdoc`-generated documentation.
The methods for different states will be broken out into separate headings,
something like:

> ## `impl<S> Vga<S>`
>
> In any state...
>
> `pub fn available_always(&self)`
>
> ## `impl Vga<Live>`
>
> Operations that are only available when `Live`.
>
> `pub fn available_only_when_live(&self)`


## State transitions

State transitions are really just a special case of operations that are only
available in certain states. For example, consider these two state transitions
on `Vga`:

- When in `Idle` state, `configure_timing` moves us to `Sync` state, if you can
  provide a valid `Timing` definition.
- When in `Sync` state, `stop_sync` moves us back to `Idle`.

We can express these as two state-specific methods:

```rust
impl Vga<Idle> {
    // From Idle state, we can transition to Sync by providing Timing.
    fn configure_timing(self, timing: &Timing) -> Vga<Sync> { ... }
}

impl Vga<Sync> {
    // From Sync state, we can transition back to Idle by shutting down sync
    // generation.
    fn stop_sync(self) -> Vga<Idle> { ... }
}
```

Look closely, though. While the operations in the previous section took `self`
by reference and returned nothing (or an unrelated value), these state
transition operations

- Take `self` *by-value*, and
- Return a `Vga` in a *different state.*

In application code, this looks like:

```rust
let vga: Vga<Idle> = get_vga_somehow();
let vga: Vga<Sync> = vga.configure_timing(&timing);

// do stuff

let vga: Vga<Idle> = vga.stop_sync();
```

Or, more concisely by using method chaining,

```rust
let vga = get_vga_somehow().configure_timing(&timing);

// do stuff

vga.stop_sync(); // we're done with it, don't save the result
```

Because state transition methods are state-specific, you will get a compile
error if you try to apply one in the wrong state:

```rust
get_vga_somehow().stop_sync();  // ERROR: no method named `stop_sync` found for
                                // type `Vga<Idle>`
```

Similarly, they can't be accidentally applied multiple times:

```rust
get_vga_somehow().configure_timing(&timing)
    .configure_timing(&another_timing);
    // ERROR: no method named `configure_timing` found for type `Vga<Sync>`
```

And because state transitions consume `self` by value, the transitions are
*enforced*. You cannot fork the state of the driver by retaining its
pre-transition state, deliberately or accidentally.

```rust
// I'm-a try and be sneaky:
let vga_idle: Vga<Idle> = get_vga_somehow();
let vga_sync: Vga<Sync> = vga_idle.configure_timing(&timing);

let vga_sync2 = vga_idle.configure_timing(&timing);
  // ERROR: use of moved value `vga_idle`
```

## Operations available in multiple states

Sometimes it's useful to have an operation available in multiple states. 
For example, `m4vga` provides a `sync_with_vblank` operation in any state where sync generation is enabled. The specialization approach used above doesn't work.

Instead, we need to articulate to the compiler that all the states have a
*common  property*, and enable the operation for states with that property.
Here, the property is "sync is being generated."

States are types, of course, and the way we ascribe properties to types is:
traits.

```rust
pub trait SyncOn {}

// SyncOn is true in two states:
impl SyncOn for Sync {}
impl SyncOn for Live {}
```

We can then add operations that depend on the `SyncOn` property, two different
ways:

```rust
impl<S> Vga<S> {
    // Individual bounds on methods
    fn sync_only_operation(&self)
        where S: SyncOn
    { ... }
}

// Trait-specific impl block
impl<S: SyncOn> Vga<S> {
    fn sync_only_operation2(&self) { ... }
}
```

The choice between these basically comes down to personal preference. They will
be presented differently in the documentation:

- Functions using a `where` clause will be presented with the other functions in
  their `impl` block, which might be useful for grouping several related
  operations that apply in different states.

- A separate `impl` block will be presented apart, and can have doc comments
  attached, to discuss its role.

## States are often zero-sized types (ZSTs)

Rust won't let you declare a type parameter without using it. The normal
pattern for dealing with "phantom types" is `PhantomData`:

```rust
struct Vga<S> {
    // "Use" S by including it
    _marker: PhantomData<S>,
}
```

`PhantomData` "uses" a type, but gets eliminated at runtime. This is valuable if
the type you're referencing is large or has a costly `Drop` impl, for example.

However, chances are pretty good that state types don't contain any data. This
means they can be written as [zero-sized types][0] and included in other types
as a simple field, at no cost. (While this makes states constructible --
something we avoided before, to reduce the API surface area -- we can limit
construction to the module.)

```rust
// Giving a single zero-sized private field makes the types unconstructible
// outside this module.
pub struct Idle(())
pub struct Sync(())

pub struct Vga<S> {
    // No PhantomData required
    state: S,
}
```

This makes the code easier to read, in my opinion. (I grok `PhantomData`, but
it's still potentially distracting.)

It's also free! `PhantomData` is a marker type eliminated at runtime, but this
isn't because it's special -- it's because it's zero-sized. Any zero-sized type
has the same effect, including our state types.

In fact, as written, `Vga<Idle>` and `Vga<Sync>` are *also* zero-sized types, so
they can be manipulated and stored without runtime cost. (This is actually true
of both `Vga<Sync>` and `Vga<Live>` in the real `m4vga` codebase. To see why it
isn't true of `Vga<Idle>`, keep reading.)

## States don't have to be zero-sized types (ZSTs)

As discussed in the previous section, state types are often [zero-sized][0].

But they don't have to be.

This provides a way to *change the fields* in your type depending on the state.
`m4vga` uses this to manage resources that switch ownership between the
application and the interrupt service routines (ISRs) depending on state.

At startup, the driver owns certain hardware drivers. Once sync generation
starts, an ISR takes over exclusive control. Here's a slightly simplified
version:

```rust
// The idle state, where we control the TIM4 peripheral.
pub struct Idle(device::TIM4)

// The sync state, with a single private field so clients can't get confused and
// make one.
pub struct Sync(())

// Note: if that read
//   pub struct Sync;
// we would have no way of restricting the ability to construct one.

// The driver state, as shown in the previous section.
struct Vga<S> {
    // Our state, stored as a field rather than PhantomData.
    // This means that:
    // - When S is Idle, we have a field of type TIM4.
    // - When S is Sync, we have a field of type ().
    state: S,
}

// Makes a driver instance by donating a timer.
pub fn init(tim4: device::TIM4) -> Vga<Idle> {
    Vga { state: Idle(tim4) }
}

impl Vga<Idle> {
    pub fn configure_timing(self, timing: &Timing) -> Vga<Sync> {
        // Destruct self to get at the timer.
        let Vga { state: Idle(tim4) } = self;

        // This is pseudocode.
        apply_timing_settings(timing, &tim4);
        donate_to_isr(tim4);  // we have lost access to it

        // Return the driver in the new state.
        Vga { state: Sync(()) }
    }
}

impl Vga<Sync> {
    pub fn stop_sync(self) -> Vga<Idle {
        // This is pseudocode.
        disable_sync_isr();
        let tim4 = take_timer_back();
        Vga { state: Idle(tim4) }
    }
}
```

To use this variant on the pattern, the state types (`Idle` and `Sync`, here)
need to be constructible, unlike before -- no [zero-variant enums][1] for us.

Critically, each state type can be a different size. The `Idle` state holds
information about the timer driver, while the `Sync` state holds nothing. As
written, `Vga<Sync>` is *itself* a ZST, so once the hardware is donated to the
ISR, no code will be generated to move the driver handle around until we
`stop_sync`.

`Vga<Idle>`, as written above, is merely a (transitive) [newtype][2] around
`TIM4`, so manipulating it is the same cost as directly passing a `TIM4` around.


## When you don't know the state (detecting state dynamically)

The pattern so far works great when you know the state statically, like in the
code examples above. It also lets us write code that's generic over states,
which will get monomorphised (specialized to each type it's used with):

```rust
fn do_vga_thing<S>(driver: &Vga<S>) {
    println!("I work in any state");
    driver.thingy();
}
```

Any caller that knows the true state, or who is *also* generic over it, can use
`do_vga_thing`.

But what if we need to interact with a object that uses typestates, *without
knowing its state*? For example, what if we have *many* objects in different
states that change dynamically?

There are several solutions for this. I don't yet have a favorite.

### State-independent trait

Operating on a value using typestates, without knowing its precise state, is
equivalent to operating on something without knowing its concrete type. How do
we operate on something without knowing its type?

Dynamic dispatch.

Define a *state-independent trait* containing the state-independent operations
you want to define:

```rust
trait VgaAnyState {
    // Sync to vblank, or if sync is not started yet, just return. The returned
    // flag indicates whether anything was done.
    fn maybe_sync_to_vblank(&self) -> bool;
}
```

Then, implement it for each state.

```rust
impl VgaAnyState for Vga<Idle> {
    fn maybe_sync_to_vblank(&self) -> bool { false }
}

impl VgaAnyState for Vga<Sync> {
    fn maybe_sync_to_vblank(&self) -> bool {
        self.sync_to_vblank();
        true
    }
}

// ... and so forth
```

Now you can define functions that can operate on a `Vga<S>` for any (valid) `S`,
even if it's unknown at compile time:

```rust
fn my_function(vga: &dyn VgaAnyState) {
    vga.maybe_sync_to_vblank();
}
```

Advantages:

- Any `Vga<S>` can be passed, by reference, as a `&dyn VgaAnyState`, whether it
  lives on the stack, heap, or in ROM.

- New state-independent traits can be added by users without needing to alter
  the library.

- If users can add new states, they can `impl` the trait for them.

Disadvantages:

- Traits are *open* to new `impl`s -- a user, or downstream crate, could
  introduce a new implementation of `VgaAnyState` for some random type. This is
  exactly what traits are for, but may not be what your API was expecting. You
  cannot assume that being able to call `my_function` (above) implies that the
  user holds a valid `Vga<S>` instance!

- May impose a small runtime cost, unless the compiler can erase the need for
  dynamic dispatch by inlining and analysis.

- Cannot include API that needs to operate by-value, since dynamic dispatch
  works through references only. For example, such a state-independent trait
  cannot specify state transition operations.

### State-reflecting `enum`

Assuming that the set of states is *closed* (i.e. known to us, as the library
authors), then interacting with our object without knowing its state means we're
interacting with one of a small set of variants of the object.

What do we use to encode variants of something? An `enum`.

We can define an `enum` that represents (*reifies*) the distinction between
typestates at runtime. Rather than simply enumerating the states, we package an
actual instance of the object in each typestate as a field:

```rust
enum AnyVga {
    Idle(Vga<Idle>),
    Sync(Vga<Sync>),
    Live(Vga<Live>),
}
```

Values of the type `AnyVga` serve as *containers* that can hold a `Vga<S>` in
any of three states. They can have operations defined:

```rust
impl AnyVga {
    fn video_on_if_live(&self) -> bool {
        match self {
            AnyVga::Live(v) => { v.video_on(); true },
            _ => false,
        }
    }

    fn maybe_sync_to_vblank(&self) -> bool {
        match self {
            AnyVga::Sync(v) => { v.sync_to_vblank(); true },
            AnyVga::Live(v) => { v.sync_to_vblank(); true },
            _ => false,
        }
    }

    // If `self` is in state `Idle`, applies the `configure_timing` operation
    // to transition to `Sync`. Otherwise, does nothing.
    //
    // The return value will be `Ok(vga)` with a *concrete* type of `Vga<Sync>`
    // on success, and `Err(any_vga)` on failure, returning `self` in a yet
    // undetermined state.
    //
    // This operation is somewhat contrived, but gives an example of the power
    // of this technique.
    fn maybe_configure_timing(self, timing: &Timing)
        -> Result<Vga<Sync>, AnyVga>
    {
        match self {
            AnyVga::Idle(v) => Ok(v.configure_timing(timing)),
            s => Err(s),
        }
    }
}
```

Advantages:

- It is *closed*, so you can detect the different states using `match` (as shown
  above), and you can't accidentally apply the operations to the wrong type.

- By-value API can work. For example, the (admittedly contrived)
  `maybe_configure_timing` operation above.

- Code can be more compact than using a state-independent trait.

- Exhaustiveness checks on pattern matching will catch cases where you forgot to
  handle one of the states.

Disadvantages:

- The object needs to live *in* the enum. That means whoever owns the object
  needs to have planned for this, and placed it inside a variant of the enum.
  (You can implement a variation where the enum holds references, but this
  prevents by-value API from working.)

- You need to maintain an `enum` type that reflects your state type family,
  which is boilerplate. Not a lot of it, but boilerplate all the same.

### Use `Any`

There's another way to interact with an object without knowing its type -- one
which doesn't require us to define a custom trait *or* type: `Any`.

```rust
use core::any::Any;

fn video_on_if_live(vga: &dyn Any) -> bool {
    if let Some(v) = vga.downcast::<Vga<Live>>() {
        v.video_on(); true
    } else {
        false
    }
}
```

I don't actually recommend this approach, but I'm including it for completeness.

Advantages:

- Requires very little additional code -- generally "just works."

- *Not* closed -- if you let clients introduce states, for example, this may be
  your best option.

- Can work on objects in any sort of container without advance preparation.

Disadvantages:

- *Too* general -- `video_on_if_live` above can also be called on `String`! (It
  will return `false`).

- Not closed; if you forget to test a type, there is no exhaustiveness check
  like there is for `match`.

- The way `Any` works is sometimes surprising; you can downcast to a *single*
  type, chosen by the type at the point the `Any` is captured.

- Requires repeated downcasting tests instead of a match, which will cost
  (somewhat) more at runtime.


## You can't do this in C++

And now, the section that I expect will be controversial.

This is the first common Rust design pattern that I'm fairly certain can't be
implemented in C++. Spoiler: there's a difference in the way move semantics are
defined in the two languages that causes trouble.

C++ is a ridiculously powerful programming language. It can obviously handle
driver handle types that take a state parameter like `Vga<S>`, and operations
that are only valid in one state. But it can't get the state transition
operations right.

Here's an approximation of our Rust API:

```c++
// Boilerplate follows

// States as zero-variant enums
enum Idle {};
enum Sync {};

// Placeholder for the timing parameters.
struct Timing {};

// Driver handle, with boilerplate to prevent user copies.
template<typename S>
struct Vga {
    // Prevent people from forking driver state.
    Vga(Vga const &) = delete;
    
    // Allow moves, because they're kind of the point here.
    Vga(Vga const &&) = default;

    // Assignment is fine, inherit however many operators that is now

    // Allow configure_timing taking `this` by move (note the trailing &&).
    // Only allow this if S is Idle.
    std::enable_if_t<std::is_same_v<S, Idle>, Vga<Sync>>
        configure_timing_move(Vga<Idle> vga, Timing const & timing) &&;
      
private:
    // Prevent people from synthesizing a driver in whatever state they want.
    Vga() = default;

    // Allow our constructor to work.
    friend Vga<Idle> make_vga();

    // A member field. If you omit this, anyone can construct your type using
    // aggregate initialization, and you can't make that private, much like
    // Rust's `struct Foo;`.
    bool _x;
};

// Constructor-like function with S fixed as Idle.
Vga<Idle> make_vga() { ... }

// Alternative: take the handle by-value to configure timing. We can't have a
// member function that takes `this` by-value, so this function lives out here.
Vga<Sync> configure_timing_byval(Vga<Idle> vga, Timing const & timing) { ... }
```

So far, so good -- except that `std::enable_if_t` is very difficult to read in
user documentation. C++ doesn't have anything analogous to Rust's specialized
`impl` blocks, which *add* methods -- in a template specialization we can only
replace the *entire type*, not merely add. So we have to resort to either
`std::enable_if_t` or writing free functions like `config_timing_byval`. Both
impact the clarity of the API.

Okay, clarity concerns aside, does the API work? Unfortunately, it does not.

```c++
auto vga = make_vga();
// Note: because copy construction is disallowed, we *can't* pass the object by
// value, because passing by value implies a *copy*. We can explicitly move
//though.
auto vga2 = configure_timing_byval(std::move(vga), Timing{});
auto vga3 = configure_timing_byval(std::move(vga), Timing{}); // uh-oh
auto vga4 = configure_timing_byval(std::move(vga), Timing{}); // crap

// Move time. We still need std::move to manufacture an rvalue reference:
auto vga5 = std::move(vga).configure_timing_move(Timing{});
auto vga6 = std::move(vga).configure_timing_move(Timing{}); // still? lame.
```

You could potentially design the API so that it works correctly using method
chaining, like...

```c++
make_vga()
    .configure_timing(Timing{})
    .other_stuff()
```

But this is a false sense of security, which contains a lurking violation of the
Principle of Least Surprise. Introducing a local at a point where a program
previously used an anonymous temporary value *should preserve
correctness* of a well-designed API, but it doesn't here:

```c++
auto v = make_vga();  // introduced local -- maybe the line was too long
v.configure_timing(Timing{})
    .other_stuff();

v.configure_timing // ...we're back at it
```

There are two properties of the C++ language that conspire to break state
transitions in the typestate pattern.

1. Passing by-value means *copy*, not *move*. The caller retains access to
   whatever object was passed by-value (because it was copied).

2. Even if it were defined as *move*, a *move* operation leaves the source
   object reachable and in a valid state. It must, in fact, because the source
   object's destructor will still be called.

Fixing either of these would break a *lot* of existing code, so given C++'s
focus on the past, I'm guessing they're not getting fixed.

[0]: https://doc.rust-lang.org/nomicon/exotic-sizes.html#zero-sized-types-zsts
[1]: https://doc.rust-lang.org/reference/items/enumerations.html#zero-variant-enums
[2]: https://doc.rust-lang.org/book/ch19-04-advanced-types.html#using-the-newtype-pattern-for-type-safety-and-abstraction
