# Roadmap

Each milestone should remain a narrow, executable vertical slice.

1. Compiler and headless CRuMB vertical slice.
2. Minimal graphical platform backend.
3. Input and primitive drawing.
4. Fixed-capacity arrays and entity pools.
5. Procedural sprites.
6. Synthesized sound.
7. Release-mode size optimization.
8. Cross-platform or contest-target build.
9. First complete game.

Milestones 1 and 2 are complete. The portable software framebuffer now has a
deterministic PPM presenter, a development-only remote browser presenter, and a
minimal native macOS Cocoa presenter. All three use the same private presenter
hooks and leave the eventual contest platform replaceable. The next recommended
slice is minimal presenter-independent keyboard input state on macOS, with no
audio, asset, or backend-framework expansion. Host-native compiler/runtime
portability is proven on Linux x86-64 and macOS ARM64; cross-compilation remains
deferred.
