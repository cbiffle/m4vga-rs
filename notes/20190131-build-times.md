# Notes on build times

Rust builds slower than C++, but not unacceptably so.

For release builds on a laptop:

- Complete from-scratch build including all dependencies: 1m09s.
- Rebuilding just the local code: 2.64s
- Iterating on one demo: 0.82s

In C++ using Cobble:

- Scratch build: 3.589s. This is a fair comparison for the *local code* Rust
  build -- as libstdc++ and newlib are prebuilt.
- Iterating on one demo: 0.33s

Cobble was designed to be ridiculously fast for codebases like this. The C++
build is building a lot of demos I haven't ported yet, so I expect the Rust
build to get slower still. But I'd say it's about parity.
