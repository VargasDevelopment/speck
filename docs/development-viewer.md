# Development browser viewer

The browser viewer displays a native CRuMB framebuffer from a headless host. It
is a replaceable development presenter, not Speck's graphics system and not a
browser build of the game.

## Anfibio and Mac workflow

On Anfibio, from the Speck repository:

```sh
cargo run -- dev examples/keyboard_rectangle.spk --port 0
```

The command builds and launches a native `_dev` executable, starts the viewer
server, and prints lines like:

```text
Viewer URL: http://127.0.0.1:8787/
Remote access: ssh -L 8787:localhost:8787 <anfibio-host>
Then open: http://localhost:8787/
```

On the Mac, keep this tunnel open:

```sh
ssh -L 8787:localhost:8787 <anfibio-host>
```

Then open `http://localhost:8787/` in a browser. The server binds only to
Anfibio's loopback interface by default, so the tunnel is the intended remote
access path. If port 8787 is already occupied and `--port` was not specified,
Speck selects a safe ephemeral port and prints it. Use that printed port on both
sides of the SSH forwarding command.

Development mode is unbounded by default. A Speck call to `quit()`, Ctrl-C,
game failure, or host shutdown stops the child and server together. Set an
explicit limit for deterministic automation:

```sh
cargo run -- dev examples/keyboard_rectangle.spk --port 0 --frames 120
```

Ctrl-C is forwarded as SIGINT so CRuMB performs its normal shutdown, then the
HTTP, input-watchdog, and frame-receiver threads stop. A two-second deadline
prevents an unresponsive child from becoming an orphan. Normal game completion
or a game crash stops the server; a nonzero game status is reported. Debug
output continues to use the game's stdout/stderr and never shares the binary
frame/control channel.

## Deliberate non-local binding

When both machines are on the same tailnet, the SSH tunnel remains the most
isolated option and can use Anfibio's MagicDNS name:

```sh
ssh -L 8787:localhost:8787 anfibio
```

Alternatively, expose the viewer directly to permitted tailnet peers by
binding specifically to Anfibio's Tailscale IPv4 address:

```sh
cargo run -- dev examples/moving_rectangle.spk --bind "$(tailscale ip -4)" --port 8787
```

Then open `http://<anfibio-tailscale-ip>:8787/` from the Mac. Tailscale encrypts
the tailnet connection and enforces the tailnet's grants or ACLs, but the Speck
viewer itself still has no application-level authentication or TLS. The command
therefore prints a warning for every non-loopback bind; bind to the specific
Tailscale address rather than `0.0.0.0` or an untrusted interface. An explicitly
requested occupied port produces an error instead of silently choosing a
different one.

## Presentation selection

CRuMB calls a private four-function presentation boundary: presenter
initialization, nonblocking event polling before `spk_update`, frame
presentation after `spk_draw`, and presenter shutdown.

- `speck build` compiles and links the PPM presenter. It preserves the fixed
  five-frame deterministic loop and writes `build/frame.ppm`.
- `speck dev` produces separately named `_dev` artifacts and links the loopback
  stream presenter. Its paced loop is unbounded unless `--frames N` sets the
  private `SPECK_FRAME_LIMIT` override.
- `speck run` produces separately named `_native` artifacts on macOS ARM64 and
  links the Cocoa presenter. It runs without a frame limit by default; the CLI's
  `--frames` option supplies a bounded test override.

Neither selection changes the Speck source, framebuffer drawing operations, or
portable input semantics. The transport environment variables are a private
contract between the development command and its development runtime.

## Native frame protocol

The `_dev` process connects to a compiler-owned TCP listener on `127.0.0.1`.
Loopback TCP was chosen because it is reliable, ordered, dependency-free in C,
and portable across the supported Linux and macOS POSIX hosts. The listener
uses an ephemeral private port and is never exposed to the browser or network.

Every frame is a 24-byte big-endian header followed immediately by one complete
raw RGB payload:

| Offset | Bytes | Field | Required value |
| ---: | ---: | --- | --- |
| 0 | 4 | Magic | ASCII `SPKF` |
| 4 | 1 | Version | `1` |
| 5 | 1 | Pixel format | `1` (`RGB8`) |
| 6 | 2 | Header length | `24` |
| 8 | 2 | Width | `320` |
| 10 | 2 | Height | `180` |
| 12 | 4 | Payload length | `172800` |
| 16 | 8 | Sequence number | Monotonically increasing, starting at `1` |
| 24 | 172800 | Pixel payload | Packed row-major red, green, blue bytes |

The receiver rejects invalid metadata, out-of-order sequences, cleanly
truncated headers, and truncated payloads. It publishes a frame only after the
entire validated payload arrives.

## Browser input protocol

The browser sends only `POST /input` requests with a `text/plain` body of at
most 128 bytes. Each body has exactly three ASCII fields:

```text
<client-id> down <KeyboardEvent.code>
<client-id> up <KeyboardEvent.code>
<client-id> release -
<client-id> heartbeat -
```

Client IDs contain 1–64 ASCII letters, digits, hyphens, or underscores. The
accepted browser codes are `KeyW`, `KeyA`, `KeyS`, `KeyD`, `ArrowUp`,
`ArrowDown`, `ArrowLeft`, `ArrowRight`, `Space`, `Enter`, and `Escape`.
Unsupported codes are ignored. Invalid UTF-8, field counts, kinds, client IDs,
truncated messages, and oversized bodies receive a safe error response.
The page serializes ordinary control requests so separate HTTP connections
cannot reorder a keydown and its later keyup.

One client owns control at a time. Its 250 ms heartbeat renews a one-second
lease; another client receives HTTP 409 and remains view-only until ownership
is released or expires. Blur, document hiding, and page exit send release-all.
If those messages cannot arrive because the browser, tunnel, or network
vanishes, lease expiry sends release-all. Thus a disconnected controller cannot
leave a key held indefinitely.

The Rust host translates validated browser messages into an 8-byte binary
record sent over the reverse direction of the existing loopback TCP socket:

| Offset | Bytes | Field | Values |
| ---: | ---: | --- | --- |
| 0 | 4 | Magic | ASCII `SPKI` |
| 4 | 1 | Version | `1` |
| 5 | 1 | Kind | `1` key transition, `2` release-all |
| 6 | 1 | Key | CRuMB identifier `0` through `10`; zero for release-all |
| 7 | 1 | State | `0` up, `1` down; zero for release-all |

The C stream presenter accumulates partial records, validates every field, and
ignores malformed or unsupported records. Frame records continue in the other
direction with their original format, so logs, raw RGB payloads, and control
messages never share an unframed channel.

## Browser delivery and dependencies

The Rust host serves a small embedded HTML page and exposes the latest complete
frame through an HTTP long-polling endpoint. The canvas converts RGB8 to RGBA,
disables image smoothing, preserves 16:9, and prefers integer nearest-neighbor
scaling. The page translates `KeyboardEvent.code`, prevents interfering browser
defaults for supported game keys, and ignores repeat keydowns. It separately
reports framebuffer and controller connected/disconnected states. No browser
is required by automated tests.

The HTTP server and protocol implementation use only Rust's standard library.
The compiler adds `ctrlc` for portable Ctrl-C handling; its transitive platform
support is development tooling. The viewer HTML is embedded in the compiler.
None of those bytes are compiled or linked into a normal game. The `_dev`
stream presenter is likewise omitted from `speck build`, while the normal PPM
presenter is omitted from the `_dev` game.

## Acceptance commands

On macOS ARM64, exercise the same source in Cocoa:

```sh
cargo run -- run examples/keyboard_rectangle.spk
cargo run -- run examples/keyboard_rectangle.spk --frames 120
```

On Anfibio, run it through the browser presenter and use the printed ephemeral
port in the SSH tunnel:

```sh
cargo run -- dev examples/keyboard_rectangle.spk --port 0
ssh -L <printed-port>:localhost:<printed-port> anfibio
```

A/D and arrows move, Space toggles once per press, and Escape calls `quit()` in
the example. Presenter-specific key codes never enter Speck. Movement rules and
all later game mechanics remain user-authored `.spk` code.
