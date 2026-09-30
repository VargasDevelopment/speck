# Speck language reference

This document describes the syntax and semantics implemented today. Newlines
are whitespace, semicolons are optional after simple statements, and `//`
begins a line comment.

```text
program       = "game" string resolution? ";"? declaration* EOF ;
resolution    = "resolution" "(" integer "," integer ")" ;
declaration   = import | struct | constant | global | function | start | update | draw ;
module        = (import | struct | constant | global | function)* EOF ;
import        = "import" string "as" identifier ";"? ;
name          = identifier ("::" identifier)? ;
struct        = "struct" identifier "{" struct_field* "}" ;
struct_field  = identifier ":" value_type ","? ;
constant      = "const" identifier ":" value_type "=" expression ";"? ;
global        = "let" identifier ":" value_type "=" expression ";"? ;
function      = "fn" identifier "(" parameters? ")" "->" return_type block ;
parameters    = parameter ("," parameter)* ;
parameter     = identifier ":" value_type ;
start         = "start" block ;
update        = "update" "(" identifier ":" value_type ")" block ;
draw          = "draw" block ;
value_type    = "i32" | "f32" | "bool" | name
              | "[" value_type ";" array_length "]" ;
array_length  = integer | name ;
return_type   = value_type | "void" ;

block         = "{" statement* "}" ;
statement     = local | assignment | if | while | for | return
              | expression ";"? ;
local         = "let" identifier ":" value_type "=" expression ";"? ;
assignment    = assignable ("=" | "+=" | "-=" | "*=" | "/=" | "%=")
                expression ";"? ;
assignable    = postfix ;
if            = "if" expression block ("else" block)? ;
while         = "while" expression block ;
for           = "for" identifier "in" expression ".." expression block ;
return        = "return" expression? ";"? ;

expression    = logical_or ;
logical_or    = logical_and ("||" logical_and)* ;
logical_and   = equality ("&&" equality)* ;
equality      = comparison (("==" | "!=") comparison)* ;
comparison    = term (("<" | "<=" | ">" | ">=") term)* ;
term          = factor (("+" | "-") factor)* ;
factor        = unary (("*" | "/" | "%") unary)* ;
unary         = ("-" | "!") unary | postfix ;
postfix       = primary (("[" expression "]") | ("." identifier))* ;
primary       = integer | float | "true" | "false"
              | "[" (expression ("," expression)* ","?)? "]"
              | name "{" field_initializer* "}"
              | name | name "(" arguments? ")"
              | ("i32" | "f32") "(" arguments? ")"
              | "(" expression ")" ;
arguments     = expression ("," expression)* ;
field_initializer = identifier ":" expression ","? ;
```

`i32(...)` and `f32(...)` are conversion expressions, not function calls.
Their type names are reserved only where the grammar already expects a type or
conversion. The lexer uses longest-match rules for `+=`, `-=`, `*=`, `/=`, `%=`,
`<=`, `>=`, `==`, `!=`, `&&`, `||`, `->`, `::`, and `..`.

## Files and imports

A game starts with its `game` title and owns the three lifecycle blocks.
Other `.spk` files contain structs, constants, globals, named functions, and
imports. They do not declare a game title, resolution, or lifecycle blocks.

The optional game-header clause selects the logical framebuffer size:

```text
game "Wide sketch" resolution(640, 360)
```

Without the clause, the framebuffer is 320 by 180 pixels. Each dimension must
be a positive integer literal from 1 through 4096; any aspect ratio is allowed.
Expressions, named constants,
floats, zero, and negative dimensions are rejected. The clause may appear only
once, directly after the title and before the optional semicolon, imports, or
other declarations. `resolution` remains an ordinary identifier elsewhere.

`FRAMEBUFFER_WIDTH` and `FRAMEBUFFER_HEIGHT` are immutable predefined `i32` constants
with the selected dimensions. They work in expressions, constant and global
initializers, and array lengths. Imported modules see the entry game's selected
dimensions. User declarations and local bindings cannot replace these names.

```text
// game.spk
game "Rooms"
import "rooms.spk" as rooms
start { print_i32(rooms::SPAWNS[0].x) }
update(dt: f32) {}
draw {}

// rooms.spk
struct Point { x: i32 y: i32 }
const COUNT: i32 = 2
const SPAWNS: [Point; COUNT] = [Point { x: 10, y: 20 }, Point { x: 30, y: 40 }]
```

Imports are top-level declarations, and may appear before or after the
other declarations following the game title. Paths are relative to the file
containing the import, must use `.spk`, and can contain spaces or Unicode.
An explicit alias is required. Use `rooms::Point` for a type or struct literal,
`rooms::COUNT` for a constant or array length, and `rooms::make()` for a function.
Qualified globals also support reading and assignment.

Every declaration in an imported file is accessible through its alias.
Unqualified names inside that file refer to its own declarations, lexical
locals, or builtins. Each file imports its own dependencies; imported aliases
are not reexported, and qualified names contain exactly one `::`.
Aliases cannot duplicate or conflict with top-level declarations or builtins.
Local values may share an alias's spelling because `alias::name` explicitly
selects the module namespace.

A canonical file path identifies one module, even when several files import it
through different relative paths, aliases, or symlinks. Its named types have one
identity and its globals have one storage location. Two different files defining
`Point` produce distinct types. Declaration and import order do not change this
identity. Constants and globals retain their existing compile-time initializer
rules; importing a file executes no code. Import cycles are rejected, and import
nesting is limited to 128 files.

`check`, `build`, `dev`, and `run` load the full import closure from the entry
file. The Rust `analyze_path` API does the same. The source-string `analyze` API
supports standalone programs and reports that imports require a file path.
There are no packages, selective exports, or reexports.

## Types and functions

Speck's scalar value types are `i32`, `f32`, and `bool`. Fixed arrays and
declared struct names are also value types as described below. Variables,
constants, and parameters always declare a type. There are no implicit
conversions, so this is invalid:

```text
let x: f32 = 10
```

`i32` is a signed 32-bit integer with range `-2147483648..2147483647`
(both endpoints included). Integer literals must fit that range; the negative
endpoint is accepted as `-2147483648`. `f32` is an IEEE 754 single-precision
floating-point value. Decimal floating literals are rounded to `f32` and must
remain finite; literals that overflow to infinity are rejected.

`void` is a return type, not a value type. A `void` function may fall through
its final block or use bare `return` for an early exit. It may not return an
expression. A non-void function must return a value of its declared type on
every reachable path and may not use bare `return`.

```text
fn show(value: i32) -> void {
    print_i32(value)
}

fn choose(flag: bool) -> i32 {
    if flag { return 1 } else { return 0 }
}
```

A void call is valid as an expression statement. It is invalid as a variable
initializer, function argument, return value, condition, operand, comparison,
or assignment value. Speck has no first-class unit value and does not fabricate
a sentinel result for effect-only work.

Every game declares exactly one `start`, `update(dt: f32)`, and `draw` block.
These lifecycle blocks are implicitly effect-only and do not use a return-type
annotation. CRuMB calls them; Speck source does not declare `main`.

## Struct-like value records

A `struct` declaration introduces one named, fixed-layout value type:

```text
struct Platform {
    x: i32
    y: i32
    width: i32
    height: i32
}

let platform: Platform = Platform {
    height: 5,
    width: 60,
    y: 140,
    x: 40
}
```

Field layout follows declaration order. Struct initializer expressions execute
once each, in their written source order, including nested struct literals and
array elements. Reordering named fields therefore reorders their side effects
without changing field layout. Every field must appear exactly once with its
declared type.
Unknown, duplicate, and missing initializers are errors. Struct declarations
are collected module-wide, field names must be unique within a declaration,
and unknown or directly/indirectly recursive value types are rejected.

Structs have value semantics and fixed native storage. Assignment copies the
complete value. Passing a struct to a function gives the callee a value copy,
and this implementation also supports returning a struct by value:

```text
fn moved(value: Platform) -> Platform {
    value.x += 5
    return value
}
```

Reading `platform.x` produces the field value. Writing `platform.x = 10` or
`platform.x += 1` changes the field inside that mutable struct variable.
Writing any field path rooted in `const` is rejected. There are no methods,
constructors, classes, inheritance, interfaces, traits, visibility rules,
references, identity, reflection metadata, or dynamic dispatch.

## Fixed-size arrays and indexing

An array type names its element type and fixed compile-time length:

```text
const VALUE_COUNT: i32 = 4
let values: [i32; VALUE_COUNT] = [2, 4, 6, 8]
let flags: [bool; 3] = [true, false, true]
```

The length must be a positive integer literal or an `i32` constant. Array
declarations require an explicit type annotation; array literals are not
generally inferred. A literal must contain exactly the declared number of
elements, and every element must exactly match the element type. Nested fixed
arrays follow from the type grammar and use repeated indexing such as
`matrix[row][column]`; there is no separate multidimensional-array runtime.

Fixed arrays can be passed to and returned from functions. The signature supplies
the type for a literal argument or return value; element types and lengths must
match exactly. Parameters receive value copies, so modifying an array parameter
does not change the caller's array. Returned arrays are values and can be indexed
directly. Arguments execute once each, left to right.

```text
fn shifted(values: [i32; 2]) -> [i32; 2] {
    values[0] += 1
    return values
}

fn pair() -> [i32; 2] { return [4, 5] }
```

The same rules apply to nested arrays and arrays of structs. These are fixed-size
copies, with cost proportional to the value size; there are no array references,
slices, or length-polymorphic parameters.

Arrays use ordinary value storage. Locals live in function storage, mutable
globals use fixed LLVM globals, and immutable aggregate constants use
read-only LLVM global storage. No array object, length header, heap allocation,
or garbage collector is involved. Whole-array assignment between values of
the same array type copies the complete value.

Index expressions accept only `i32`:

```text
let selected: i32 = values[index]
values[index] = 40
values[index] += 2
```

A compile-time-known index outside `0..length` is rejected. Every runtime index
is checked for both a negative value and a value at or above the length before
LLVM emits an in-bounds element address. Failure calls the narrow
`crumb_bounds_fail(index, length)` runtime function, prints
`Speck array index N is out of bounds for length L` to standard error, and exits
with failure. There is no exception or recoverable panic value. LLVM and Clang
can fold away checks for constant valid indices.

Writing through an indexed path requires a mutable root. A path rooted in an
immutable `const` array is rejected. Reading an element produces a value.

## Aggregate composition

Arrays may contain structs, and struct fields may contain fixed arrays or other
non-recursive structs. Postfix access is composable, so reads and writes may
alternate indexing and field selection:

```text
struct Platform {
    x: i32
    width: i32
}

struct Level {
    platforms: [Platform; 2]
    positions: [i32; 2]
}

let levels: [Level; 1] = [
    Level {
        platforms: [
            Platform { x: 10, width: 20 },
            Platform { x: 40, width: 30 }
        ],
        positions: [0, 0]
    }
]

levels[0].platforms[1].x += 2
levels[0].positions[0] = 50
```

Every aggregate remains a value. In:

```text
let copy: Platform = levels[0].platforms[0]
copy.x = 99
```

the indexed read copies the `Platform`, so changing `copy` does not change
`levels`. By contrast, `levels[0].platforms[0].x = 99` follows one lvalue path
into the mutable global and changes the stored field. The root binding controls
mutability for the complete path; no path rooted in a `const` value may be
written.

Compile-time initialization recursively accepts scalar constants, array
literals, and struct literals. This permits immutable level data such as
`const PLATFORMS: [Platform; 3] = [...]` without a runtime constructor. Nested
fixed arrays also remain supported where their explicitly declared types
match. Aggregate composition adds no references, aliases, hidden identity,
runtime metadata, allocation, or heap.

## Explicit numeric conversions

The conversions `i32(expression)` and `f32(expression)` each take exactly one
numeric value:

```text
let pixels: i32 = i32(position)
let velocity: f32 = f32(120)
```

- `f32(i32_value)` performs signed integer-to-floating conversion. Large
  integers may be rounded because not every `i32` is exactly representable as
  `f32`; for example, `f32(16777217)` rounds to `16777216.0`.
- `i32(f32_value)` truncates finite, in-range values toward zero, clamps values
  at or above `2147483648.0` (including positive infinity) to `2147483647`,
  clamps values at or below `-2147483648.0` (including negative infinity) to
  `-2147483648`, and converts NaN to zero.
- Same-type conversions such as `i32(i32_value)` and `f32(f32_value)` are
  accepted no-ops.
- Boolean/number and void/number conversions are invalid.

These conversion rules also apply in constant expressions, but their operands
must first evaluate successfully: `i32(1.0 / 0.0)` is rejected in a top-level
initializer before conversion can clamp the result.

The LLVM lowering checks NaN and both bounds before placing `fptosi` on the
in-range-only control-flow path, avoiding poison-producing out-of-range LLVM
conversion behavior.

## Constants and global initialization

Top-level `const` declarations are immutable, explicitly typed, and available
throughout the program regardless of declaration order:

```text
const AREA: i32 = WIDTH * HEIGHT
const HEIGHT: i32 = 180
const WIDTH: i32 = 320
const DEBUG: bool = false
```

Constant expressions support numeric and Boolean literals, unary `-` and `!`,
other constants, arithmetic, comparisons, equality, `&&`, `||`, parentheses,
and explicit numeric conversions. They cannot call functions, reference
mutable globals or runtime values, or use `void`. Integer `+`, `-`, `*`, `/`,
`%`, and unary negation are checked for overflow. Every evaluated floating
arithmetic result must remain finite. Overflow, division by zero (including
floating positive or negative zero), integer remainder by zero, and invalid
conversions are diagnosed at the initializer. These checks apply to intermediate
results, even when a later operation would bring the final value back in range.

Dependencies are evaluated after all constant names have been collected.
Cycles are rejected with the participating names in dependency order.
Short-circuiting also applies during constant evaluation, so an unreachable
right operand is not evaluated. Constants need no runtime initialization.
Scalar constants are inlined directly. Constant arrays and structs use
read-only aggregate storage so indexed or field access still has a stable
native address.

Mutable top-level `let` initializers use the same compile-time expression rules
and may reference constants, but not another mutable global. Locals remain
runtime initialized.

## Operators and mutation

Arithmetic and ordering work on same-typed `i32` or `f32` operands, except
that `%` (remainder) requires `i32`. Equality
works on matching numeric or Boolean operands. Conditions must be `bool`.
Runtime `i32` addition, subtraction, multiplication, and unary negation wrap
modulo 2^32 and interpret the result as signed: `2147483647 + 1` becomes
`-2147483648`, and negating `-2147483648` leaves it unchanged. This also applies
to compound assignments and local initializers, even when their operands are
literals. Top-level initializers instead use the checked constant rules above.

`i32` division is signed integer division with truncation toward zero, and
`i32` remainder takes the sign of the dividend, so `-7 % 3` is `-1` and
`7 % -3` is `1`. A runtime divisor of zero and the
overflowing `-2147483648 / -1` case terminate through the narrow
`crumb_division_fail(dividend, divisor)` runtime hook with a development
diagnostic; the same two cases for `%` terminate through the parallel
`crumb_remainder_fail(dividend, divisor)` hook. The guard executes after both operands have been evaluated and
before LLVM emits `sdiv` or `srem`, so invalid division never reaches LLVM undefined
behavior. This failure edge is deliberately isolated; it is not an exception
system and may be replaced if Speck later gains one.

Runtime `f32` arithmetic can produce infinity or NaN, including through overflow
or division by zero; it does not use the integer failure hooks. Unary
floating-point negation preserves the IEEE sign, including negative zero.
All six floating comparisons (`==`, `!=`, `<`, `<=`, `>`, `>=`) are ordered:
if either operand is NaN, the result is false, **including `!=`**. Positive and
negative zero compare equal.

Boolean precedence, from lowest to highest, is `||`, `&&`, equality,
comparison, arithmetic, unary, and primary expressions. Both operands of `&&`
and `||` must be `bool`. Evaluation short-circuits: `left && right` skips
`right` when `left` is false, while `left || right` skips `right` when `left` is
true. LLVM emits branches and a merged Boolean result rather than eager bitwise
operations.

`+=`, `-=`, `*=`, `/=`, and `%=` are statement-only shorthand for numeric mutation:

```text
x += velocity * dt
frames += 1
```

The target must be a mutable local, parameter, global, indexed element path, or
struct field path.
The target and right operand must have the same numeric type; no conversion is
inserted. The right expression is evaluated once, and the result is stored
back. Constants and Boolean values cannot be compound-assignment targets.
Assignment remains a statement and does not produce a value.

Locals use lexical block scope and may shadow outer names. Parameters preserve
Speck's existing mutable behavior.

## Exclusive range loops

The narrow `for` statement iterates upward by exactly one over a
lower-inclusive, upper-exclusive `i32` range:

```text
for i in 0..PLATFORM_COUNT {
    draw_platform(PLATFORMS[i])
}
```

The lower bound is evaluated once, then the upper bound is evaluated once,
before the first condition check. Both must have type `i32`. The loop variable
is a new read-only `i32` binding scoped to the loop body; it may shadow an outer
name, and nested loops may shadow it again. Assigning or compound-assigning to
the loop variable is an error.

At each iteration Speck checks `i < upper`, runs the body when true, and then
increments `i` by one. A lower bound greater than or equal to the upper bound
therefore runs zero iterations. Negative bounds work normally. `..` is accepted
only in this statement syntax: ranges are not values. There is no inclusive
range, custom or negative step, array-item iteration, iterator protocol,
`break`, `continue`, or loop expression.

## Periodic math

`sin(angle: f32) -> f32` returns the sine of an angle in radians, using the
host C library's single-precision `sinf`. Finite results lie in `[-1.0, 1.0]`;
NaN and either infinity produce NaN. Finite results are approximate and are
not guaranteed bit-identical across platforms. Keep animation phases bounded
to preserve useful `f32` input precision during long runs.

Like other builtins, `sin` is a runtime function, not a constant initializer.
It requires an explicit `f32` argument; use `f32(integer)` when converting.

## CRuMB functions and graphics

The available effect-only functions all return real `void`:

- `print_i32(value: i32)`
- `debug_frame(frame: i32, value: f32)`
- `clear_rgb(r: i32, g: i32, b: i32)`
- `fill_rect(x: i32, y: i32, width: i32, height: i32, r: i32, g: i32, b: i32)`

The built-in ABI and LLVM declarations use `void`; there is no fabricated
result. RGB components clamp to 0 through 255. Filled rectangles use half-open
bounds, clip to the game's logical framebuffer, and do nothing for non-positive
sizes or wholly off-screen rectangles. No graphics-specific language type is
introduced.

The quoted game title is compile-time metadata, not a general-purpose string
value. Strings are otherwise absent from the type system.

## Audio

`tone(frequency: f32, seconds: f32, volume: f32)` plays a sine tone and
`noise(seconds: f32, volume: f32)` plays a short noise burst. Both return `void`.
Playback is asynchronous and available through native macOS `speck run` only.
`speck build` (PPM) and `speck dev` (browser) accept the same calls silently,
without opening an audio device on the host.

Frequency is in hertz, duration is in seconds, and volume ranges from 0.0 to 1.0.
Tones outside 20–20,000 Hz, nonfinite arguments, nonpositive duration, and
nonpositive volume do nothing. Durations above two seconds and finite volumes
above 1.0 clamp to those limits. Durations shorter than two samples at 48 kHz
are silent. Effects have a short attack and a decay to silence; volume 1.0
leaves mixing headroom rather than producing a full-scale single voice.

The runtime admits eight simultaneous effects and at most 32 pending commands.
New effects are dropped when either capacity is full. Calls never wait for
playback to finish. Audio-device initialization failure prints one warning and
continues the game silently. Shutdown stops current sounds immediately, so a
sound immediately followed by `quit()` may not be heard.

For example, a high tone can mark a match, a brief noise burst can mark a
mismatch, and a lower, longer tone can mark an escape:

```speck
tone(880.0, 0.08, 0.6)
noise(0.06, 0.35)
tone(220.0, 0.18, 0.5)
```

Call the desired effect once when its event occurs; calling it every update
starts overlapping effects. `examples/audio_feedback.spk` offers a native
keyboard audition of these three effects.

A top-level sound declaration embeds one backing track in the executable:

```speck
sound TRACK = "assets/track.wav"
```

The path must be relative to the file containing the declaration. Sound
declarations in imported files resolve from that imported file and can be used
through the import alias, such as `music::TRACK`. A declaration acts as an
immutable `i32` handle; quoted paths remain declaration syntax and do not add a
string value type.

The compiler accepts RIFF/WAVE files containing uncompressed mono PCM16 at
48,000 Hz. Encoded files are limited to 32 MiB and decoded audio to 180 seconds.
It rejects missing, malformed, compressed, stereo, differently sampled, empty,
or oversized assets at the declaration and includes asset files in `--watch`
dependency tracking. Only PCM sample data is embedded, so the executable does
not read the WAV or source tree at runtime.

One backing track can play at a time:

- `sound_play(sound: i32, volume: f32)` restarts that sound at the beginning.
- `sound_pause()` pauses the track without affecting procedural effects.
- `sound_resume()` resumes a paused track.
- `sound_stop()` stops the track and resets its reported position to zero.
- `sound_position() -> f32` reports playback position in seconds.
- `sound_seek(seconds: f32)` moves within the active track.

Playback and seek are asynchronous. `sound_play` ignores unknown handles,
nonfinite volume, and nonpositive volume, and clamps volume above 1.0.
`sound_seek` ignores nonfinite values and clamps finite values to the track's
bounds. Tracks stop at the end; there is no automatic looping. Pause and stop
cannot be lost when the bounded audio command queue is full. On PPM and browser
development backends all track operations are silent no-ops and
`sound_position()` returns `0.0`.

## Digital keyboard input and shutdown

The presenter-independent input built-ins are:

- `key_down(key: i32) -> bool`
- `key_pressed(key: i32) -> bool`
- `key_released(key: i32) -> bool`
- `quit() -> void`

Speck predefines immutable `i32` constants for ordinary physical keyboard keys:

| Family | Constants |
| --- | --- |
| Letters | `KEY_A` through `KEY_Z` |
| Number row | `KEY_0` through `KEY_9` |
| Punctuation | `KEY_MINUS`, `KEY_EQUAL`, `KEY_BRACKET_LEFT`, `KEY_BRACKET_RIGHT`, `KEY_BACKSLASH`, `KEY_SEMICOLON`, `KEY_QUOTE`, `KEY_BACKQUOTE`, `KEY_COMMA`, `KEY_PERIOD`, `KEY_SLASH` |
| Navigation | `KEY_UP`, `KEY_DOWN`, `KEY_LEFT`, `KEY_RIGHT`, `KEY_TAB`, `KEY_BACKSPACE`, `KEY_DELETE`, `KEY_INSERT`, `KEY_HOME`, `KEY_END`, `KEY_PAGE_UP`, `KEY_PAGE_DOWN` |
| Common controls | `KEY_SPACE`, `KEY_ENTER`, `KEY_ESCAPE` |
| Function row | `KEY_F1` through `KEY_F24` |
| Keypad | `KEY_NUMPAD_0` through `KEY_NUMPAD_9`, `KEY_NUMPAD_ADD`, `KEY_NUMPAD_SUBTRACT`, `KEY_NUMPAD_MULTIPLY`, `KEY_NUMPAD_DIVIDE`, `KEY_NUMPAD_DECIMAL`, `KEY_NUMPAD_EQUAL`, `KEY_NUMPAD_ENTER` |
| Sided modifiers | `KEY_SHIFT_LEFT`, `KEY_SHIFT_RIGHT`, `KEY_CONTROL_LEFT`, `KEY_CONTROL_RIGHT`, `KEY_ALT_LEFT`, `KEY_ALT_RIGHT`, `KEY_META_LEFT`, `KEY_META_RIGHT` |

Names denote physical positions (browser `KeyboardEvent.code`), rather than text
characters. Shift does not turn `KEY_1` into a separate exclamation-mark key.
Meta is Command on macOS and the corresponding Meta/Windows key in browsers.
The native macOS mapping has no Insert or F21–F24 virtual code; those remain
available through browser input. Lock keys and Fn are not ordinary held-key
inputs and are outside this API. OS or browser shortcuts may intercept keys.

Keypad Enter has its own `KEY_NUMPAD_ENTER` identity. Earlier native builds
aliased it to `KEY_ENTER`; games that want either should query both. Other
existing key IDs retain their values. The complete mapping lives in
[`runtime/crumb/keys.def`](../runtime/crumb/keys.def).

Bindings belong to the game, for example:

```speck
let flip_key: i32 = KEY_F
// In update:
if key_pressed(flip_key) { tone(740.0, 0.06, 0.2) }
```

They may be used wherever an `i32` value is valid, including compile-time
constant expressions. User constants, globals, functions, parameters, and
locals may not silently replace the predefined names. The numeric identifiers
are an internal CRuMB ABI detail; Speck programs should use the names. Passing
any other integer is safe and returns `false` from all three query functions.

`key_down` remains true for every frame during which a key is held.
`key_pressed` is true during exactly the frame that observes an up-to-down
transition, and `key_released` is true during exactly the frame that observes a
down-to-up transition. Native or browser repeat events while a key remains down
do not create another press. If both transitions arrive between two updates,
both one-frame queries are true in the next frame and `key_down` is false.

Each interactive frame clears the previous one-frame flags, pumps presenter
events, applies all pending key transitions, runs `update(dt)`, runs `draw`, and
presents the completed framebuffer. The three input queries therefore remain
stable throughout both lifecycle blocks. PPM presentation has no event source,
so it reports every key up unless a test harness explicitly manipulates CRuMB's
private input state.

`quit()` is effect-only and cannot be used as a value. It sets a CRuMB-owned
request flag rather than terminating inside generated code. If called from
`update` or `draw`, the current update/draw/present cycle completes and the loop
shuts down before beginning another frame. Closing the native window may stop
before another update begins because close is observed during event polling.

Cocoa hardware codes and browser `KeyboardEvent.code` strings are not Speck
language values. Presenters translate them into CRuMB's fixed identifiers.
Movement, jumping, collision, Pong rules, and other game mechanics remain
ordinary user-authored Speck code.

## Compiler nesting limit

The compiler limits combined syntax and expression-tree nesting to 80 levels
and reports a source-located diagnostic when it is exceeded. This includes
deeply nested blocks, types, calls, and operators, as well as long binary or
field/index chains whose trees become deep without extra parentheses. Split
such expressions into intermediate values or helpers. Array width and the
number of independent declarations do not count as nesting.


Resolved compile-time aggregate values have a separate limit of 128 nested
array or struct levels. Scalars have depth zero; each enclosing array or struct
adds one level. This counts the resulting value, including values referenced
from other constants, regardless of declaration order or file boundaries.
The same limit applies to constant declarations, global initializers, and
constants evaluated for array lengths. Exceeding it reports the constructor
that would create the oversized value. Splitting a value across declarations
or files does not bypass this limit. Array width is not aggregate depth.

## Persistent integer slots

Games can retain a small record across executions:

```speck
let best: i32 = 0
start { best = load_i32(0, 0) }
update(dt: f32) {
    if key_pressed(KEY_S) {
        if save_i32(0, best) { print_i32(best) }
    }
}
```

`load_i32(slot: i32, fallback: i32) -> i32` returns a stored value or the
fallback when the slot is absent, malformed, or unavailable.
`save_i32(slot: i32, value: i32) -> bool` atomically replaces one slot and
reports success. Slots are `0` through `15`; out-of-range requests fail or use
the fallback. All signed 32-bit values are supported. Calls are synchronous;
save at a run boundary or deliberate user action rather than every frame.

The exact UTF-8 `game` title selects the namespace, using a fixed hash so titles
never become filesystem paths. The namespace is initialized before `start`.
Moving the executable or source tree keeps the record; changing the title
selects another namespace. Games with identical titles share it.

- macOS: `~/Library/Application Support/Speck/game-<hash>/`
- Linux: `$XDG_DATA_HOME/speck/game-<hash>/` when XDG_DATA_HOME is absolute,
  otherwise `~/.local/share/speck/game-<hash>/`.
- An absolute `SPECK_SAVE_DIR` overrides the base on either host; each game still
  gets its own namespace below it. Relative overrides are rejected.

Reading does not create directories. Saves create private directories/files
and replace a single slot through a temporary file and rename. Concurrent
writers to different slots preserve one another; the last rename wins for the
same slot. A save is not a transaction across multiple slots. Missing home
configuration, permissions, and invalid files never terminate the game.

Storage runs on the game host for every presenter, including development
previews; it is not browser localStorage. Tests should provide an isolated
`SPECK_SAVE_DIR`. The language exposes no arbitrary-path runtime file API.
