# Architecture

Speck remains one Rust crate plus a small C runtime. The compiler has one direct
development-process dependency (`ctrlc`); generated games do not link Rust:

```text
.spk source
  -> lexer (tokens with byte spans)
  -> parser (AST)
  -> semantic/type validation and compile-time constant evaluation
  -> host-tagged textual LLVM emitter
  -> compatible llvm-as, or Clang-direct, validation and bitcode
  -> Clang native object
  -> native link with selected CRuMB presenter objects
       (LLD on Linux, Apple ld and system frameworks for Cocoa on macOS)
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

The validated AST distinguishes `ValueType` (scalars, named structs, and fixed
arrays) from `ReturnType` (a value type or `void`). Array lengths retain their
source form until semantic analysis resolves positive literals and `i32`
constants.
Semantic analysis collects constant, global, and function names before checking
bodies. A small dependency-walking constant evaluator annotates top-level
constants and mutable-global initializers with scalar or aggregate values,
detects cycles and checked-expression errors, and lets LLVM emit native
initializers. This remains small enough that a separate HIR would duplicate
rather than simplify the pipeline.

Struct declarations are collected before value and function checking, so type
use and literals can refer forward within the module. Compiler-only metadata
keeps each field's source name, type, and declaration index. A graph walk
rejects recursive value layout. Literal checking builds a name-to-initializer
view for duplicate, missing, and unknown diagnostics while LLVM emission always
uses declaration order.

Boolean `&&` and `||` lower directly to branches and merge phi nodes. Numeric
compound assignment lowers to one target load, one right-expression evaluation,
the existing typed arithmetic instruction, and one store. `f32`-to-`i32`
conversion branches around the potentially unsafe `fptosi`: NaN, high, and low
paths produce zero or a clamp value, and only a proven in-range path executes
the conversion. User functions and all effect-only CRuMB declarations emit
actual LLVM `void`, `call void`, and `ret void` forms.

Assignment targets now retain expression structure. Semantic lvalue validation
recursively walks any alternating sequence of indexed elements and struct
fields back to its root binding, carries the selected type and root mutability
forward, and rejects non-addressable expressions or any write rooted in
`const`. LLVM lowering performs the same validated path walk to produce one
pointer into nested storage. Fixed arrays lower to ordinary `[N x T]` LLVM
values and storage; local literals use `insertvalue`, globals use native
aggregate initializers, and element addresses use `getelementptr`.

Named records lower to LLVM named aggregate types such as
`%spk_struct_Platform = type { i32, i32, i32, i32 }`. Locals and globals use
that value type directly. Field lvalues use their declaration index in
`getelementptr`, and field reads from returned rvalues use `extractvalue`.
Function parameters and returns carry the aggregate by value in the generated
LLVM signature. No target size, alignment, or data layout is hard-coded; the
host-selected Clang remains authoritative.

Array and struct lowering is recursive rather than special-cased by aggregate
shape. An array of structs is `[N x %spk_struct_Name]`; a fixed-array field is
the same `[N x T]` value nested in the named record. Constant evaluation and
LLVM constant emission recurse through the declared element and field types,
so immutable nested level data becomes a native constant initializer. Loading
an indexed struct produces a value copy, while an assignment path retains the
address of the root storage and descends with checked array GEPs and fixed field
GEPs. No compiler or runtime reference value is introduced.

Each dynamic array access emits signed nonnegative and upper-bound comparisons.
The valid branch alone executes an `inbounds getelementptr`; the failure branch
calls `crumb_bounds_fail(index, length)` and is unreachable afterward. The
CRuMB helper is a 39-byte function in the audited Linux object plus its
diagnostic string. Link-section garbage collection removes it from programs
that never perform checked indexing.

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
loop, and exposes initialization, frame delta, debug output, software drawing,
digital-key queries, orderly quit requests, and shutdown. A normal headless
build executes five deterministic 1/60-second frames without wall-clock pacing
or host input. Browser development and native Cocoa builds are paced and
unbounded by default, with the same private finite override for tests.

The public ABI also contains the deliberately narrow array-bounds failure hook.
It reports the invalid index and fixed length to standard error and terminates;
it does not introduce an exception, allocator, generalized panic object, or
runtime array metadata.

CRuMB's graphics path is split by responsibility:

- `framebuffer.c` owns a packed, row-major 320x180 RGB framebuffer and implements
  clear and clipped filled-rectangle rasterization.
- `input.c` owns fixed current, pressed, and released bytes for CRuMB's eleven
  portable key identifiers plus the runtime quit flag. It allocates nothing,
  bounds-checks every public query, and can be manipulated directly by tests.
- `present_ppm.c` is selected for normal builds. After every `spk_draw`, it
  overwrites `build/frame.ppm` with a dependency-free binary P6 PPM image.
- `present_stream.c` is selected only by `speck dev`. It connects to a loopback
  TCP listener created by the development host, sends length-checked,
  sequence-numbered complete RGB frames, and receives separate fixed-size input
  records over the reverse direction of that socket.
- `present_cocoa.m` is selected only by `speck run` on macOS ARM64. It owns the
  AppKit window, native-key translation, event pumping, focus-loss release,
  CoreGraphics image presentation, backing-pixel scaling, and window-close
  request.
- `crumb.c` owns lifecycle sequencing and calls four private presenter hooks:
  initialize, poll, present, and shut down. Poll and present report continue,
  clean stop, or error. CRuMB knows neither platform event values nor the
  selected presenter's implementation.
- `platform/posix_main.c` supplies the program entry point and the private
  SIGINT request flag used by both supported hosts. The signal handler performs
  no work beyond setting `sig_atomic_t`; `crumb.c` observes it between frames,
  shuts down the selected presenter, and restores the previous handler.

The public C ABI provides fixed framebuffer dimensions and pixel access, the
typed functions emitted by Speck, and the four input/quit functions. Stable key
numbers live only in CRuMB's private header and the compiler's predefined
constants. Browser codes, HTTP, TCP, AppKit, `NSEvent`, and macOS hardware codes
are absent from the public header and Speck semantics. Every presenter consumes
the same framebuffer and produces the same portable input state, so a future
contest presenter can replace PPM, stream, or Cocoa without changing Speck
programs, drawing operations, or the rasterizer.

The interactive frame lifecycle is deliberately explicit:

1. Clear the previous frame's `pressed` and `released` flags.
2. Call the selected presenter's nonblocking poll hook and apply all pending
   transitions.
3. Run `spk_update(dt)` and `spk_draw` while input state remains stable.
4. Present the completed framebuffer.
5. Stop after that completed frame if Speck called `quit()`, or before another
   update when close, disconnect, or SIGINT was observed.

If a down and up transition are both drained in step 2, `pressed` and
`released` are both true for that frame while `down` is false. Repeated events
that do not change held state set no transition flag. The PPM poll hook is a
no-op, preserving deterministic five-frame output.

## Native Cocoa presenter

`speck run game.spk` selects Cocoa only for a native macOS ARM64 host and emits
separately named `_native` IR, bitcode, objects, and executable artifacts. The
toolchain compiles `present_cocoa.m` explicitly as Objective-C and links AppKit
and CoreGraphics only for that variant. Apple Clang's framework autolinking also
records CoreFoundation, `libobjc`, and `libSystem` as direct system loads. PPM,
browser-development, and normal Linux builds never compile the `.m` file or see
Apple framework arguments.

The presenter creates one titled, closable, miniaturizable, resizable `NSWindow`
with a first-responder custom `NSView`. Each completed `spk_draw` is presented synchronously:
the view makes a `CGImage` over CRuMB's packed RGB888 bytes, disables
CoreGraphics interpolation and antialiasing, and draws into a centered
backing-pixel rectangle. When at least one native-size framebuffer fits, the
scale is the largest fitting integer; smaller windows use a fractional
nearest-neighbor downscale. Unused space is black letterbox area. AppKit events
are drained by the pre-update poll hook on the main thread. `keyDown:` and
`keyUp:` translate only W/A/S/D, arrows, Space, Enter, and Escape from
presenter-local macOS codes; AppKit repeat events are ignored.
`windowDidResignKey:` releases all held keys. Escape is delivered normally and
is not a presenter-level exit shortcut. `windowWillClose` releases keys and
converts the close button into the private clean-stop result instead of
terminating the process inside Cocoa.

Both interactive presenters use a monotonic deadline advanced by 16,666,667 ns.
Frame work consumes part of the interval, so CRuMB sleeps only for the remaining
time instead of sleeping a fixed duration after every frame. A deadline missed
by more than one interval is reset rather than causing an extended catch-up
burst. The Speck-visible `dt` remains the provisional fixed `1/60`; it is
simulation time, not a measurement of wall-clock work or display refresh.

## Development browser presenter

`speck dev` is a host-side orchestration mode, not a different language. It
builds a separately named `_dev` native executable whose CRuMB objects contain
the stream presenter instead of the PPM presenter. The compiler process creates
two listeners:

```text
native Speck `_dev` process
  <-> private loopback TCP frame/control protocol
  <-> Rust development host (validates and retains latest complete frame)
  <-> localhost HTTP long polling and bounded input POSTs
  <-> generic HTML canvas viewer
```

The binary frame/control channel and inherited game stdout/stderr are separate,
so debug logs cannot corrupt framing. Game-to-host frame bytes are unchanged.
Host-to-game controls use an 8-byte `SPKI` record described in the viewer
documentation. The server retains only the latest complete frame. A slow
browser may skip frames, but it cannot observe a partial frame. HTTP long
polling and narrow input POSTs avoid adding a WebSocket framework. The viewer
disables smoothing and chooses integer scaling whenever the viewport can fit at
least one native-size framebuffer.

One browser client owns input at a time. A 250 ms heartbeat renews a one-second
host lease; explicit blur/hide/page-exit release messages and lease expiry both
send `release-all`. This supplies deterministic disconnect behavior despite
the viewer's stateless HTTP requests. Unsupported keys are ignored, client IDs
and message shapes are validated, request bodies are capped at 128 bytes, and
ordinary browser control requests are serialized to preserve transition order.

The normal build path selects only `present_ppm.c`; it does not compile or link
the TCP presenter. The HTML, HTTP server, Ctrl-C handler, and orchestration code
live in the Speck compiler executable, which is already a development-time
tool. They do not appear in a generated normal game executable.

## Current limitations

- Native host support is limited to Linux x86-64 and macOS ARM64.
- Output is dynamically linked against host system libraries and, for Cocoa,
  Apple system frameworks.
- CRuMB uses `printf` solely for development verification.
- Keyboard input is limited to eleven fixed digital keys. There is no text,
  mouse, controller, rebinding, or arbitrary-key API.
- The Cocoa presenter has no audio, assets, allocation API, display-link
  synchronization, full-screen mode, or game-specific behavior. It redraws at
  a deadline-paced nominal 60 Hz rather than synchronizing to the monitor's
  refresh, so frame delivery can drift, tear, or skip under load.
- Native window presentation is macOS ARM64-only. Linux retains PPM and browser
  development presentation and never attempts to compile Objective-C.
- The browser transport keeps only the newest frame and has no compression,
  authentication, TLS, persistent browser connection, or hot reload. Its HTTP listener is
  loopback-only unless the developer explicitly selects another bind address.
- Software drawing is limited to clear and filled rectangles in RGB888 format.
- Global initialization is deliberately compile-time-only; local constants and
  general compile-time execution do not exist.
- Constant evaluation diagnoses integer overflow and division by zero. Runtime
  integer overflow and division-by-zero behavior remain provisional.
- The backend emits straightforward stack-based IR and relies on Clang's LLVM
  optimizer; it is inspectable rather than size-optimal.
