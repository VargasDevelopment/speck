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
Speck compiler's small browser server. On macOS ARM64, a native Cocoa presenter
displays the same framebuffer in a resizable window. CRuMB remains independent
of SDL, Raylib, GLFW, and other game frameworks.

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

# Language-ergonomics example: constants, conversions, void, &&/||, and +=
cargo run -- build examples/delta_rectangle.spk
./build/delta_rectangle

# Infrastructure-only framebuffer verification
cargo run -- build examples/framebuffer_rect.spk
./build/framebuffer_rect

# Development-only live viewer (stops after 1,800 frames by default)
cargo run -- dev examples/moving_rectangle.spk

# Native macOS window (runs until the window closes or Ctrl-C)
cargo run -- run examples/moving_rectangle.spk

# The language-ergonomics sketch in the same native presenter
cargo run -- run examples/delta_rectangle.spk

# Bounded native run for smoke tests
cargo run -- run examples/moving_rectangle.spk --frames 3

# macOS native-binary inspection
file build/moving_rectangle_native
otool -L build/moving_rectangle_native
```

The build command writes host-tagged, inspectable LLVM IR to
`build/crumb_bum.ll`, validates it with a compatible `llvm-as` when available
or with Clang otherwise, compiles it with Clang, and links it with
`runtime/crumb`. It reports the detected host, tools, validation path, and
executable byte size when complete.

The framebuffer example writes the final headless frame to `build/frame.ppm`.
The original moving-rectangle example verifies browser and native presentation.
`delta_rectangle.spk` expresses the same kind of sketch with named constants,
`f32` simulation state, explicit pixel conversion, effect-only helper
functions, short-circuit conditions, and compound assignment. These remain
verification sketches, not the contest game.

`speck dev` builds a separate `_dev` executable, starts its native frame stream
and a local HTTP viewer, and prints the exact viewer URL. It binds only to
`127.0.0.1` unless `--bind` is supplied explicitly. See the
[development viewer guide](docs/development-viewer.md) for remote access over
an SSH tunnel, frame limits, and the transport protocol.

`speck run` is currently available only on macOS ARM64. It builds a separately
named `_native` executable and launches a normal AppKit window over CRuMB's
320x180 RGB framebuffer. The window uses nearest-neighbor drawing, prefers
integer backing-pixel scale factors, and centers or letterboxes the image while
resizing. Interactive runs are unbounded; closing the window or pressing Ctrl-C
requests an orderly shutdown. `--frames N` supplies the finite private runtime
limit used by automation. It does not change Speck language semantics.

## Current limits

This is an honest small compiler/runtime slice, not a general-purpose language
or a finished tiny-game platform. There is no heap, garbage collector,
input, audio, arrays, modules, or asset system. Graphics are currently limited
to clearing the software framebuffer and drawing clipped filled rectangles.
There is no keyboard, mouse, or controller input. The Cocoa presenter is a
minimal macOS display path, and the browser viewer remains remote development
tooling rather than game semantics. Numeric types do not convert implicitly;
`i32(...)` and `f32(...)` make conversions explicit. Top-level immutable
constants and mutable globals use compile-time expressions, while native
executables use the host's dynamic system libraries.

See [the language reference](docs/language.md),
[design principles](docs/design-principles.md),
[language friction log](docs/friction-log.md),
[architecture](docs/architecture.md), [byte budget](docs/byte-budget.md), and
[roadmap](docs/roadmap.md).
