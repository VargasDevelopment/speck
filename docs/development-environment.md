# Development environment

This file records the native environments used for Speck's initial Linux slice
and the macOS ARM64 portability slice. Supported host-native builds are Linux
x86-64 and macOS ARM64; cross-compilation is not implemented.

## macOS ARM64 audit

Audit performed on 2026-07-25 on the local Apple Silicon checkout.

### Host

- Operating system: macOS 26.5.2 (build 25F84), Darwin 25.5.0
- Architecture: ARM64; `rustc -vV` host `aarch64-apple-darwin`
- Rust compiler: Homebrew `rustc` 1.97.1, built with LLVM 22.1.8
- Cargo: Homebrew Cargo 1.97.1
- Apple Clang: 21.0.0 (`clang-2100.1.1.101`)
- Clang-reported target: `arm64-apple-darwin25.5.0`
- Apple linker: `ld` project 1267
- macOS SDK: 26.5 at the active `xcrun --show-sdk-path` location

Rust and Cargo were initially absent. At the user's direction they were
installed with:

```sh
brew install rust
```

No packages were otherwise installed, and no system configuration or elevated
privileges were used.

### Tool status

| Tool | Status on this Mac |
| --- | --- |
| `rustc` | Homebrew 1.97.1; host `aarch64-apple-darwin` |
| Cargo | Homebrew 1.97.1 |
| Apple Clang | 21.0.0, found by `xcrun --find clang` |
| macOS SDK | 26.5, found by `xcrun --show-sdk-path` |
| Apple `ld` | Project 1267, found by `xcrun --find ld` |
| Homebrew `llvm-as` | LLVM 22.1.8; not on PATH, found through `brew --prefix llvm` |
| Homebrew `opt` | LLVM 22.1.8; present but not needed by Speck |
| `ld64.lld` | Missing; not required because native macOS linking uses Apple `ld` through Clang |

The selected validation path was Homebrew `llvm-as` 22.1.8 followed by Apple
Clang 21.0.0. The versions proved bitcode-compatible for the generated module.
If `llvm-as` is absent or its output cannot be consumed by the selected Clang,
the build uses Clang once to validate and emit `.bc`, then compiles the textual
`.ll` directly. That no-`llvm-as` fallback was also executed successfully on
this Mac with Homebrew removed from the child process's PATH. No manually
copied LLVM data-layout string is emitted.

### Portability problems found in the Linux-only implementation

- `llvm-as` and `clang` were invoked by fixed command names with no discovery
  or actionable candidate list.
- LLVM IR contained no selected-host target triple.
- Every build required standalone `llvm-as`, although Apple's command-line
  tools do not include it.
- Every link forced LLD and ELF-only `--gc-sections`/`--strip-all` flags.
- Object and executable naming was implicit rather than part of a host policy.
- CRuMB's portable lifecycle code also contained the process entry point, so
  the platform boundary was not explicit.
- Documentation assumed ELF, glibc, and the Linux dynamic loader globally.

The runtime source root already used Cargo's manifest directory and did not
hardcode a machine-specific absolute path. Both supported hosts use `.o`
objects and extensionless executables, but those choices now live in the host
policy rather than being accidental assumptions. Framebuffer ownership,
rasterization, PPM serialization, deterministic stepping, and the public C ABI
were already platform-neutral and remain unchanged.

### Repeat the macOS audit

```sh
sw_vers
uname -m
rustc --version --verbose
cargo --version
xcrun --find clang
xcrun --show-sdk-path
xcrun --sdk macosx --show-sdk-version
clang --version
"$(brew --prefix llvm)/bin/llvm-as" --version
ld -v
```

### Native Cocoa presenter audit

The first native-presentation slice was built and exercised on 2026-07-26 with
the same ARM64 host, Apple Clang 21.0.0, Apple `ld` 1267, and macOS 26.5 SDK.
Apple Clang compiled the portable sources as strict C11 and compiled only
`runtime/crumb/present_cocoa.m` as Objective-C. The Cocoa link selected AppKit
and CoreGraphics; `otool -L` also records their Apple system support loads,
CoreFoundation, `libobjc`, and `libSystem`.

The interactive command is:

```sh
cargo run -- run examples/moving_rectangle.spk
```

It produced a 53,400-byte ARM64 Mach-O before keyboard input was added. A three-frame bounded run,
window display, changing rectangle, standard window-close shutdown, SIGINT
shutdown, PPM regression, and browser-stream regression were verified without
installing packages or changing system configuration. The Cocoa code is not
selected for Linux, PPM, or stream artifacts.

### Presenter-independent keyboard input audit

The keyboard slice was completed on 2026-07-26 with the same Rust 1.97.1,
Cargo 1.97.1, Apple Clang/clang-format 21.0.0, Apple `ld` 1267, macOS 26.5 SDK,
and ARM64 target. No packages, frameworks, or system configuration were added.
All C11 and Objective-C sources passed `-Wall -Wextra -Wpedantic -Werror` and
clang-format 21's strict dry run.

The comparable native `moving_rectangle` artifact is now 53,896 bytes, +496
bytes from its pre-input 53,400-byte measurement. Framework loads are unchanged.
The full 68-test current-host suite, bounded PPM/browser/native runs, real Chrome
Space/Escape input, held-state movement over the browser control path, and
parent/child clean shutdown were exercised. A macOS-only Objective-C harness
sent AppKit events through the real Cocoa view and verified A, Escape, repeat
suppression, key-up, and delegate focus-loss release. The command-line Cocoa
process launched and stopped cleanly, but the available desktop automation
could not attach physical key events to that unbundled process; native physical
key interaction remains an explicit user acceptance command rather than a
claimed automated observation.

## Original Linux x86-64 audit

The following audit was performed on 2026-07-25 before bootstrapping the first
Speck vertical slice.

### Host

- Operating system: Ubuntu 26.04 LTS (Resolute Raccoon), Linux 7.0.0-27-generic
- Architecture and compiler target: x86_64 (`x86_64-linux-gnu`)
- C compiler: GCC/`cc` 15.2.0
- System linker: GNU ld 2.46
- C library: glibc 2.43
- Make: GNU Make 4.4.1
- Git: 2.53.0
- `/home/joseph/code` was not inside a Git repository at audit time.

### Initial toolchain status

The initial machine image did not contain the development toolchain required
to build Speck. No matching binaries were found on `PATH`, in
`/usr/local/bin`, `/opt`, `/snap/bin`, or the user's Cargo bin directory.

| Tool | Status | Ubuntu 26.04 package candidate |
| --- | --- | --- |
| Rust compiler (`rustc`) | Missing | 1.93.1ubuntu1 |
| Cargo | Missing | 1.93.1ubuntu1 |
| Clang | Missing | 21.1.6 |
| `llvm-config` | Missing | LLVM 21.1.6 packages |
| `llc` | Missing | LLVM 21.1.6 packages |
| `opt` | Missing | LLVM 21.1.6 packages |
| `llvm-as` | Missing | LLVM 21.1.6 packages |
| LLD / `ld.lld` | Missing | 21.1.6 |
| CMake | Missing | 4.2.3 |
| pkg-config | Missing | 2.5.1 |

The available GCC toolchain was sufficient to compile ordinary C, but it could
not replace Cargo for the Rust compiler or provide the required LLVM IR
validation and compilation pipeline.

### Verified toolchain after installation

The toolchain was re-audited after the required packages were installed:

| Tool | Installed version |
| --- | --- |
| `rustc` | 1.93.1 |
| Cargo | 1.93.1 |
| rustfmt | 1.8.0 |
| Clippy | 0.1.93 |
| Clang | Ubuntu 21.1.8 |
| `llvm-config` | 21.1.8 |
| `llc` | Ubuntu LLVM 21.1.8 |
| `opt` | Ubuntu LLVM 21.1.8 |
| `llvm-as` | Ubuntu LLVM 21.1.8 |
| LLD / `ld.lld` | Ubuntu LLD 21.1.8 |
| C compiler (`cc`) | GCC 15.2.0 |
| CMake | 4.2.3 |
| pkg-config | 2.5.1 |
| clang-format | Ubuntu 21.1.8 |

### Graphical backend

This is a headless TTY session: `DISPLAY` and `WAYLAND_DISPLAY` are unset and
`XDG_SESSION_TYPE=tty`. A DRM render/card device exists, but X11, Wayland,
SDL2, GLFW, and Raylib remain undiscoverable after installing pkg-config. A
headless CRuMB v0 is therefore the realistic first backend. No graphics
packages are required for the first vertical slice.

### Installation record

The user installed the required Ubuntu packages with:

```sh
sudo apt update
sudo apt install rustc cargo clang llvm llvm-dev lld cmake pkg-config
sudo apt install rustfmt rust-clippy clang-format
```

The installation and subsequent verification completed successfully. The
project does not depend on CMake or pkg-config for this slice. It uses Cargo for
the compiler, `llvm-as` for IR validation, Clang for object generation, and LLD
for the final link.

### Browser-presenter slice

The development browser presenter was implemented and audited on this same
Linux host on 2026-07-26. It required no system packages, display server, or
elevated configuration. The native runtime side uses only C11 and POSIX TCP
APIs shared by Linux and macOS. The Rust HTTP server and frame decoder use the
standard library; `ctrlc` 3.5.2 is the one direct Cargo dependency added for
portable development-process interruption handling.

The managed development sandbox blocks loopback socket creation, so socket and
end-to-end viewer tests were executed outside that sandbox with the same user
account. The server itself was then manually verified on `127.0.0.1`: the HTML
endpoint returned 200, a live frame returned exactly 172,800 RGB bytes, a
finite run shut down cleanly, and an interrupted run left no child game
process. No real browser or graphical backend was required for the test suite.

The audit can be repeated with:

```sh
rustc --version
cargo --version
clang --version
llvm-config --version
llc --version
opt --version
llvm-as --version
ld.lld --version
cmake --version
pkg-config --version
```
