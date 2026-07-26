# Architecture

Speck remains one Rust crate plus a small C runtime. The compiler has one direct
development-process dependency (`ctrlc`); generated games do not link Rust:

```text
.spk source
  -> lexer (tokens with byte spans)
  -> parser (AST)
  -> semantic/type validation
  -> host-tagged textual LLVM emitter
  -> compatible llvm-as, or Clang-direct, validation and bitcode
  -> Clang native object
  -> native link with CRuMB objects (LLD on Linux, Apple ld on macOS)
  -> native executable
```

`src/lib.rs` exposes the analysis and LLVM-emission pipeline. `src/main.rs` and
`src/cli.rs` are a thin command boundary. `lexer`, `parser`, and `sema` are
independent stages that return source-located diagnostics. The AST is small
enough that a separate HIR would currently duplicate it without simplifying
lowering, so the validated AST goes directly to `codegen/llvm.rs`.

The LLVM backend only produces text. Tool discovery, files, subprocesses, and
linking live in `toolchain.rs`, keeping the emitter independent of LLVM's API.
A future library-based backend can therefore replace the emitter without
changing parsing or semantics.

`toolchain.rs` contains the deliberately small host abstraction. It recognizes
only Linux x86-64 and macOS ARM64, and owns the object/executable suffixes,
runtime platform source, compiler flags, and link strategy that differ between
them. Unsupported hosts receive a diagnostic; this is native compilation, not
cross-compilation.

On macOS, Clang and the SDK are discovered with `xcrun` before PATH and an
already installed Homebrew LLVM are considered. On Linux, Clang, `llvm-as`, and
`ld.lld` are discovered on PATH. The selected Clang's `-dumpmachine` result is
validated against the Rust-detected host and written into the textual module as
its LLVM target triple. Speck intentionally omits a hand-maintained target data
layout: the selected Clang supplies the authoritative layout when it consumes
the module. A discovered `llvm-as` is used only when its bitcode can also be
consumed by that Clang; otherwise Clang validates and compiles the `.ll` file
directly. In either case, both `.ll` and `.bc` remain in `build/`.

CRuMB's `crumb.h` is the stable C ABI boundary. The generated object exports
`spk_start`, `spk_update(float)`, and `spk_draw`; CRuMB supplies `main`, owns the
finite loop, and exposes initialization, frame delta, debug output, software
drawing, and shutdown. A normal headless build executes five deterministic
1/60-second frames. A development build uses an explicit finite frame limit and
paces the loop at approximately 60 frames per second.

CRuMB's graphics path is split by responsibility:

- `framebuffer.c` owns a packed, row-major 320x180 RGB framebuffer and implements
  clear and clipped filled-rectangle rasterization.
- `present_ppm.c` is selected for normal builds. After every `spk_draw`, it
  overwrites `build/frame.ppm` with a dependency-free binary P6 PPM image.
- `present_stream.c` is selected only by `speck dev`. It connects to a loopback
  TCP listener created by the development host and sends length-checked,
  sequence-numbered complete RGB frames.
- `crumb.c` owns lifecycle sequencing and calls three private presenter hooks:
  initialize, present, and shut down. It knows neither the framebuffer's
  pixel-writing rules nor the selected presenter's implementation.
- `platform/posix_main.c` contains only the program entry point used by both
  supported hosts. The toolchain selects it as the platform source.

The public C ABI provides fixed width, height, channel, stride, and byte-count
constants plus immutable framebuffer pixel access. Browser, HTTP, TCP, and
operating-system concepts are absent from this ABI and from Speck semantics. A
future Cocoa presenter can implement the same private presenter hooks, consume
the immutable framebuffer view, and leave Speck programs, drawing operations,
and the rasterizer unchanged.

## Development browser presenter

`speck dev` is a host-side orchestration mode, not a different language. It
builds a separately named `_dev` native executable whose CRuMB objects contain
the stream presenter instead of the PPM presenter. The compiler process creates
two listeners:

```text
native Speck `_dev` process
  -> private loopback TCP frame protocol
  -> Rust development host (validates and retains latest complete frame)
  -> localhost HTTP long polling
  -> generic HTML canvas viewer
```

The binary frame channel and inherited game stdout/stderr are separate, so
debug logs cannot corrupt framing. The server retains only the latest complete
frame. A slow browser may skip frames, but it cannot observe a partial frame.
HTTP long polling was sufficient for fixed 320x180 frames and avoided adding a
WebSocket framework. The viewer disables smoothing and chooses integer scaling
whenever the viewport can fit at least one native-size framebuffer.

The normal build path selects only `present_ppm.c`; it does not compile or link
the TCP presenter. The HTML, HTTP server, Ctrl-C handler, and orchestration code
live in the Speck compiler executable, which is already a development-time
tool. They do not appear in a generated normal game executable.

## Current limitations

- Native host support is limited to Linux x86-64 and macOS ARM64.
- Output is dynamically linked against the host C library.
- CRuMB uses `printf` solely for development verification.
- There is no native window, input, audio, asset, allocation, or interactive
  platform backend. PPM and remote browser presentation are development
  presenters; native macOS presentation is intentionally deferred.
- The browser transport keeps only the newest frame and has no compression,
  authentication, TLS, input path, or hot reload. Its HTTP listener is
  loopback-only unless the developer explicitly selects another bind address.
- Software drawing is limited to clear and filled rectangles in RGB888 format.
- Global initialization is deliberately constant-only.
- Semantic analysis validates types and conservative return coverage but does
  not yet model integer overflow or division-by-zero behavior beyond literal
  range checking.
- The backend emits straightforward stack-based IR and relies on Clang's LLVM
  optimizer; it is inspectable rather than size-optimal.
