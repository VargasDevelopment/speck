# Roadmap

Speck is a small native language for little games and visual experiments. Keep
binaries small and CRuMB narrow. A complete 1.44 MB game distribution remains
an optional challenge, rather than a contest deadline or a requirement for
every project. New features should earn their place through real programs.

## Completed language and development foundations

- **Consistent semantics.** Constant evaluation has shared ownership across
  declarations and array lengths. A checked-program boundary separates semantic
  validation from LLVM emission. Initializers evaluate effects in source order;
  numeric overflow, division, remainder, and conversion behavior have documented
  contracts and executable boundary coverage.
- **Programs composed from files.** Relative imports and qualified names support
  shared functions, constants, globals, and nominal structs. Canonical file
  identity handles shared dependencies; diagnostics retain their original files.
  Syntax, import, type, and constant evaluation have explicit resource limits.
- **Useful value types.** Fixed arrays and named structs compose recursively and
  cross function boundaries with value-copy semantics and checked indexing.
  Explicit conversions, void functions, short-circuit Boolean expressions,
  compound assignment, integer remainder, and range loops remain small features
  with clear contracts.
- **An edit/run loop.** `speck check` analyzes source without native tools or
  artifacts. The VS Code / Cursor extension supplies syntax highlighting.
  `speck dev --watch` rebuilds and restarts after root or import edits, recovers
  from invalid or missing files, and preserves the viewer URL. Development
  bounds and integer arithmetic failures identify their Speck source locations.
- **Native execution.** Linux x86-64 and macOS ARM64 builds share framebuffer and
  keyboard contracts across deterministic PPM, browser development, and Cocoa
  presentation. Compiler installations carry their CRuMB sources, and process
  shutdown and compiler robustness have dedicated regression coverage.

## Evidence from external programs

BOOTS, the PULSE arcade game, and the TIDELINES visual sketch live in a separate
showcase project. Actual games stay outside this compiler repository; small
regression fixtures belong here when they demonstrate language or runtime
behavior. BOOTS and Boots Ascent also provide earlier, substantial game code.

These programs have already justified narrow additions: array arguments and
returns make game-owned aggregate helpers reusable, while TIDELINES' periodic
contours selected `sin(f32) -> f32`. Game-owned bitmap labels expose possible
text-authoring friction, but do not yet define a runtime string or font API.
Deterministic game checks and visual inspection are useful evidence; they do
not establish a measured human playthrough duration or settle game feel.

## Next check-in: sharing and distribution (priority 6)

The next roadmap phase is explicitly deferred for discussion before work starts:

- Choose the presentation and launch experience for someone receiving a game.
- Verify compiler installation and game launch from clean environments.
- Measure complete distributable sizes, including required runtime libraries,
  and decide whether the optional floppy-size challenge is useful.
- Select another native platform only from actual player needs.

Cross-compilation, package-management infrastructure, a language server,
completion, broader graphics, and audio remain proposals requiring concrete
use cases. This is an outcome roadmap, not a promised feature list or schedule.

The [language reference](language.md) owns implemented behavior, the
[architecture map](architecture.md) describes ownership, and the
[friction log](friction-log.md) records the programs behind feature decisions.
