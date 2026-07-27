# Roadmap

Each milestone remains a narrow, executable vertical slice. Programs we
genuinely try to write determine which language feature earns a place.

1. **Portable framebuffer and presentation — complete.** The same 320x180 RGB
   framebuffer has deterministic PPM, remote browser, and native macOS Cocoa
   presenters.
2. **Focused language ergonomics — complete.** Explicit numeric conversions,
   top-level constants, short-circuit Boolean composition, proper void
   functions, and numeric compound assignments support natural visual sketches.
3. **Presenter-independent keyboard input — complete.** Speck's stable digital
   key API uses one portable CRuMB state through Cocoa and browser event
   translation while headless PPM remains input-free.
4. **Manual controllable-character and platforming experiments — next.** Write a
   controllable rectangle, then the smallest BOOTS movement prototype: horizontal
   movement, gravity, one floor, one jump, and one powered-boot variation. Record
   friction before adding more infrastructure.
5. **Assets, audio, and larger data abstractions — deferred.** Their exact shapes
   should come from concrete pressure in those manually authored programs.
6. Release-mode size optimization.
7. Cross-platform or contest-target build.
8. First complete game.

Host-native compiler/runtime portability is proven on Linux x86-64 and macOS
ARM64; cross-compilation remains deferred. Syntax highlighting, completion,
language-server work, and numeric aliases also remain deferred while manual
programming experience accumulates. See the [friction log](friction-log.md).
