# Windows DPI diagnostic

The paired diagnostics capture Miniquad and Macroquad behavior before and after
the Windows DPI implementation. They do not compensate for startup sizing or
poll Windows for DPI changes. A stationary blocking-mode window must be woken
by the backend when its DPI changes.

## Run Miniquad

From this checkout:

```powershell
cargo run --example dpi_diagnostic
cargo run --example dpi_diagnostic -- --blocking
cargo run --example dpi_diagnostic -- --odd-size
cargo run --example dpi_diagnostic -- --low-dpi
```

Flags can be combined. The default configuration requests an 800 x 600 window
with high DPI enabled; `--odd-size` requests 801 x 601. No startup resize request
is issued by the diagnostic. `--low-dpi` sets `Conf.high_dpi` to false; it does
not override awareness established by an executable manifest or host.

The console logs startup, geometry changes, resize callback arguments, mouse
coordinates, and minimize/restore callbacks. Each snapshot includes the frame
number, reported scale, reported framebuffer size, derived logical size, and
the last absolute mouse position in both framebuffer and logical coordinates.
Resize callbacks read the public state again to expose publication-order bugs.
The small bar at the top left changes color on each draw.

Both examples draw a centered 200 x 120 logical rectangle, turn it gold when
hovered, and show a crosshair at the reported mouse position. Macroquad also
renders text with the current measurements and frame count.

| Key | Action |
| --- | --- |
| R | Request 800 x 600 logical units at the current scale. |
| O | Request 801 x 601 logical units at the current scale. |
| F | Toggle fullscreen. |
| C | Toggle cursor grab. |
| P | Log a measurement snapshot. |
| Escape | Exit. |

The Miniquad runtime resize control converts logical units to framebuffer
pixels once and rounds to the nearest pixel. The Macroquad control uses its
existing `request_new_screen_size()` API directly, including its current
rounding behavior. These controls specifically exercise the Windows runtime
contract; they do not establish a new cross-platform size-request contract.
Raw relative mouse motion is not converted into logical units by the Miniquad
example. Cursor-grab behavior is deliberately observed through the existing APIs.

## Run the paired Macroquad example against this checkout

The Macroquad checkout must contain `examples/dpi_diagnostic.rs` from its
`feature/windows-dpi` branch. From `../macroquad`:

```powershell
cargo tree --config 'patch.crates-io.miniquad.path="../miniquad"' -i miniquad
cargo run --example dpi_diagnostic --config 'patch.crates-io.miniquad.path="../miniquad"'
cargo run --example dpi_diagnostic --config 'patch.crates-io.miniquad.path="../miniquad"' -- --blocking
```

Check that `cargo tree` names this local Miniquad directory. Without the patch,
Macroquad's manifest selects the registry dependency, which does not test this
fork. The command-line patch leaves the tracked dependency manifest unchanged.
The game retains its existing Git dependencies throughout this diagnostic work.

## Recorded baseline: 2026-11-01

Backend sources: Miniquad `2080ce3` (the `game` branch point) and Macroquad
`a28a268` (its local `game` branch point). Only the diagnostic examples were
added for these runs; neither library's behavior was changed.

Windows desktop, default high-DPI mode, non-blocking loop:

| Diagnostic | Requested size | Reported DPI | Reported framebuffer | Derived logical size |
| --- | --- | --- | --- | --- |
| Miniquad | 800 x 600 | 1.75 | 799 x 598 | 456.57144 x 341.7143 |
| Macroquad, local Miniquad patch | 800 x 600 | 1.75 | 799 x 598 | 456.57144 x 341.7143 |

Under the proposed logical-startup contract, an unconstrained 800 x 600 window
at 175% should have a 1400 x 1050 framebuffer and an 800 x 600 logical size.
The baseline reproduces the startup sizing mismatch and fractional-pixel loss.
The Macroquad window visibly rendered the rectangle, text, and mouse marker.
The native client rectangle was not measured independently during this run;
the table records engine queries, not an independent physical-size measurement.

Validation completed:

- Built the Miniquad diagnostic on Windows and checked its Wasm compilation.
- Built the Macroquad diagnostic on Windows with the local Miniquad patch;
  verified the resolved path using `cargo tree`.
- Launched both diagnostics and recorded their startup logs at reported 175%.
- Built and launched the existing SDL comparison from
  `D:/Projects/tmp/game-find-all/2d-game-experiments/sdl3-main-callbacks`.
  Its window closed before a baseline measurement was captured, so no SDL size
  or transition result is claimed.

Builds emitted existing library warnings and incremental-cache warnings. The
SDL link also reported an MSVC runtime-library conflict warning; the build
completed. No backend fixes or new shared APIs are part of this milestone.

## Remaining runtime checks

Record actual display scales, awareness mode, requested size, native client
size, reported framebuffer, logical size, mouse behavior, and redraw behavior.
Keep baseline observations separate from results after implementing the fix.

- Compare the SDL diagnostic on the same displays and retain its measurements.
- Check 100%, 125%, 150%, 175%, and 200% startup where displays are available.
- Move between differently scaled monitors, including negative desktop positions.
- Repeat round trips with 801 x 601 logical intent and verify no accumulated drift.
- Check runtime R/O requests for double scaling and pixel rounding.
- Test stationary-mouse and blocking-mode DPI changes without pressing P to wake
  the application; the visible frame counter/indicator must change on its own.
- Check normal resize, minimize/restore, maximize/restore, and fullscreen.
- Compare standalone awareness with a PerMonitorV2 executable manifest and
  repeat with high DPI disabled.
- Check cursor clipping, cached mouse coordinates, and IME placement. Use the
  existing `ime_test` example for IME behavior; this diagnostic has no text input.

These checks remain unverified by the initial startup runs. Build success is
not evidence of correct monitor-transition behavior.
