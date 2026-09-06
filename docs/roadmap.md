# Roadmap

Speck is a small native language for little games and visual experiments. Keep
binaries small and CRuMB narrow; a complete 1.44 MB game distribution remains
an optional challenge, rather than a contest deadline or a requirement for
every project. New features should earn their place through programs people
actually want to write.

## Foundation already in place

- Linux x86-64 and macOS ARM64 host-native builds, with deterministic PPM,
  browser development, and native Cocoa presentation.
- A shared framebuffer and keyboard-input contract across presenters.
- Explicit conversions, constants, void functions, short-circuit Boolean
  expressions, compound assignment, integer remainder, and range loops.
- Fixed arrays and named structs, including nested aggregate composition,
  checked indexing, and value-copy semantics.
- BOOTS and the larger Boots Ascent already exercise substantial game code.
  The first movement prototype is no longer the next milestone.
- Compiler correctness and installation fixes: lexical shadowing in bounds
  checks, bounded syntax nesting and struct graph traversal, clean early quit,
  and CRuMB resources that travel with the compiler.

## Next outcomes

1. **Trust the implemented language.** Consolidate constant evaluation and give
   binding/type decisions clear owners. Consider a checked-program boundary
   where it removes repeated validation. Specify initializer effect order and
   numeric behavior before adding more arithmetic or aggregate features.
2. **Make the edit/run loop pleasant.** Add analysis without Clang through a
   `speck check` command, lightweight syntax highlighting, and predictable
   rebuild/restart on save. Make development runtime failures point to Speck
   source. Completion and a language server can wait for demonstrated need.
3. **Learn from distinct programs.** Continue BOOTS, build a small arcade game,
   and make a visual sketch. Record reproducible friction and promote useful
   examples into the repository. Use that evidence to choose between simple
   modules, arrays at function boundaries, sprites/text, audio, or math helpers.
4. **Make games easy to share.** Clarify presenter selection and host-native
   distribution, test clean installation and launch, and measure binary size.
   Choose another native platform from actual players; cross-compilation and
   package-management infrastructure remain deferred.

These are outcome priorities, not a promised feature list or schedule. The
[language reference](language.md) describes implemented behavior, the
[architecture map](architecture.md) describes current ownership, and the
[friction log](friction-log.md) retains observations from earlier slices.
