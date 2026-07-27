# Byte budget

The eventual maximum distribution size is **1,474,560 bytes**, the capacity of
a 1.44 MB floppy disk.

On the audited Ubuntu 26.04 x86_64 environment, the verified software-graphics
infrastructure example is **5,816 bytes**, and the existing `crumb_bum` example
is **5,832 bytes** after linking the framebuffer and PPM presenter. Both are
stripped ELF position-independent executables with no debug sections. These
post-presenter-boundary measurements were collected on 2026-07-26.

The current Linux development artifact is dynamically linked. Its interpreter
is `/lib64/ld-linux-x86-64.so.2`, and its only reported shared-library
dependency is `libc.so.6`. LLVM and the Speck compiler are build-time tools and
are not part of the game executable, but the host dynamic loader and C library
remain external requirements.

The separately selected browser-development game executable for
`moving_rectangle.spk` is **7,056 bytes** on this Linux host. It contains the
native TCP frame sender but not the HTTP server or HTML. It is a development
artifact and is not a normal game distribution.

## Measured macOS ARM64 artifact

On the audited macOS 26.5.2 ARM64 host, the native Cocoa build of
`examples/moving_rectangle.spk` is a **53,896-byte** ARM64 Mach-O PIE after the
keyboard-input slice. Its direct
dynamic loads are AppKit, CoreGraphics, CoreFoundation, `libobjc.A.dylib`, and
`libSystem.B.dylib`; all are Apple system components. The executable contains
the Objective-C window/view presenter and the portable framebuffer, but no
Rust, Swift, browser, JavaScript, networking, PPM presenter, or third-party
framework.

The same checkout's deterministic PPM build of `examples/framebuffer_rect.spk`
is **34,704 bytes** and loads only `libSystem.B.dylib`. Its final PPM remains
172,815 bytes: a 15-byte P6 header followed by exactly 320x180x3 RGB bytes.

These Mach-O sizes are recorded as host measurements. They must not be read as
direct size regressions against the Linux ELF figures: Mach-O and ELF have
different headers, load commands, alignment, stripping behavior, startup code,
dynamic loaders, and system-library linkage conventions.

The macOS measurements were collected with:

```sh
cargo run -- build examples/framebuffer_rect.spk
file build/framebuffer_rect
otool -hv build/framebuffer_rect
otool -L build/framebuffer_rect
wc -c build/framebuffer_rect build/frame.ppm

cargo run -- run examples/moving_rectangle.spk --frames 3
file build/moving_rectangle_native
otool -hv build/moving_rectangle_native
otool -L build/moving_rectangle_native
wc -c build/moving_rectangle_native
```

## Keyboard-input slice delta

The previous comparable `moving_rectangle_native` measurement was 53,400
bytes. It is now 53,896 bytes, an increase of approximately **496 bytes**. This
is the portable fixed input state plus Cocoa key translation and focus handling;
no third-party runtime or new system framework was added. The input example
itself is 54,072 bytes because its generated game code calls the new query and
quit functions.

For context, the comparable PPM `moving_rectangle` artifact increased from
50,880 to 51,216 bytes (**+336 bytes**) because every shipped CRuMB variant owns
the small portable state and safe query functions. The development stream
variant increased from 51,336 to 51,912 bytes (**+576 bytes**) because it also
contains the fixed control-record receiver. The Rust HTTP control endpoint,
client ownership/lease policy, and embedded JavaScript are development-tool
code and do not enter normal PPM or Cocoa game executables. Conversely, the
development stream presenter is not linked into those shipped variants.

Direct dynamic loads are unchanged: the Cocoa executable uses AppKit,
CoreGraphics, CoreFoundation, `libobjc`, and `libSystem`; PPM and development
stream executables load only `libSystem`. No third-party runtime dependency was
added.

## Measured composition

These figures come from GNU `size`, `nm`, `file`, `ldd`, and `readelf`:

| Measurement | Bytes | Meaning |
| --- | ---: | --- |
| Graphics example ELF file | 5,816 | Entire normal PPM executable on disk |
| Linked `.text` | 1,007 | Speck, CRuMB, framebuffer, PPM, and startup machine code |
| Linked `.init`, `.fini`, and `.plt` | 136 | Dynamic-process and linkage code |
| Generated graphics-example code | 99 | Pre-link `.text` attributable to Speck output |
| CRuMB lifecycle/debug code | 197 | Pre-link sum of lifecycle function sections |
| CRuMB framebuffer code | 390 | Pre-link clear, rectangle, and pixel-view functions |
| PPM presenter code | 144 | Pre-link presenter hooks and dependency-free P6 encoder |
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

The Rust HTTP server, embedded viewer HTML, and `ctrlc` dependency are compiled
into the Speck development tool, not into generated game executables. A string
audit of the normal graphics executable finds none of the private stream magic
or environment-variable names. Normal builds compile only `present_ppm.c`,
development runs compile only `present_stream.c`, and native runs compile only
`present_cocoa.m` as their respective presenter.

This development artifact is **not proof of final standalone floppy-disk
compliance**. A byte count below the limit does not establish that the eventual
game, assets, audio, platform code, or self-contained runtime will fit, and a
dynamically linked executable is not a self-sufficient distribution.
