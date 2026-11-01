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

## Step 4: geometry and event delivery (2026-11-01)

Step 3 already established actual client-pixel sizing, size-only resize callbacks,
signed DPI placement, and shared main/modal delivery. Step 4 adds checked geometry
snapshots and closes gaps around initialization, native message nesting, recovery,
and shutdown.

Geometry queries now distinguish failure from an unchanged result. Reversed,
overflowing, and empty normal client dimensions are rejected; the previous public
snapshot remains intact and callbacks/drawing wait for valid geometry. A minimized
client keeps the existing nonzero policy (an empty client reports 1 x 1). Recovery
requests a frame even if the resulting size equals the last valid size. Native
positions remain signed internally; the existing public `u32` API is unchanged.

`WM_WINDOWPOSCHANGED` and its nested size/move messages finish before callbacks
are delivered. Non-client calculation does not trigger premature notification.
The backend queries actual dimensions rather than trusting the truncated fields
in `WM_SIZE`. The Win32 nesting follows the documented
[default window-position handling](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-windowposchanged).

The native payload is attached after shared state and WGL are ready, before
`ShowWindow` and the application factory. DPI/geometry messages during that phase
update public state and queue events until the handler exists. Showing the window
does not replace logical startup intent with its rounded physical size.

Ordinary native resizes now wake a blocking `GetMessage` wait, just as DPI changes
do. Nested move/menu loops share one active redraw timer until the final exit;
already-queued timer messages are ignored afterward. This matters because
[`KillTimer` does not remove queued timer messages](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-killtimer).
`GetMessage` quit/error results stop the loop without dispatching stale message
data. An accepted quit or native destruction stops further callbacks/drawing,
and teardown detaches the payload before deleting the graphics context.

### Step 4 validation results

The full Windows all-target test suite passed. The startup suite now includes
eleven isolated cases, including destruction from inside `update`; the four
runtime cases still cover high/low DPI with normal/blocking loops. New checks
verify:

- DPI messages during the application factory update public geometry before a
  handler is returned.
- Position-only moves and duplicate `WM_SIZE` messages do not emit resize callbacks;
  callbacks observe client dimensions and position from the same native state.
- A native resize produces exactly one resize callback and wakes blocking mode
  without a posted test wake-up or user input.
- A real empty normal client area, produced through a test `WM_NCCALCSIZE` hook,
  does not publish a fabricated resize, draw, or deliver queued input. Restoring
  valid geometry at the same size resumes input and redraws without a resize event.
- Nested move/menu timers continue until the outer loop exits. Replayed stale
  timer messages do not draw after exit.
- A canceled close keeps the window running; an accepted close stops callbacks.
  Ordering quit or destroying the window during `update` does not call `draw`.

Checked-geometry unit tests cover failed HWND queries, negative and overflowing
dimensions, signed positions, fractional-scale pixels, and minimized dimensions.
Rust 1.70 Windows library compilation and all-target Linux/Wasm checks passed.
The paired Macroquad diagnostic was rebuilt against this checkout and passed the
same high/low-DPI blocking transition checks recorded in step 3; logs are in
`target/dpi-validation/step4-macroquad-{high,low}.log` (not versioned). Existing
unrelated compiler and incremental-cache warnings remain.

These tests use real windows but injected DPI messages. They do not replace the
physical-monitor and interactive integration checks below.

## Step 5: Macroquad integration (2026-11-01)

The paired Macroquad branch now rounds Windows runtime size requests to the
nearest framebuffer pixel. Previously an 801 x 601 request at 175% truncated to
1401 x 1051, although startup produced 1402 x 1052. The integration audit also
found Windows button events using the previous mouse position and composition
startup overwriting an explicit IME anchor. Button events now use their own
signed client coordinates; IME startup retains the requested pixel anchor.
Releasing a cursor grab refreshes absolute mouse coordinates without waiting
for pointer movement. Failed/empty clip-rectangle queries are ignored.

### Coordinate contract

These Windows rules apply to the paired fork branches:

| API / value | Units and behavior |
| --- | --- |
| Miniquad `Conf.window_width`, `window_height` | Logical client dimensions for normal high-DPI startup; multiplied by the actual window's rendering scale and rounded once. |
| Miniquad `window::screen_size()` | Actual client/framebuffer pixels, subject to the documented nonzero minimized policy. |
| Miniquad `window::dpi_scale()` / Macroquad `screen_dpi_scale()` | Current rendering scale, published before resize callbacks or drawing. |
| Miniquad `window::set_window_size()` on Windows | Client/framebuffer pixels, already scaled by the caller. No additional DPI multiplication. |
| Macroquad `screen_width()`, `screen_height()` | Actual framebuffer divided by current scale. Fractional logical dimensions are expected after pixel rounding. |
| Macroquad `request_new_screen_size()` on Windows | Logical client dimensions, multiplied by current scale and rounded once; the result arrives asynchronously and remains subject to OS constraints. |
| Miniquad absolute mouse events | Signed client pixels, matching the framebuffer coordinate system. |
| Macroquad `mouse_position()` | Logical client coordinates, matching drawing and hit testing. While grabbed, the cache integrates raw relative motion and describes a virtual position. |
| Miniquad raw relative mouse events | Hardware motion units; the DPI transition does not rescale them. |
| Windows cursor clipping | Client-pixel corners converted to signed screen coordinates. Updated after geometry/DPI changes. |
| Miniquad `window::set_ime_position()` on Windows | Client pixels for composition/candidate placement; callers resubmit after DPI or layout changes. |

For 801 x 601 logical units, startup and a fresh Macroquad runtime request produce:

| Scale | Framebuffer pixels | Reported logical size (approximately) |
| --- | --- | --- |
| 1 | 801 x 601 | 801 x 601 |
| 1.25 | 1001 x 751 | 800.8 x 600.8 |
| 1.5 | 1202 x 902 | 801.3333 x 601.3333 |
| 1.75 | 1402 x 1052 | 801.1429 x 601.1429 |
| 2 | 1602 x 1202 | 801 x 601 |

Positive half pixels round upward. Each boundary can differ from the requested
logical size by at most half a physical pixel before OS constraints. Startup
retains the original logical intent; runtime Miniquad requests can retain only
the submitted integer pixels divided by the scale at request time. Subsequent
DPI transitions preserve that intent without repeatedly feeding rounded results
back into it. Applications should lay out against the reported screen size.

Macroquad already rasterizes and caches text at a DPI-dependent font size and
converts glyph geometry back to logical units. No text-renderer change was
needed. For an IME anchor supplied from Macroquad, convert once:

```rust
let scale = macroquad::window::screen_dpi_scale();
macroquad::window::miniquad::window::set_ime_position(
    (logical_cursor_x * scale).round() as i32,
    (logical_cursor_y * scale).round() as i32,
);
```

The IME may adjust its popup placement; the tests inspect the requested IMM
forms, not the final visible popup. See Microsoft's
[composition form](https://learn.microsoft.com/en-us/windows/win32/api/imm/ns-imm-compositionform),
[candidate form](https://learn.microsoft.com/en-us/windows/win32/api/imm/ns-imm-candidateform),
and [cursor clip coordinate contract](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-clipcursor).

### Awareness and compatibility limits

The executable's existing awareness remains authoritative. An explicitly unaware
window reports scale 1 through `GetDpiForWindow`; a system-aware window uses the
system DPI rather than tracking each monitor. Windows can virtualize or stretch
those windows. These fallback modes cannot promise native sharpness or per-monitor
transitions. Older API fallbacks are covered by unit tests, not older-OS hardware.
`high_dpi: false` keeps rendering scale 1 and unscaled client sizes even in a
PerMonitorV2 host; it does not introduce a low-resolution offscreen buffer.

The startup logical-size change is intentional compatibility behavior. Other
backends keep their existing native sizing contracts; the Macroquad rounding
change is Windows-only. Fullscreen entry still selects the primary monitor, the
public position API still uses `u32`, and no monitor-selection API was added.
Game Git dependency revisions remain unchanged until these branches are adopted.

### Integration evidence

The opt-in Macroquad `windows_dpi_integration` test creates real HWND/WGL windows
and exercises the public Macroquad API in four isolated processes: high/low DPI,
each with normal/blocking loops under PerMonitorV2 awareness. It checks GPU
readback for text and a centered hit-test rectangle, matches text measurements
to drawing, checks actual client dimensions independently, and tests even/odd
runtime size requests. Clicks are sent without preceding motion; release events
include negative coordinates. DPI awaits have no test polling timer or explicit
frame request. Explicit test commands and native monitor moves do request a frame.

Real native window moves were exercised between these connected displays:

| Display bounds in screen pixels | Native DPI | High-DPI client (801 x 601 intent) | Low-DPI client |
| --- | --- | --- | --- |
| (0, 0) to (3840, 2160) | 168 / 175% | 1402 x 1052 | 801 x 601 |
| (-2560, 720) to (0, 2160) | 96 / 100% | 801 x 601 | 801 x 601 |

The test moves out and back using native `SetWindowPos`; these DPI messages are
generated by Windows. Separate injected transitions cover 100%, 125%, 150%,
175%, 200%, 175%, and 100% without changing desktop settings. Cursor clipping is
queried at 125%, 200%, and 175%, followed by stationary-pointer verification on
release. IMM composition/candidate anchor queries pass at every injected scale,
including composition startup (seven checks in each of the four modes).

GPU readbacks at 125% and 175% were visually inspected: text is visible at the
appropriate pixel size and the hit-test rectangle is centered and green. The
default font's measured `MMMM` remains 56 x 16 logical units across the tested
scales. This is evidence for the default-font path, not every font or script.

To repeat from `../macroquad` (Windows 10 1703+, an interactive desktop, OpenGL,
and IMM support required):

```powershell
cargo tree --config 'patch.crates-io.miniquad.path="../miniquad"' -i miniquad
cargo test --test windows_dpi_integration --config 'patch.crates-io.miniquad.path="../miniquad"' -- --ignored --nocapture --test-threads=1
```

The test is ignored by default because it requires this paired Miniquad fork and
an interactive desktop. It briefly clips the cursor and restores the original
cursor position/clip afterward. Set `MACROQUAD_DPI_ARTIFACTS` to a directory to
retain framebuffer PNGs. Local results for this run are under
`target/dpi-validation/macroquad-step5-test.log` and `macroquad-step5-images/`
(not versioned).

The full Miniquad Windows all-target test suite passed, as did its Rust 1.70
library check and all-target Linux/Wasm checks. Macroquad's library, DPI example,
and integration-test target also passed Linux/Wasm compilation with the local
Miniquad override. Existing unrelated compiler and incremental-cache warnings
remain. Formatting and whitespace checks passed for the changed Rust files.

## Remaining physical-monitor checks

Record actual display scales, awareness mode, requested size, native client
size, reported framebuffer, logical size, mouse behavior, and redraw behavior.
Keep baseline observations separate from results after implementing the fix.

- Compare the SDL diagnostic on the same displays and retain its measurements.
- Check 100%, 125%, 150%, 175%, and 200% startup where displays are available.
- Physically drag between differently scaled monitors, including negative desktop
  positions, and repeat round trips with 801 x 601 intent to check for drift.
- Check runtime R/O requests for double scaling and pixel rounding.
- Test stationary-mouse and blocking-mode DPI changes without pressing P to wake
  the application; the visible frame counter/indicator must change on its own.
- Check normal resize, minimize/restore, maximize/restore, and fullscreen.
- Repeat the verified startup awareness modes across monitor transitions,
  including an embedded PerMonitorV2 manifest with high DPI disabled.
- Check cursor clipping, cached mouse coordinates, and IME placement. Use the
  existing `ime_test` example for IME behavior; this diagnostic has no text input.

Step 5 verifies native moves between the actual 100% and 175% displays, GPU text
and hit testing, cursor clip rectangles, and IMM anchor coordinates. The broader
interactive matrix above remains open: physical dragging, display-settings
changes, other real scale combinations, visible IME composition/candidate popups,
and SDL comparison measurements. Automated window moves and injected messages
do not establish those results.
