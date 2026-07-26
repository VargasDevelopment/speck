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

Milestone 1 is complete. Its portable software framebuffer now has both the
deterministic PPM presenter and a development-only remote browser presenter.
These validate pixels and live presentation without defining the eventual
contest platform. Milestone 2 remains the next product-facing slice: a minimal
native macOS presenter over the same framebuffer boundary, with no drawing API
or Speck-language changes. Host-native compiler/runtime portability is proven
on Linux x86-64 and macOS ARM64; cross-compilation remains deferred.
