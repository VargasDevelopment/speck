# Reliability bug bash

This document records the bounded reliability investigation completed on
2026-07-28 after fixed arrays, value structs, aggregate composition, and range
loops landed. It is a regression contract and a decision log, not a claim that
all possible Speck programs have been proved correct.

## Scope and stopping rule

The investigation was capped at three independently reviewable pull requests:

1. compiler-input robustness and diagnostic-span safety;
2. semantic, lowering, and native-behavior contracts plus fixes for confirmed
   defects;
3. failure-path coverage, Linux/macOS CI, and this report.

The audit covered the implemented lexer, parser, AST/type representation,
semantic analysis, constant evaluation, LLVM lowering, CRuMB failure ABI,
global/local initialization, nested lvalues, control flow, and host toolchain
selection. It did not add language features, rewrite compiler architecture,
modify game code, conduct a security penetration test, or attempt exhaustive
formal verification.

## Reproducible input robustness

`tests/compiler_robustness.rs` uses four known-valid programs spanning scalar
control flow, nested aggregates, functions, constants, conversions, and range
loops. It then runs:

- every byte-prefix truncation of every seed;
- 5,000 deterministic mutations selected with seed
  `0x5EEC_C0DE_D15C_A11E`;
- bounded insert, delete, replace, duplicate, and delimiter operations;
- a separately generated 64-level nested expression.

Generated inputs are capped at 2 KiB. Every case must return either valid LLVM
or source-located diagnostics without panicking. Every diagnostic span must be
ordered, in bounds, and on UTF-8 character boundaries, and rendering the
diagnostic must also remain panic-free. This is deterministic regression
coverage; it is intentionally not an unbounded fuzzer.

## Semantic and native oracle matrix

`tests/semantic_contract.rs` exercises interactions that are easy to miss when
features are tested only in isolation:

| Contract | Evidence |
| --- | --- |
| Forward constants and nested aggregate initialization | accepted programs plus LLVM verification |
| Aggregate copy-out and by-value function calls/returns | exact native output oracle |
| Nested lvalue mutation and whole-aggregate assignment | exact native output oracle |
| Single evaluation and Boolean short-circuiting | call-count output oracle |
| Struct-name shadowing, nested loops, and scope | compile/verify matrix plus existing range tests |
| Invalid rvalues, const paths, field/index errors, and unsupported signatures | diagnostic substring and span checks |
| Dynamic `i32` division failures | nonzero exit, exact stderr, operand-order stdout, and IR control-flow checks |
| Loop-local storage lifetime | all allocas precede loop blocks plus native output |
| IEEE negative zero | exact `-inf` native output for local and constant paths plus `fneg` IR |

The existing array and composition suites additionally require negative,
upper-bound, and nested aggregate indexing to fail with an exact diagnostic.
The invalid branch must call `crumb_bounds_fail`, end in `unreachable`, and
remain separate from the valid branch's `inbounds getelementptr`.

## Findings and fixes

The severity labels used for this bounded pass are:

- **P0:** compiler/runtime compromise or broad data corruption;
- **P1:** valid source can produce LLVM undefined behavior, memory unsafety, or
  an unbounded resource failure during ordinary execution;
- **P2:** deterministic language-semantics violation with a contained blast
  radius;
- **P3:** diagnostic, documentation, or maintainability defect without wrong
  execution.

No P0 issue was found. Three confirmed defects were fixed:

1. **P1 — unchecked signed division.** Dynamic `i32` division could emit LLVM
   `sdiv` for a zero divisor or `-2147483648 / -1`, both invalid at the LLVM
   level. Generated code now evaluates both operands once, branches around the
   invalid pairs, and emits `sdiv` only in the valid block.
2. **P1 — repeated loop allocation.** A source `let` or hidden range counter
   declared inside a loop emitted `alloca` at the execution point, so a
   long-running loop could grow the native stack without bound. All function
   storage is now allocated once in the entry block; initializer stores remain
   where the declaration executes.
3. **P2 — lost floating negative zero.** Unary `f32` negation used subtraction
   from positive zero, which maps positive zero back to positive zero. Lowering
   now uses LLVM `fneg`, preserving the IEEE sign bit.

## Failure-policy boundary

Bounds and invalid integer division use two deliberately narrow CRuMB calls:

```text
crumb_bounds_fail(index, length)
crumb_division_fail(dividend, divisor)
```

Each call reports fixed scalar context to standard error and terminates. The
generated invalid block ends in `unreachable`; valid operations do not carry a
runtime result wrapper, tag, allocation, or metadata. This is a small policy
boundary, not a generalized panic facility. If Speck later gains exceptions,
the invalid branch can target the future throw/unwind lowering without changing
valid expression syntax, operand evaluation order, aggregate representation, or
the source-level definition of invalid division/indexing.

The division hook contributes 59 bytes of `.text` in the audited Linux runtime
object plus two strings and link metadata when used. Link-section garbage
collection removes the hook and strings from programs that do not perform
runtime `i32` division. Detailed measurements live in `docs/byte-budget.md`.

## Continuous integration contract

`.github/workflows/ci.yml` runs formatting, clippy with warnings denied, and
all targets on Ubuntu 24.04 x86-64 after installing Speck's required LLD native
linker. A macOS 15 ARM64 job runs the same checks and all non-window tests,
including native compilation/linking, PPM and stream presenters, LLVM
verification, and the Cocoa input harness. The single Cocoa window-launch test
is compiled but skipped on hosted CI to avoid depending on an interactive
display session.

The exact local Linux commands for each stacked revision are:

```sh
git switch codex/reliability-inputs
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --test compiler_robustness
cargo test --all-targets

git switch codex/reliability-semantics
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --test llvm_golden
cargo test --test semantic_contract
cargo test --all-targets

git switch codex/reliability-ci
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --test arrays
cargo test --test aggregate_composition
cargo test --all-targets
```

## Remaining deliberate limits and questions

- Runtime `i32` add, subtract, multiply, and unary negation use LLVM's ordinary
  wrapping operations, while constant integer arithmetic diagnoses overflow.
  This is deterministic but asymmetric. Real game code should establish
  whether Speck wants documented wrapping everywhere or future checked
  arithmetic before more operators are added.
- Runtime floating arithmetic follows LLVM/IEEE behavior and may produce
  infinities or NaNs. Float-to-integer conversion already checks NaN and both
  range boundaries before `fptosi`.
- Arrays remain unsupported as function parameters and return types. Structs,
  including structs that contain arrays, are passed and returned by value.
- The failure ABI terminates today. Recovery, stack unwinding, cleanup, and
  catch syntax were deliberately left for a coherent future exception design.
- CI covers the two supported host architectures, but its macOS job does not
  open a real Cocoa window. Manual presenter smoke testing remains appropriate
  before releases that change window lifecycle, focus, or input translation.
- Deterministic mutation tests reduce parser/diagnostic regression risk; they
  do not replace sanitizers, coverage-guided fuzzing, or formal verification.

Before expanding the language again, pressure-test aggregate-heavy game code,
long-running update loops, arithmetic near scalar boundaries, and failure
diagnostics during development. New bug-bash work should start only from a
specific observed risk or feature proposal rather than extending this audit
without a new bound.
