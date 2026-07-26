# Roadmap

Each milestone remains a narrow, executable vertical slice. Programs we
genuinely try to write determine which language feature earns a place.

1. **Native presentation — complete.** The same portable 320x180 framebuffer
   has deterministic PPM, remote browser, and native macOS Cocoa presenters.
2. **Focused language ergonomics — complete.** Explicit numeric conversions,
   top-level constants, short-circuit Boolean composition, proper void
   functions, and numeric compound assignments now support natural
   delta-time-based sketches without expanding the runtime boundary.
3. **Manual visual sketches and language evaluation — next.** Write several
   small animations, generative sketches, and simulations; record friction;
   evaluate whether `i32` and `f32` remain the right user-facing names before
   adding more language surface.
4. **Unified input — deferred until after those sketches.** When resumed, add a
   small presenter-independent keyboard state boundary and prove it through the
   supported presenters without creating a backend framework.
5. Fixed-capacity arrays and entity pools.
6. Procedural sprites.
7. Synthesized sound.
8. Release-mode size optimization.
9. Cross-platform or contest-target build.
10. First complete game.

Host-native compiler/runtime portability is proven on Linux x86-64 and macOS
ARM64; cross-compilation remains deferred. Syntax highlighting, completion,
language-server work, and numeric aliases also remain deferred while manual
programming experience accumulates. See the [friction log](friction-log.md).
