# Development environment

Environment audit performed on 2026-07-25 before bootstrapping the Speck
vertical slice.

## Host

- Operating system: Ubuntu 26.04 LTS (Resolute Raccoon), Linux 7.0.0-27-generic
- Architecture and compiler target: x86_64 (`x86_64-linux-gnu`)
- C compiler: GCC/`cc` 15.2.0
- System linker: GNU ld 2.46
- C library: glibc 2.43
- Make: GNU Make 4.4.1
- Git: 2.53.0
- `/home/joseph/code` was not inside a Git repository at audit time.

## Initial toolchain status

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

## Verified toolchain after installation

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

## Graphical backend

This is a headless TTY session: `DISPLAY` and `WAYLAND_DISPLAY` are unset and
`XDG_SESSION_TYPE=tty`. A DRM render/card device exists, but X11, Wayland,
SDL2, GLFW, and Raylib remain undiscoverable after installing pkg-config. A
headless CRuMB v0 is therefore the realistic first backend. No graphics
packages are required for the first vertical slice.

## Installation record

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
