# Architecture

The first slice is one dependency-free Rust crate plus a small C runtime:

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
drawing, and shutdown. The headless loop executes five deterministic
1/60-second frames.

CRuMB's graphics path is split by responsibility:

- `framebuffer.c` owns a packed, row-major 320x180 RGB framebuffer and implements
  clear and clipped filled-rectangle rasterization.
- `present_ppm.c` is the current headless presenter. After every `spk_draw`, it
  overwrites `build/frame.ppm` with a dependency-free binary P6 PPM image.
- `crumb.c` owns lifecycle sequencing and calls the presenter; it does not know
  the framebuffer's pixel-writing rules.
- `platform/posix_main.c` contains only the program entry point used by both
  supported hosts. The toolchain selects it as the platform source.

The public C ABI provides fixed width, height, channel, stride, and byte-count
constants plus immutable framebuffer pixel access. The ABI did not change for
host portability. A future native presenter can consume this same pixel view
and replace the PPM presentation call without changing Speck programs or the
rasterizer.

## Current limitations

- Native host support is limited to Linux x86-64 and macOS ARM64.
- Output is dynamically linked against the host C library.
- CRuMB uses `printf` solely for development verification.
- There is no window, input, audio, asset, allocation, or interactive platform
  backend. PPM output is the only presenter; native macOS window presentation
  is intentionally deferred.
- Software drawing is limited to clear and filled rectangles in RGB888 format.
- Global initialization is deliberately constant-only.
- Semantic analysis validates types and conservative return coverage but does
  not yet model integer overflow or division-by-zero behavior beyond literal
  range checking.
- The backend emits straightforward stack-based IR and relies on Clang's LLVM
  optimizer; it is inspectable rather than size-optimal.
