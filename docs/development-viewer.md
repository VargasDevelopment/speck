# Development browser viewer

The browser viewer displays a native CRuMB framebuffer from a headless host. It
is a replaceable development presenter, not Speck's graphics system and not a
browser build of the game.

## Anfibio and Mac workflow

On Anfibio, from the Speck repository:

```sh
cargo run -- dev examples/moving_rectangle.spk
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

Development mode defaults to 1,800 frames, approximately 30 seconds at 60 Hz.
Set an explicit limit when useful:

```sh
cargo run -- dev examples/moving_rectangle.spk --frames 6000
```

Ctrl-C stops the native game and both server threads. Normal game completion or
a game crash stops the server; a nonzero game status is reported. Debug output
continues to use the game's stdout/stderr and never shares the binary frame
channel.

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

CRuMB calls a private three-function presentation boundary after `spk_draw`:
presenter initialization, frame presentation, and presenter shutdown.

- `speck build` compiles and links the PPM presenter. It preserves the fixed
  five-frame deterministic loop and writes `build/frame.ppm`.
- `speck dev` produces separately named `_dev` artifacts and links the loopback
  stream presenter. It also enables a finite, paced development loop controlled
  by `SPECK_FRAME_LIMIT` internally.
- `speck run` produces separately named `_native` artifacts on macOS ARM64 and
  links the Cocoa presenter. It runs without a frame limit by default; the CLI's
  `--frames` option supplies a bounded test override.

Neither selection changes the Speck source, built-ins, framebuffer drawing
operations, or stable `crumb.h` ABI. The transport environment variables are a
private contract between the development command and its development runtime.

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

## Browser delivery and dependencies

The Rust host serves a small embedded HTML page and exposes the latest complete
frame through an HTTP long-polling endpoint. The canvas converts RGB8 to RGBA,
disables image smoothing, preserves 16:9, and prefers integer nearest-neighbor
scaling. It reports waiting, live, stopped, and disconnected states. No browser
is required by automated tests.

The HTTP server and protocol implementation use only Rust's standard library.
The compiler adds `ctrlc` for portable Ctrl-C handling; its transitive platform
support is development tooling. The viewer HTML is embedded in the compiler.
None of those bytes are compiled or linked into a normal game. The `_dev`
stream presenter is likewise omitted from `speck build`, while the normal PPM
presenter is omitted from the `_dev` game.
