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
  -> LLD link with CRuMB object
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
finite loop, and exposes only initialization, frame delta, debug output, and
shutdown. The headless loop executes five deterministic 1/60-second frames.

## Current limitations

- Linux/x86_64 is the only tested target.
- Output is dynamically linked against the host C library.
- CRuMB uses `printf` solely for development verification.
- There is no graphics, input, audio, asset, allocation, or platform backend.
- Global initialization is deliberately constant-only.
- Semantic analysis validates types and conservative return coverage but does
  not yet model integer overflow or division-by-zero behavior beyond literal
  range checking.
- The backend emits straightforward stack-based IR and relies on Clang's LLVM
  optimizer; it is inspectable rather than size-optimal.
