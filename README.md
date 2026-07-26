# Speck

**Speck — a tiny language for tiny games.**

Speck is a deliberately small compiled language whose eventual forcing
function is a complete native game distribution that fits on a 1.44 MB floppy
disk. Speck source is checked and lowered to readable textual LLVM IR, compiled
to native object code, and linked with CRuMB.

**CRuMB — Compact Runtime for ultra-Minimal Binaries.**

CRuMB is Speck's narrow C ABI platform layer. It owns startup, the frame loop,
the frame delta, a fixed 320x180 RGB software framebuffer, debug output, and
shutdown. Its normal headless build runs five deterministic frames and writes a
PPM. A development-only presenter can instead stream complete frames to the
Speck compiler's small browser server. CRuMB remains independent of SDL,
Raylib, GLFW, and other game frameworks.

LLVM and the Speck compiler are development-time tools. They are not intended
to ship with a compiled game; the build output is a native executable linked
with CRuMB.

## Quick start

The current slice supports native builds on Linux x86-64 and macOS ARM64. It
requires Rust/Cargo and Clang. Linux also requires LLD. A standalone `llvm-as`
is optional because Clang can validate and compile textual LLVM IR directly.

```sh
cargo test --all-targets
cargo run -- build examples/crumb_bum.spk
./build/crumb_bum

# Infrastructure-only framebuffer verification
cargo run -- build examples/framebuffer_rect.spk
./build/framebuffer_rect

# Development-only live viewer (stops after 1,800 frames by default)
cargo run -- dev examples/moving_rectangle.spk

# macOS native-binary inspection
file build/framebuffer_rect
otool -L build/framebuffer_rect
```

The build command writes host-tagged, inspectable LLVM IR to
`build/crumb_bum.ll`, validates it with a compatible `llvm-as` when available
or with Clang otherwise, compiles it with Clang, and links it with
`runtime/crumb`. It reports the detected host, tools, validation path, and
executable byte size when complete.

The framebuffer example writes the final headless frame to `build/frame.ppm`.
The moving-rectangle example verifies browser presentation. Both are
infrastructure checks, not the contest game.

`speck dev` builds a separate `_dev` executable, starts its native frame stream
and a local HTTP viewer, and prints the exact viewer URL. It binds only to
`127.0.0.1` unless `--bind` is supplied explicitly. See the
[development viewer guide](docs/development-viewer.md) for remote access over
an SSH tunnel, frame limits, and the transport protocol. The CLI remains
organized so `check`, `run`, and `size` can be added naturally.

## Current limits

This is an honest headless compiler/runtime slice, not a general-purpose
language or a finished tiny-game platform. There is no heap, garbage collector,
input, audio, arrays, modules, asset system, or native window. Graphics are
currently limited to clearing the software framebuffer and drawing clipped
filled rectangles. The browser viewer is remote development tooling, not a game
runtime feature. Global initializers are literal constants, numeric types do
not convert implicitly, and native executables use the host's dynamic C library.
Native macOS presentation is intentionally deferred.

See [the language reference](docs/language.md),
[architecture](docs/architecture.md), [byte budget](docs/byte-budget.md), and
[roadmap](docs/roadmap.md).
