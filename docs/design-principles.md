# Design principles

Speck is a tiny ahead-of-time compiled language for tiny games and visual
programs. Its immediate-mode lifecycle and small graphics API should make
animations, generative sketches, simulations, and Processing-like experiments
pleasant without weakening the floppy-sized native-game constraint.

- Prefer explicit behavior over implicit coercion. Numeric conversions should
  be visible at the point where precision or representation changes.
- Let visual programs stay small. Lifecycle blocks, effect-only functions,
  constants, and common mutation should require little scaffolding.
- Do not fabricate meaningless values. Operations that exist only for their
  effects return `void`.
- Keep common mutation concise without turning assignment into an expression
  language. Compound assignment is a statement and evaluates its right side
  once.
- Add language features in response to real program pressure. Programs we
  genuinely try to write should drive the language.
- Keep compile-time convenience out of shipped runtime cost. Constants are
  evaluated by the compiler and inlined without storage or initialization code.
- Preserve CRuMB as a narrow portable boundary. Language ergonomics must not
  couple Speck semantics to a presenter, window system, or host platform.
- Expose stable game meaning, not host events. Speck asks whether a named
  digital key is down, pressed, or released; Cocoa and the browser translate
  their own codes below the CRuMB boundary.
- Keep mechanics in the program. Input state is infrastructure, while movement,
  jumping, collision, Pong rules, and BOOTS behavior remain user-authored Speck
  until real programs demonstrate a reusable need.
