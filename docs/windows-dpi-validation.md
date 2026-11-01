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

## Step 3: runtime transitions (2026-11-01)

`WM_DPICHANGED` now copies the message rectangle and retains the new DPI for a
geometry transaction. Normal windows use the suggested signed position, stable
logical client-size intent, and target-DPI borders. The suggested outer size is
not multiplied again. New DPI messages raised during sizing are handled before
the resulting scale, actual client size, and position are published together.

Application framebuffer requests and native/user resizes replace the normal-size
intent. The rounded result of a DPI resize does not. If Windows constrains a DPI
resize, the actual client dimensions are published, but the original intent is
retained for a later transition. Border correction is bounded; there is no frame
polling loop that repeatedly forces the requested size.

Minimized windows remain minimized. Maximized windows use their suggested outer
geometry; fullscreen windows cover the destination monitor. These states retain
normal-size intent for restore. Leaving fullscreen restores saved windowed
placement through `SetWindowPlacement`; fullscreen entry still uses the existing
primary-monitor policy. Low-DPI rendering keeps scale 1 and unscaled client size,
while non-client borders use the window DPI.

Native geometry operations buffer application events. Callbacks run after the
transaction, with the handler owned outside the native state and no display mutex
held. The main loop and modal timer use the same publication/delivery path.
Only framebuffer changes cause `resize_event`; scale-only changes still request
a frame. A posted wake-up also handles DPI messages sent while `GetMessage` is
waiting. Absolute mouse coordinates are refreshed from the stationary OS cursor
after a transition; raw relative motion is unchanged.

### Automated results

The full Windows `cargo test --offline --all-targets -- --nocapture` passed,
including the earlier startup cases and four new isolated runtime cases: high
DPI and low DPI, each with normal and blocking event loops in a PerMonitorV2
process. The runtime tests create real HWND/WGL windows and inject
`WM_DPICHANGED`; they do **not** change the actual monitor DPI.

Verified by these injected-message tests:

- Three round trips through 100%, 125%, 150%, 175%, and 200% with 801 x 601
  logical intent, without accumulated size drift or intermediate resize callbacks.
- Signed placement at (-20, -10), native client/public framebuffer agreement,
  and the new scale visible inside resize callbacks.
- A 1173 x 853 framebuffer request and a 987 x 731 native resize both become
  the new intent and survive subsequent DPI round trips.
- Minimize/maximize and fullscreen restore, including a scale-only maximized
  transition that redraws without a resize callback.
- Stationary pointer refresh, blocking-loop wake-up without input, modal timer
  delivery, and a nested DPI message sent from inside a resize callback.

Rational-size unit tests cover repeated rounding and invalid/overflowing sizes.
Rust 1.70 Windows library compilation and all-target Linux/Wasm checks also
passed. Existing compiler warnings remain in unrelated code; Cargo also reports
an incremental-cache finalization warning on this machine.

The sibling Macroquad diagnostic was rebuilt with the local Miniquad patch and
run in blocking mode with odd dimensions. Injected transitions produced:

| Rendering scale | Framebuffer | Macroquad logical size (approximately) |
| --- | --- | --- |
| 1.25 | 1001 x 751 | 800.8 x 600.8 |
| 1.5 | 1202 x 902 | 801.3333 x 601.3333 |
| 1.75 | 1402 x 1052 | 801.1429 x 601.1429 |
| 2 | 1602 x 1202 | 801 x 601 |
| 1 | 801 x 601 | 801 x 601 |

It redrew after each transition and refreshed its logical mouse coordinates.
The low-DPI diagnostic retained scale 1 and 801 x 601 for all injected values.
These checks required no additional Macroquad source changes. Local logs are in
`target/dpi-validation/step3-macroquad-{high,low}.log` (not versioned).

Repeat the committed runtime checks with:

```powershell
cargo test --test windows_dpi_transition -- --nocapture
cargo test --lib native::windows::dpi::tests
```

## Remaining physical-monitor checks

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

These physical-monitor checks remain unverified. Passing startup, injected DPI
message tests, and builds does not verify Windows' actual monitor selection,
message ordering during a physical drag, or display-settings changes. Visual
text/hit-test accuracy, cursor grabbing and IME still need interactive validation.
