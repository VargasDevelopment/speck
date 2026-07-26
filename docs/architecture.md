# Architecture

The first slice is one dependency-free Rust crate plus a small C runtime:

```text
.spk source
  -> lexer (tokens with byte spans)
  -> parser (AST)
  -> semantic/type validation
  -> textual LLVM emitter
  -> llvm-as validation and bitcode
  -> Clang native object
  -> LLD link with CRuMB objects
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

The public C ABI provides fixed width, height, channel, stride, and byte-count
constants plus immutable framebuffer pixel access. A future X11 presenter can
consume this same pixel view and replace the PPM presentation call without
changing Speck programs or the rasterizer.

## Current limitations

- Linux/x86_64 is the only tested target.
- Output is dynamically linked against the host C library.
- CRuMB uses `printf` solely for development verification.
- There is no window, input, audio, asset, allocation, or interactive platform
  backend. PPM output is the only presenter.
- Software drawing is limited to clear and filled rectangles in RGB888 format.
- Global initialization is deliberately constant-only.
- Semantic analysis validates types and conservative return coverage but does
  not yet model integer overflow or division-by-zero behavior beyond literal
  range checking.
- The backend emits straightforward stack-based IR and relies on Clang's LLVM
  optimizer; it is inspectable rather than size-optimal.
