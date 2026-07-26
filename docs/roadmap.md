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

Milestone 1 is complete. The headless software framebuffer is infrastructure
for milestone 2, but milestone 2 still requires an actual presentation window;
the current slice deliberately stops at deterministic PPM output.
