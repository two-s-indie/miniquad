# DPI fix plan

Status: proposed implementation; no engine changes made by this document.
Prepared: 2026-11-01.
Target: the two-s-indie Miniquad fork, with Macroquad integration checks.

## Goal

Make the Windows backend handle startup DPI and changes of monitor scale correctly, including fractional scales. Match the useful behavior of the existing SDL3 experiment: an 800 x 600 logical client area renders sharply at the current monitor scale and adapts when the window moves between monitors.

Keep drawing, framebuffer dimensions, mouse coordinates, and requested window sizes consistent. Fix this in Miniquad so Macroquad can consume the correct scale through its existing APIs.

## Reference projects

- Miniquad checkout: `D:/Projects/two-s-indie/miniquad`.
- Macroquad checkout: `D:/Projects/two-s-indie/macroquad`.
- Game crate: `D:/Projects/two-s-indie/Where-s-the-Yokai/Where-s-the-Yokai`.
- SDL3 comparison: `D:/Projects/tmp/game-find-all/2d-game-exeriments/sdl3-main-callbacks/src/main.rs` and its README.

The SDL3 example queries primary-display content scale before creating its window. At runtime it reads `window.display_scale()` and `window.pixel_density()`, derives window dimensions from the original logical size, updates renderer scaling, and triggers redraws on changes. It avoids normal-window resize requests while minimized, maximized, or fullscreen. It does not itself enumerate every connected monitor.

The game currently depends on Git revisions of the forks. Editing the sibling checkouts alone does not change the game build. Use an explicit local dependency override for integration testing, then update the Git dependency revision/lockfile deliberately when adopting the fix.

## Findings from the current source

| Location | Observed behavior | Consequence |
| --- | --- | --- |
| `src/native/windows.rs`, `WindowsDisplay::init_dpi` | Queries the monitor near `(1, 1)` once using `MonitorFromPoint` and `GetDpiForMonitor`. | Startup scale need not match the actual window monitor. |
| `src/native/windows.rs`, `run` | Stores `display.window_scale` in `NativeDisplayData.dpi_scale` once. | Public DPI queries remain at the startup value. |
| `src/native/windows.rs`, `win32_wndproc` | Has no `WM_DPICHANGED` handler. | Monitor moves and settings changes do not update engine DPI state. |
| `src/native/windows.rs`, `create_window` | Passes configured dimensions into initial client geometry without applying the discovered scale; DPI initialization happens later. | High-DPI startup sizing needs an explicit, consistent contract. |
| `src/native/windows.rs`, `set_window_size` | Uses `AdjustWindowRectEx`. | Non-client sizing is not explicitly calculated for the target DPI. |
| `src/native/windows.rs`, `update_dimensions` | Divides physical client dimensions by scale, truncates to integers, then multiplies again. | Fractional scales can lose framebuffer pixels. |
| `src/native/windows.rs`, `run` | Calls legacy `SetProcessDPIAware()` when high DPI is enabled. | Standalone applications without an appropriate manifest do not get a reliable per-monitor-awareness setup from this call alone. |
| `src/lib.rs`, `window` | Exposes framebuffer size and one window DPI scale, but no public monitor list or current-monitor identity. | Full monitor discovery would require a separate API. |

Macroquad's `src/window.rs` divides framebuffer dimensions by Miniquad's scale for `screen_width()` and `screen_height()`. `screen_dpi_scale()` forwards the scale. `request_new_screen_size()` multiplies logical dimensions by that scale before calling Miniquad. `src/input.rs` similarly divides mouse positions by the scale. Correct Miniquad state is therefore essential.

The game's `build.rs` already requests a `PerMonitorV2` manifest. That establishes awareness but does not implement DPI-change handling inside Miniquad.

These are source-inspection findings, not results of a runtime multi-monitor test.

## Scope

First implementation: Windows startup DPI, dynamic DPI changes, client/framebuffer geometry, coordinate consistency, and redraw behavior.

Preserve existing public entry points where possible. Do not add a new monitor-enumeration subsystem as a prerequisite. Keep other backends building; their dynamic DPI behavior needs its own verification before claiming a cross-platform fix.

## Coordinate and compatibility contract

Write down and test these rules before changing the implementation:

- In high-DPI mode, logical client size multiplied by the rendering scale gives physical client/framebuffer size, with rounding only at the final pixel boundary.
- `window::screen_size()` describes the actual framebuffer, not desktop or monitor resolution.
- `window::dpi_scale()` must describe the current rendering scale and be current before application callbacks observe new dimensions.
- On Windows with high DPI enabled, absolute Miniquad mouse coordinates must match framebuffer coordinates so Macroquad's existing division produces logical coordinates. Raw relative mouse motion is a different API; do not rescale its hardware units casually.
- Proposed startup behavior: treat `Conf.window_width` and `Conf.window_height` as logical dimensions when high DPI is enabled, consistent with the intent of the high-DPI configuration documentation. This corrects current Windows behavior and must be called out as a visible compatibility change.
- Preserve the existing Windows runtime `set_window_size` contract used by Macroquad: Macroquad has already multiplied its logical request by DPI. Do not multiply that request a second time. Audit other backend contracts before making any global API/documentation change.
- Preserve normal user resizing. The engine must not force every application back to 800 x 600; that is the comparison application's chosen logical size.
- Keep a stable logical normal-window size for DPI transitions. Update it on intentional application/user resizing, not from a rounded result of the engine's own DPI resize. Define how OS-constrained sizes are recorded without creating a resize loop.
- Treat OS DPI awareness and the `high_dpi` rendering preference separately. Explicitly test `high_dpi: false`, including in an application with a PerMonitorV2 manifest; do not silently redefine its existing behavior.

## Implementation sequence

### 1. Establish a reproducible diagnostic

- Add a focused Miniquad diagnostic example showing/logging reported DPI scale, actual framebuffer size, derived logical size, mouse position, and resize/DPI transitions.
- Add a corresponding small Macroquad integration example, or adapt a temporary test application, with an 800 x 600 logical client area, a centered rectangle, text, and a mouse marker.
- Run the existing SDL3 comparison on the same displays and record a baseline.
- Make the tested dependency source explicit so the Macroquad example definitely uses this Miniquad checkout.

### 2. Make startup DPI match the actual window

- Respect awareness already established by the executable manifest or host. Prefer the manifest for the game.
- For a standalone high-DPI application with no explicit awareness setup, establish an appropriate per-monitor context before creating any HWND, including helper windows. Do not interpret an already-set awareness context as permission to override the host.
- Prefer querying DPI from the actual HWND with `GetDpiForWindow` once it exists. Its result depends on the window's awareness context; a query cannot compensate for incorrect awareness.
- Keep the main window hidden while initial geometry is corrected. Use its actual monitor/DPI to convert the requested logical client size, calculate the non-client border, and establish the initial framebuffer state before showing it.
- Preserve the existing ordering that shows the window only after WGL initialization.
- Use DPI-aware border calculation where available, such as `AdjustWindowRectExForDpi`.
- Define checked fallbacks for unavailable APIs, zero/failed DPI queries, and older Windows versions. Resolve newer functions dynamically if retaining an older runtime baseline; do not accidentally raise it through unconditional imports.

Windows recommends manifest configuration for process awareness and reports an error when an API attempts to set an already-established process default. See [SetProcessDpiAwarenessContext](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocessdpiawarenesscontext).

`GetDpiForWindow` and `AdjustWindowRectExForDpi` require Windows 10 version 1607 or later when imported directly. See [window DPI query](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow) and [DPI-aware window rectangles](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectexfordpi).

### 3. Handle DPI changes as one coherent transition

- Handle `WM_DPICHANGED` and retain fractional scale values such as 1.25, 1.5, and 1.75.
- Capture the new DPI and copy the suggested rectangle while processing the message; do not retain the `lParam` pointer.
- Update backend scale state and `NativeDisplayData.dpi_scale` coherently with the resulting framebuffer dimensions.
- For a normal window, use the suggested placement and preserve the stable logical client-size intent with target-DPI non-client metrics. Avoid applying both the suggested scaled geometry and an additional scale multiplication.
- Preserve minimized, maximized, and fullscreen states. Update scale information without forcing these states into the normal-window 800 x 600 test geometry; apply pending normal-window intent appropriately on restore.
- Reconcile actual client dimensions after the OS applies or constrains the request.
- Ensure changes redraw even with a stationary mouse or `blocking_event_loop` enabled, including changes where framebuffer dimensions happen to stay the same.

`WM_DPICHANGED` covers moves between differently scaled monitors and changes to the hosting monitor's DPI. Its suggested rectangle describes outer window geometry. See [Microsoft's message contract](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged).

### 4. Make geometry and event delivery reliable

- In native high-DPI rendering, use the actual physical client rectangle for framebuffer dimensions; remove the divide-truncate-multiply round trip.
- Check the returned size before notifying clients, and maintain a safe minimized-window policy.
- Keep desktop positions signed internally. Monitors to the left or above the primary monitor have negative coordinates; avoid routing suggested rectangles through the current public `u32` position API. A public position-type change is separate compatibility work.
- Update dimension and position state together; do not let an early return leave one stale.
- Publish the updated scale before `resize_event` or subsequent drawing reads it. Deliver `resize_event` when framebuffer dimensions change and schedule an update for scale-only changes.
- A new default no-op DPI-change callback is optional follow-up API work. The first fix must work through the existing scale query and redraw path.
- Do not hold the native-display mutex or an application event-handler borrow across Win32 calls that may synchronously re-enter the window procedure, or across user callbacks. Review raw-pointer aliasing in this path as well as deadlock risk.
- Handle messages during creation/initialization safely; do not assume the event handler and global display state exist for every HWND message.
- Use one transition/notification path for the normal message loop and the modal move/resize timer so neither produces stale state or duplicate resize loops.

### 5. Verify Macroquad integration and document behavior

- Confirm `screen_width()`, `screen_height()`, `screen_dpi_scale()`, text rendering, mouse hit testing, and `request_new_screen_size()` remain consistent after transitions.
- Check for stale cached mouse coordinates when DPI changes without mouse movement.
- Verify cursor clipping and IME placement against their documented coordinate units after a scale change.
- Document startup dimensions, runtime size-request units, fractional scaling, and any limitations of fallback awareness modes.
- Keep the fix focused. Existing fullscreen placement and monitor-selection behavior should be recorded separately unless a change is necessary to avoid a regression caused by this fix.

## Likely files

| File | Expected work |
| --- | --- |
| `src/native/windows.rs` | Awareness setup, window-DPI query, startup sizing, DPI message handling, geometry, event scheduling, and focused tests. |
| `src/native.rs` | Shared state only if the existing fields are insufficient. |
| `src/conf.rs` | Clarify configuration size units and high-DPI behavior. |
| `src/lib.rs` | Clarify runtime query/request contracts; change only where justified. |
| `src/event.rs` | Only if a separate DPI-change callback is deliberately introduced. |
| `examples/` | Diagnostic that makes manual multi-monitor verification repeatable. |
| Macroquad `src/window.rs`, `src/input.rs`, and `src/lib.rs` | Integration audit; modify only for an actual remaining mismatch. |

## Validation

For an unconstrained normal window configured as 800 x 600 logical units:

| Display scale | Expected client/framebuffer pixels | Expected Macroquad logical size |
| --- | --- | --- |
| 100% | 800 x 600 | 800 x 600 |
| 125% | 1000 x 750 | 800 x 600 |
| 150% | 1200 x 900 | 800 x 600 |
| 175% | 1400 x 1050 | 800 x 600 |
| 200% | 1600 x 1200 | 800 x 600 |

Automated checks should target observable failures: fractional-size rounding, repeated DPI round trips without drift, no double scaling of runtime requests, invalid-query fallback, and consistent publication of dimensions/scale. Include odd logical sizes to expose rounding errors. Do not rely only on tests that repeat the implementation's arithmetic.

Manual Windows checks:

- [ ] Launch on displays with different scales; validate startup before the first visible frame.
- [ ] Move repeatedly between 100%, 150%, and 200% displays, including a monitor left/above the primary display.
- [ ] Change display scaling in Windows Settings while the application is open.
- [ ] Verify crisp text, the centered rectangle, and mouse hit testing at every scale.
- [ ] Verify redraw with no mouse movement and with the blocking event loop enabled.
- [ ] Manually resize, then move across displays; preserve the user's new logical size.
- [ ] Request a logical size through Macroquad; confirm it is scaled exactly once.
- [ ] Minimize/restore, maximize/restore, and enter/leave fullscreen around DPI changes.
- [ ] Repeat with PerMonitorV2 manifest, standalone awareness fallback, and `high_dpi: false`.
- [ ] Verify drag/resize modal loops, cursor grab, and IME placement do not regress.
- [ ] Confirm OS size constraints do not cause repeated resize requests or oscillation.

Run focused tests and the existing Windows checks, then the repository's existing cross-platform build matrix for shared API changes. Include the existing MSRV check if new language/library features are introduced. Record which real monitor/scale combinations were exercised; do not treat compile success as DPI behavior verification.

## Separate follow-up: monitor discovery

Full monitor enumeration is useful for display selection but is not required to fix the current window's DPI. If needed, design it separately with monitor identity, primary/current monitor queries, signed bounds/work area, scale, and lifecycle behavior.

Specify what is available before window creation and how unsupported platforms/browser environments report unavailable data. Do not assume Miniquad's single rendering scale has the same meaning as SDL3's separate content scale, display scale, and pixel density on every platform.

## Completion criteria

- [ ] Windows reports the actual window's current DPI scale at startup and after changes.
- [ ] The comparison application's logical size and input remain stable across fractional scales without accumulated drift.
- [ ] Framebuffer dimensions match the actual renderable client area.
- [ ] Existing Macroquad APIs work correctly without game-specific DPI polling workarounds.
- [ ] Redraw works for DPI-only changes and idle/blocking operation.
- [ ] Awareness, compatibility, and coordinate contracts are documented and tested.
- [ ] Real multi-monitor results and any remaining platform limitations are recorded.
