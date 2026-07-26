# Speck language: vertical-slice grammar

This document describes the syntax actually implemented by the first vertical
slice. Newlines are whitespace. Semicolons are optional after simple
statements. `//` begins a line comment.

```text
program       = "game" string ";"? declaration* EOF ;
declaration   = global | function | start | update | draw ;
global        = "let" identifier ":" type "=" expression ";"? ;
function      = "fn" identifier "(" parameters? ")" "->" type block ;
parameters    = parameter ("," parameter)* ;
parameter     = identifier ":" type ;
start         = "start" block ;
update        = "update" "(" identifier ":" type ")" block ;
draw          = "draw" block ;
type          = "i32" | "f32" | "bool" ;

block         = "{" statement* "}" ;
statement     = local | assignment | if | while | return | expression ";"? ;
local         = "let" identifier ":" type "=" expression ";"? ;
assignment    = identifier "=" expression ";"? ;
if            = "if" expression block ("else" block)? ;
while         = "while" expression block ;
return        = "return" expression? ";"? ;

expression    = equality ;
equality      = comparison (("==" | "!=") comparison)* ;
comparison    = term (("<" | "<=" | ">" | ">=") term)* ;
term          = factor (("+" | "-") factor)* ;
factor        = unary (("*" | "/") unary)* ;
unary         = ("-" | "!") unary | primary ;
primary       = integer | float | "true" | "false"
              | identifier | identifier "(" arguments? ")"
              | "(" expression ")" ;
arguments     = expression ("," expression)* ;
```

## Semantics

- Every variable is mutable and has an explicit `i32`, `f32`, or `bool` type.
- There are no implicit conversions. Both operands of a binary operator must
  have the same type.
- Arithmetic and ordering work on `i32` and `f32`. Equality also works on
  `bool`. Conditions must be `bool`.
- Locals use lexical block scope and may shadow names from outer scopes.
- Named functions have typed parameters and a required return type. Every path
  through a named function must return a value.
- A game must declare exactly one `start`, `update(dt: f32)`, and `draw` entry.
  CRuMB calls them; source code does not declare `main`.
- The available CRuMB debug functions are `print_i32(value: i32)` and
  `debug_frame(frame: i32, value: f32)`. Both return no value.
- The software graphics functions are `clear_rgb(r: i32, g: i32, b: i32)` and
  `fill_rect(x: i32, y: i32, width: i32, height: i32, r: i32, g: i32, b: i32)`.
  Both return no value, and no graphics-specific language type is introduced.
- RGB components are clamped to 0 through 255. Filled rectangles use half-open
  bounds, are clipped to the 320x180 framebuffer, and do nothing when width or
  height is non-positive or the rectangle is wholly outside the framebuffer.
- Global initializers are currently restricted to numeric or Boolean literals,
  optionally with unary `-` for numeric literals.
- `i32` division is signed integer division. Floating comparisons are ordered,
  so comparisons involving NaN are false; source literals themselves must be
  finite.

The quoted game title is compile-time metadata, not a general-purpose string
value. Strings are otherwise absent from the type system.
