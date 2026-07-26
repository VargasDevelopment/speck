# Byte budget

The eventual maximum distribution size is **1,474,560 bytes**, the capacity of
a 1.44 MB floppy disk.

On the audited Ubuntu 26.04 x86_64 environment, the verified software-graphics
infrastructure example is **5,752 bytes**, and the existing `crumb_bum` example
is **5,768 bytes** after linking the framebuffer and PPM presenter. Both are
stripped ELF position-independent executables with no debug sections.

The current Linux development artifact is dynamically linked. Its interpreter
is `/lib64/ld-linux-x86-64.so.2`, and its only reported shared-library
dependency is `libc.so.6`. LLVM and the Speck compiler are build-time tools and
are not part of the game executable, but the host dynamic loader and C library
remain external requirements.

## Measured macOS ARM64 artifact

On the audited macOS 26.5.2 ARM64 host, `examples/framebuffer_rect.spk` produces
a **34,016-byte** ARM64 Mach-O PIE. `otool -L` reports one dynamic-library load,
`/usr/lib/libSystem.B.dylib`. The final PPM remains 172,815 bytes: its 15-byte
P6 header is followed by exactly 320x180x3 RGB bytes.

This Mach-O size is recorded as its own host measurement. It must not be read as
a direct size regression against the Linux ELF figures: Mach-O and ELF have
different headers, load commands, alignment, stripping behavior, startup code,
dynamic loaders, and system-library linkage conventions.

The macOS measurements were collected with:

```sh
cargo run -- build examples/framebuffer_rect.spk
file build/framebuffer_rect
otool -hv build/framebuffer_rect
otool -L build/framebuffer_rect
wc -c build/framebuffer_rect build/frame.ppm
```

## Measured composition

These figures come from GNU `size`, `nm`, `file`, `ldd`, and `readelf`:

| Measurement | Bytes | Meaning |
| --- | ---: | --- |
| Graphics example ELF file | 5,752 | Entire development executable on disk |
| Linked `.text` | 970 | Speck, CRuMB, framebuffer, PPM, and startup machine code |
| Linked `.init`, `.fini`, and `.plt` | 136 | Dynamic-process and linkage code |
| Generated graphics-example code | 99 | Pre-link `.text` attributable to Speck output |
| CRuMB lifecycle/debug code | 170 | Pre-link sum of lifecycle function sections |
| CRuMB framebuffer code | 390 | Pre-link clear, rectangle, and pixel-view functions |
| PPM presenter code | 133 | Pre-link dependency-free P6 encoder |
| Framebuffer BSS | 172,800 | Runtime memory; zero-initialized and not stored in the ELF file |
| Final `frame.ppm` | 172,815 | 15-byte header plus 320x180x3 pixel bytes |
| Debug information | 0 | No debug sections; final file is stripped |
| `.comment` | 104 | Compiler-identification metadata |

The remaining bytes are ELF headers, program and section tables, dynamic symbol
and string tables, relocations, loader metadata, alignment, and other link-time
overhead. Pre-link object totals do not add directly to the final ELF because
LLD merges sections, removes unused content, and adds process-startup and
dynamic-linking structures.

`frame.ppm` is a development presentation artifact, not a file that must ship
with the executable. The framebuffer's BSS cost matters to runtime memory, even
though it adds essentially no pixel payload to the executable on disk.

This development artifact is **not proof of final standalone floppy-disk
compliance**. A byte count below the limit does not establish that the eventual
game, assets, audio, platform code, or self-contained runtime will fit, and a
dynamically linked executable is not a self-sufficient distribution.
