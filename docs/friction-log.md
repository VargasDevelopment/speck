# Language friction log

Speck language work is driven by programs we genuinely try to write. This log
keeps observed friction visible without treating every familiar language
feature as an automatic requirement.

## Resolved in the language-ergonomics slice

- Effect-only user functions now return `void`; effect-only CRuMB built-ins are
  represented as real `void` calls rather than values.
- Common numeric mutation now has statement-level `+=`, `-=`, `*=`, and `/=`.
- Moving between `i32` pixel coordinates and `f32` simulation values is
  explicit and predictable through `i32(...)` and `f32(...)`.
- Named dimensions, speeds, and flags can use immutable top-level constants.
- Boolean conditions compose with short-circuiting `&&` and `||`.

## Partially addressed, still under evaluation

- `i32` and `f32` are precise and make conversion points clear, but we still
  need more manual visual sketches before deciding whether the names feel
  pleasantly explicit or unnecessarily mechanical.

## Explicitly deferred

- Alternative names, aliases, or renaming for `i32` and `f32`.
- Syntax highlighting.
- Completion for built-ins and declared functions.
- Language-server functionality.

These editor and naming questions remain open; this slice does not silently
discard them or treat them as language requirements prematurely.

## Input-slice observations

- A fixed set of readable `KEY_*` names plus `key_down`, `key_pressed`, and
  `key_released` was enough to express the infrastructure rectangle without an
  enum feature, input syntax, or presenter concepts.
- Browser long polling has no durable controller connection, so focus messages
  alone cannot guarantee release after a vanished tab or tunnel. A small
  controller heartbeat lease solved that host-lifecycle problem without
  changing Speck or creating a generalized event system.
- Movement helpers, action mapping, rebinding, arbitrary key enumeration, and
  physics remain unevaluated. The next manually written controllable rectangle
  and BOOTS sketch should determine whether any of them represent real friction.
