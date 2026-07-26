# Speck

**Speck — a tiny language for tiny games.**

Speck is a deliberately small compiled language whose eventual forcing
function is a complete native game distribution that fits on a 1.44 MB floppy
disk. Speck source is checked and lowered to readable textual LLVM IR, compiled
to native object code, and linked with CRuMB.

**CRuMB — Compact Runtime for ultra-Minimal Binaries.**

CRuMB is Speck's narrow C ABI platform layer. Its headless v0 owns startup, a
deterministic five-frame loop, the frame delta, debug output, and shutdown. It
is intentionally independent of SDL, Raylib, GLFW, and other game frameworks.

LLVM and the Speck compiler are development-time tools. They are not intended
to ship with a compiled game; the build output is a native executable linked
with CRuMB.

## Quick start

The current slice requires Rust/Cargo, Clang, LLVM command-line tools, and LLD:

```sh
cargo test
cargo run -- build examples/crumb_bum.spk
./build/crumb_bum
```

The build command writes inspectable LLVM IR to `build/crumb_bum.ll`, validates
it with `llvm-as`, compiles it with Clang, and links it with `runtime/crumb`.
It reports the executable's exact byte size when complete.

Only `speck build` exists in this slice. The CLI is organized around a command
argument so `check`, `run`, and `size` can be added without changing the
compiler pipeline.

## Current limits

This is an honest headless compiler/runtime slice, not a general-purpose
language or a finished tiny-game platform. There is no heap, garbage collector,
graphics, input, audio, arrays, modules, or asset system. Global initializers
are literal constants, numeric types do not convert implicitly, and the current
Linux executable uses the host's dynamic C library.

See [the language reference](docs/language.md),
[architecture](docs/architecture.md), [byte budget](docs/byte-budget.md), and
[roadmap](docs/roadmap.md).
