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

## Step 2: startup DPI and geometry

The Windows backend now establishes awareness before creating its main or
helper HWND. It tries PerMonitorV2, then the older per-monitor modes where
available, with system awareness as the legacy fallback. An already-established
process mode stops this sequence; thread overrides are never replaced. A
manifest remains the preferred way for an application to choose awareness.
See Microsoft's [process awareness contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext).

The hidden main window supplies its own DPI through `GetDpiForWindow`. That
query reflects the window's awareness, so an explicitly unaware host remains
at 96 DPI. On older Windows, the fallback queries the monitor associated with
that HWND, then its device context. A failed modern query falls back to 96 DPI
instead of substituting an unrelated monitor's scale. Newer functions and the
optional system Shcore DLL are resolved dynamically.
See [window DPI](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow).

Normal high-DPI startup converts the original logical request to client pixels,
rounds once, and uses `AdjustWindowRectExForDpi` for the non-client border. The
legacy border fallback is checked and reconciled once against actual client
geometry. Startup DPI is queried again after resizing, with bounded retries if
the larger window overlaps another monitor. OS-constrained sizes are reported
as they are. Fullscreen startup retains its existing placement policy and skips
normal-window sizing; both modes stay hidden until WGL is initialized.
See [DPI-aware border sizing](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectexfordpi).

This is a visible compatibility change: a normal 800 x 600 configuration with
high DPI enabled now means 800 x 600 logical units. The Windows runtime
`set_window_size` contract remains framebuffer pixels. Low-DPI mode does not
change host awareness and continues to request unscaled client dimensions with
rendering scale 1. Under a PerMonitorV2 host, this means native client pixels,
not a new low-resolution offscreen rendering layer.

### Recorded results: 2026-11-01

Real Windows startup checks compare `GetClientRect` and `GetDpiForWindow` with
Miniquad's public state inside the first application callback:

| Case | Rendering scale | Native client and reported framebuffer |
| --- | --- | --- |
| Standalone high DPI, 800 x 600 | 1.75 | 1400 x 1050 |
| Standalone high DPI, 801 x 601 | 1.75 | 1402 x 1052 |
| Non-resizable high-DPI window, 800 x 600 | 1.75 | 1400 x 1050 |
| Standalone low DPI, 800 x 600 | 1 | 800 x 600 |
| Existing PerMonitorV2 host, high DPI | 1.75 | 1400 x 1050 |
| Existing PerMonitorV2 host, low DPI | 1 | 800 x 600 |
| Existing system-aware host, high DPI | 1.75 | 1400 x 1050 |
| Explicitly unaware host, high-DPI preference | 1 | 800 x 600 |
| PerMonitorV2 process with unaware thread override | 1 | 800 x 600 |
| Embedded PerMonitorV2 manifest, high DPI | 1.75 | 1400 x 1050 |
| Embedded PerMonitorV2 manifest, low DPI | 1 | 800 x 600 |

A subsequent runtime request for 1173 x 853 framebuffer pixels was reported
as exactly 1173 x 853, without double scaling or truncation. A separate native
test confirms normal and fullscreen HWNDs are hidden before WGL initialization.

Checks passed:

- Full Windows `cargo test --offline --all-targets`, including ten isolated
  startup/runtime scenarios and five DPI unit tests.
- Two additional startup checks with an actual embedded PerMonitorV2 manifest.
- Rust 1.70 Windows library compilation.
- All-target compilation checks for Wasm and x86-64 Linux.
- Import-table inspection: no static Shcore dependency or newer DPI entry points.
- Rebuilt the paired Macroquad diagnostic with the local Miniquad patch. Its
  first-frame logs report 1400 x 1050 pixels / 800 x 600 logical units in high-DPI
  mode; 1402 x 1052 / approximately 801.1429 x 601.1429 for the odd-size request;
  and 800 x 600 / 800 x 600 in low-DPI mode. The odd logical result reflects
  unavoidable final-pixel rounding, not a second scaling operation.

Fallback failures and the 100%, 125%, 150%, 175%, and 200% rounding cases have
automated coverage. The real window runs above used 175%; older Windows and
startup on other monitors/scales have not been exercised on real hardware.

### Repeat the startup checks

```powershell
cargo test --test windows_dpi_startup -- --nocapture
cargo test --lib native::windows::dpi::tests
```

To test a real embedded manifest with the Windows MSVC toolchain:

```powershell
$dpiManifest = (Resolve-Path tests/fixtures/dpi-per-monitor-v2.manifest).Path
cargo rustc --test windows_dpi_startup --target-dir target/dpi-manifest -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTINPUT:$dpiManifest"
$dpiTests = @(Get-ChildItem target/dpi-manifest/debug/deps/windows_dpi_startup-*.exe)
if ($dpiTests.Count -ne 1) { throw 'Expected one manifest test executable' }
try {
    $env:MINIQUAD_DPI_STARTUP_CASE = 'manifest-pmv2'
    & $dpiTests[0].FullName --exact startup_contract --nocapture
    $env:MINIQUAD_DPI_STARTUP_CASE = 'manifest-pmv2-low'
    & $dpiTests[0].FullName --exact startup_contract --nocapture
} finally {
    Remove-Item Env:MINIQUAD_DPI_STARTUP_CASE -ErrorAction SilentlyContinue
}
```

The manifest cases assert that awareness was already set before Miniquad ran.
These startup tests do not validate dynamic DPI transitions; `WM_DPICHANGED`
handling remains outside this step.

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
- Repeat the verified startup awareness modes across monitor transitions,
  including an embedded PerMonitorV2 manifest with high DPI disabled.
- Check cursor clipping, cached mouse coordinates, and IME placement. Use the
  existing `ime_test` example for IME behavior; this diagnostic has no text input.

These monitor-transition checks remain unverified. Passing startup checks and
builds is not evidence of correct monitor-transition behavior.
