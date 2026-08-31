# Contributing to Speck

Thanks for your interest in Speck. The project is deliberately small, so
changes should stay focused and earn their complexity through concrete programs
or platform needs.

## Getting started

Speck requires Rust/Cargo and Clang. Linux also requires LLD. See the
[development environment guide](docs/development-environment.md) for platform
details.

Before submitting a change, run:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

## Proposing changes

- Open an issue first for substantial language, runtime, or architecture
  changes so the direction can be discussed before implementation.
- Keep pull requests narrow and explain the behavior they add or change.
- Add or update tests and documentation when behavior changes.
- Preserve Speck's small-language and small-runtime constraints; the
  [design principles](docs/design-principles.md) explain the intended tradeoffs.

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE).
