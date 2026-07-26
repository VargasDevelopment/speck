# Byte budget

The eventual maximum distribution size is **1,474,560 bytes**, the capacity of
a 1.44 MB floppy disk.

The verified `examples/crumb_bum.spk` build is **4,976 bytes** on the audited
Ubuntu 26.04 x86_64 environment. It is a stripped ELF position-independent
executable with no debug sections.

The current Linux development artifact is dynamically linked. Its interpreter
is `/lib64/ld-linux-x86-64.so.2`, and its only reported shared-library
dependency is `libc.so.6`. LLVM and the Speck compiler are build-time tools and
are not part of the game executable, but the host dynamic loader and C library
remain external requirements.

## Measured composition

These figures come from GNU `size`, `nm`, `file`, `ldd`, and `readelf`:

| Measurement | Bytes | Meaning |
| --- | ---: | --- |
| Final ELF file | 4,976 | Entire development executable on disk |
| Linked `.text` | 518 | Speck, CRuMB, and compiler/linker startup machine code |
| Linked `.init`, `.fini`, and `.plt` | 88 | Dynamic-process and linkage code |
| Generated game object code | 195 | Pre-link `.text` attributable to Speck output |
| Generated game data/BSS/constants | 16 | Pre-link mutable state and float constant |
| CRuMB function code | 96 | Pre-link sum of CRuMB function sections |
| CRuMB constants/format strings | 28 | Pre-link read-only data |
| Debug information | 0 | No debug sections; final file is stripped |
| `.comment` | 104 | Compiler-identification metadata |

The remaining bytes are ELF headers, program and section tables, dynamic symbol
and string tables, relocations, loader metadata, alignment, and other link-time
overhead. Pre-link object totals do not add directly to the final ELF because
LLD merges sections, removes unused content, and adds process-startup and
dynamic-linking structures.

This development artifact is **not proof of final standalone floppy-disk
compliance**. A byte count below the limit does not establish that the eventual
game, assets, audio, platform code, or self-contained runtime will fit, and a
dynamically linked executable is not a self-sufficient distribution.
