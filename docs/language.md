# Speck language reference

This document describes the syntax and semantics implemented today. Newlines
are whitespace, semicolons are optional after simple statements, and `//`
begins a line comment.

```text
program       = "game" string ";"? declaration* EOF ;
declaration   = constant | global | function | start | update | draw ;
constant      = "const" identifier ":" value_type "=" expression ";"? ;
global        = "let" identifier ":" value_type "=" expression ";"? ;
function      = "fn" identifier "(" parameters? ")" "->" return_type block ;
parameters    = parameter ("," parameter)* ;
parameter     = identifier ":" value_type ;
start         = "start" block ;
update        = "update" "(" identifier ":" value_type ")" block ;
draw          = "draw" block ;
value_type    = "i32" | "f32" | "bool" ;
return_type   = value_type | "void" ;

block         = "{" statement* "}" ;
statement     = local | assignment | if | while | return | expression ";"? ;
local         = "let" identifier ":" value_type "=" expression ";"? ;
assignment    = identifier ("=" | "+=" | "-=" | "*=" | "/=")
                expression ";"? ;
if            = "if" expression block ("else" block)? ;
while         = "while" expression block ;
return        = "return" expression? ";"? ;

expression    = logical_or ;
logical_or    = logical_and ("||" logical_and)* ;
logical_and   = equality ("&&" equality)* ;
equality      = comparison (("==" | "!=") comparison)* ;
comparison    = term (("<" | "<=" | ">" | ">=") term)* ;
term          = factor (("+" | "-") factor)* ;
factor        = unary (("*" | "/") unary)* ;
unary         = ("-" | "!") unary | primary ;
primary       = integer | float | "true" | "false"
              | identifier | identifier "(" arguments? ")"
              | ("i32" | "f32") "(" arguments? ")"
              | "(" expression ")" ;
arguments     = expression ("," expression)* ;
```

`i32(...)` and `f32(...)` are conversion expressions, not function calls.
Their type names are reserved only where the grammar already expects a type or
conversion. The lexer uses longest-match rules for `+=`, `-=`, `*=`, `/=`,
`<=`, `>=`, `==`, `!=`, `&&`, `||`, and `->`.

## Types and functions

Speck's value types are `i32`, `f32`, and `bool`. Variables, constants, and
parameters always declare one of these types. There are no implicit
conversions, so this is invalid:

```text
let x: f32 = 10
```

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

## Explicit numeric conversions

The conversions `i32(expression)` and `f32(expression)` each take exactly one
numeric value:

```text
let pixels: i32 = i32(position)
let velocity: f32 = f32(120)
```

- `f32(i32_value)` performs signed integer-to-floating conversion. Large
  integers may be rounded because not every `i32` is exactly representable as
  `f32`.
- `i32(f32_value)` truncates finite, in-range values toward zero, clamps values
  at or above the positive boundary to `2147483647`, clamps values at or below
  the negative boundary to `-2147483648`, and converts NaN to zero.
- Same-type conversions such as `i32(i32_value)` and `f32(f32_value)` are
  accepted no-ops.
- Boolean/number and void/number conversions are invalid.

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
mutable globals or runtime values, or use `void`. Integer arithmetic is checked
for overflow. Non-finite floating results, division by zero, and invalid
conversions are diagnosed at the initializer.

Dependencies are evaluated after all constant names have been collected.
Cycles are rejected with the participating names in dependency order.
Short-circuiting also applies during constant evaluation, so an unreachable
right operand is not evaluated. Constants are inlined into LLVM; they create no
mutable storage and need no runtime initialization.

Mutable top-level `let` initializers use the same compile-time expression rules
and may reference constants, but not another mutable global. Locals remain
runtime initialized.

## Operators and mutation

Arithmetic and ordering work on same-typed `i32` or `f32` operands. Equality
works on matching numeric or Boolean operands. Conditions must be `bool`.
`i32` division is signed integer division. Floating comparisons are ordered,
so comparisons involving NaN are false; source floating literals must be
finite.

Boolean precedence, from lowest to highest, is `||`, `&&`, equality,
comparison, arithmetic, unary, and primary expressions. Both operands of `&&`
and `||` must be `bool`. Evaluation short-circuits: `left && right` skips
`right` when `left` is false, while `left || right` skips `right` when `left` is
true. LLVM emits branches and a merged Boolean result rather than eager bitwise
operations.

`+=`, `-=`, `*=`, and `/=` are statement-only shorthand for numeric mutation:

```text
x += velocity * dt
frames += 1
```

The identifier must name a mutable local, parameter, or global. The target and
right operand must have the same numeric type; no conversion is inserted. The
right expression is evaluated once, and the result is stored back. Constants
and Boolean values cannot be compound-assignment targets. Assignment remains a
statement and does not produce a value.

Locals use lexical block scope and may shadow outer names. Parameters preserve
Speck's existing mutable behavior.

## CRuMB functions and graphics

The available effect-only functions all return real `void`:

- `print_i32(value: i32)`
- `debug_frame(frame: i32, value: f32)`
- `clear_rgb(r: i32, g: i32, b: i32)`
- `fill_rect(x: i32, y: i32, width: i32, height: i32, r: i32, g: i32, b: i32)`

The built-in ABI and LLVM declarations use `void`; there is no fabricated
result. RGB components clamp to 0 through 255. Filled rectangles use half-open
bounds, clip to the 320x180 framebuffer, and do nothing for non-positive sizes
or wholly off-screen rectangles. No graphics-specific language type is
introduced.

The quoted game title is compile-time metadata, not a general-purpose string
value. Strings are otherwise absent from the type system.

## Digital keyboard input and shutdown

The presenter-independent input built-ins are:

- `key_down(key: i32) -> bool`
- `key_pressed(key: i32) -> bool`
- `key_released(key: i32) -> bool`
- `quit() -> void`

Speck predefines these immutable `i32` constants:

```text
KEY_W       KEY_A       KEY_S       KEY_D
KEY_UP      KEY_DOWN    KEY_LEFT    KEY_RIGHT
KEY_SPACE   KEY_ENTER   KEY_ESCAPE
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
