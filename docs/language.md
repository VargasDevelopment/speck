# Speck language reference

This document describes the syntax and semantics implemented today. Newlines
are whitespace, semicolons are optional after simple statements, and `//`
begins a line comment.

```text
program       = "game" string ";"? declaration* EOF ;
declaration   = struct | constant | global | function | start | update | draw ;
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
value_type    = "i32" | "f32" | "bool" | identifier
              | "[" value_type ";" array_length "]" ;
array_length  = integer | identifier ;
return_type   = value_type | "void" ;

block         = "{" statement* "}" ;
statement     = local | assignment | if | while | for | return
              | expression ";"? ;
local         = "let" identifier ":" value_type "=" expression ";"? ;
assignment    = assignable ("=" | "+=" | "-=" | "*=" | "/=")
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
              | identifier "{" field_initializer* "}"
              | identifier | identifier "(" arguments? ")"
              | ("i32" | "f32") "(" arguments? ")"
              | "(" expression ")" ;
arguments     = expression ("," expression)* ;
field_initializer = identifier ":" expression ","? ;
```

`i32(...)` and `f32(...)` are conversion expressions, not function calls.
Their type names are reserved only where the grammar already expects a type or
conversion. The lexer uses longest-match rules for `+=`, `-=`, `*=`, `/=`, `%=`,
`<=`, `>=`, `==`, `!=`, `&&`, `||`, `->`, and `..`.

## Types and functions

Speck's scalar value types are `i32`, `f32`, and `bool`. Fixed arrays and
declared struct names are also value types as described below. Variables,
constants, and parameters always declare a type. There are no implicit
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

Field layout follows declaration order. Literal initializer order does not
matter, but every field must appear exactly once with its declared type.
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

Arrays use ordinary value storage. Locals live in function storage, mutable
globals use fixed LLVM globals, and immutable aggregate constants use
read-only LLVM global storage. No array object, length header, heap allocation,
or garbage collector is involved. Whole-array assignment between values of
the same array type copies the complete value. Arrays are not yet accepted as
function parameter or return types.

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
for overflow. Non-finite floating results, division or remainder by zero, and invalid
conversions are diagnosed at the initializer.

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
`i32` division is signed integer division with truncation toward zero, and
`i32` remainder takes the sign of the dividend, so `-7 % 3` is `-1` and
`7 % -3` is `1`. A runtime divisor of zero and the
overflowing `-2147483648 / -1` case terminate through the narrow
`crumb_division_fail(dividend, divisor)` runtime hook with a development
diagnostic; the same two cases for `%` terminate through the parallel
`crumb_remainder_fail(dividend, divisor)` hook. The guard executes after both operands have been evaluated and
before LLVM emits `sdiv`, so invalid division never reaches LLVM undefined
behavior. This failure edge is deliberately isolated; it is not an exception
system and may be replaced if Speck later gains one. Unary floating-point
negation preserves the IEEE sign, including negative zero. Floating comparisons
are ordered, so comparisons involving NaN are false; source floating literals
must be finite.

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
