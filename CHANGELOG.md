# Changelog

Completed changes in the [two-s-indie Miniquad fork](https://github.com/two-s-indie/miniquad), starting after upstream commit [`39c928f`](https://github.com/not-fl3/miniquad/commit/39c928fb7e09fd44e45f37fe9a98503968bca412). Changes inherited from that upstream baseline are excluded. Dated sections record development history, not published releases.

## Unreleased

### Added

- Added this fork changelog and repository instructions in `AGENTS.md` requiring changelog updates with commits and recurring checks for undocumented changes.
- Added a DPI diagnostic example with geometry/input logging, a centered hit-test rectangle, runtime resize controls, and high-DPI, low-DPI, odd-size, and blocking-loop modes. Added a validation guide with the recorded 175% startup baseline and instructions for testing the paired Macroquad example against this checkout.
- Added Windows DPI fallback and rounding tests, real-window startup checks in isolated processes, and a PerMonitorV2 manifest fixture covering both high-DPI and low-DPI rendering.
- Added Windows runtime DPI regression tests using real windows and injected DPI messages: fractional round trips, signed positions, callback consistency and reentrancy, application/native resizing, minimize/maximize and fullscreen restore, stationary pointer refresh, and normal, blocking, and modal event loops.
- Extended Windows regressions to cover initialization-time DPI messages, empty-client recovery and queued input, position-only moves, duplicate size messages, nested and stale modal timers, quit cancellation, and window destruction during an update. Added checked-geometry tests for failed queries, invalid dimensions, and the nonzero minimized policy.
- Documented the paired Macroquad coordinate contract and integration results, including native moves between 100% and 175% displays, GPU text/hit-test readback, fractional runtime sizing, cursor clipping, and IMM anchor queries in high/low-DPI normal/blocking modes.

### Fixed

- **Windows input and IME integration:** Use each mouse-button message's signed client coordinates instead of a stale motion cache. Refresh absolute mouse coordinates after releasing a cursor grab, ignore failed or empty clip-rectangle queries, and retain explicitly requested IME pixel anchors when composition starts.
- **Windows startup DPI:** Establish per-monitor awareness before creating any window when the host has not already chosen an awareness mode. Query DPI from the actual main window and correct its hidden client geometry using DPI-aware border metrics before showing it after WGL initialization. Newer DPI APIs are loaded dynamically, with checked legacy and failed-query fallbacks.
- **Windows framebuffer dimensions:** Publish the actual client size without the fractional-scale divide/truncate/multiply round trip, and update size and position together. Normal and fullscreen startup windows remain hidden until the graphics context is ready.
- **Windows DPI transitions:** Handle `WM_DPICHANGED`, retain fractional scales, and publish the resulting scale, actual framebuffer size, and position together before callbacks. Normal windows use the suggested position and target-DPI borders while retaining logical client-size intent without cumulative rounding. Low-DPI rendering keeps scale 1 and unscaled client dimensions.
- **Windows window states and redraw:** Preserve minimized, maximized, and fullscreen states during DPI changes and apply normal-size intent on restore, including restoring saved windowed placement when leaving fullscreen. DPI changes wake blocking loops even when framebuffer size stays unchanged. Refresh absolute pointer coordinates and cursor clipping after transitions; raw relative motion remains in hardware units.
- **Windows callback delivery:** Buffer events during reentrant native geometry operations and share geometry publication between the main loop and modal resize timer. Resize callbacks report framebuffer changes rather than position-only moves, and observe the updated DPI. Runtime size requests use DPI-aware borders without scaling framebuffer dimensions again.
- **Windows geometry validation:** Retain a checked snapshot with signed native position, exact client pixels, and scale. Failed queries and invalid normal-window dimensions preserve the last valid public state and defer callbacks/drawing; recovery requests a frame even when the size is unchanged. Nested native size/move messages are published after non-client processing completes, without duplicate resize notifications.
- **Windows initialization and shutdown:** Handle DPI and geometry messages after WGL initialization but before the application factory returns, without calling an unavailable handler or replacing logical startup intent with rounded pixels. Stop callbacks and drawing after accepted quit or native window destruction, and detach window state before graphics teardown.
- **Windows event loops:** Wake blocking loops for ordinary native resizes, retain the modal redraw timer through nested move/menu loops, ignore stale timer messages after exit, and handle `GetMessage` errors and quit results without dispatching an invalid message.

### Changed

- **Windows high-DPI startup sizing:** `Conf.window_width` and `Conf.window_height` now specify logical client dimensions for normal high-DPI windows. For example, 800 x 600 at 175% becomes a 1400 x 1050 framebuffer. Runtime size requests remain in framebuffer pixels; low-DPI startup keeps unscaled dimensions and rendering scale 1, including in a per-monitor-aware host.

## 2026-11-01

### Fixed

- **Windows file drops:** Query each dropped path's length and allocate an initialized UTF-16 buffer of the required size, preserving paths at and beyond `MAX_PATH`, including Unicode paths. Skip failed path queries and unreadable files instead of panicking while processing a drop. ([`8e37cdc`](https://github.com/two-s-indie/miniquad/commit/8e37cdc405af2f55b66a46a0956155ad17cfb346))
- **Metal uniforms:** Validate the entire uniform write against the remaining buffer capacity before copying or advancing frame state. Reject overflowing lengths and invalid offsets while allowing writes that exactly fill the buffer. ([`80ba93e`](https://github.com/two-s-indie/miniquad/commit/80ba93ebc3929a23d2a923382059eef6b1c2b963))
- **X11 drag and drop:** Store the `XdndAware` version in a native C `long`, matching Xlib's format-32 property representation and avoiding an undersized source buffer on 64-bit systems. ([`1af79a6`](https://github.com/two-s-indie/miniquad/commit/1af79a631b49d41bfd01a899b28687a8cab87498))
- **X11 property reads:** Use the correct element sizes for 8-, 16-, and 32-bit properties, check byte-count overflow and slice limits, and handle failed queries or invalid buffers. Free intermediate allocations during incremental (`INCR`) transfers. ([`6bad3b5`](https://github.com/two-s-indie/miniquad/commit/6bad3b5ca8e14249213b2cfeb977cb8477d14b0b))

### Added

- Regression tests for Windows dropped-path lengths and Unicode, Metal uniform capacity boundaries, and X11 property element sizes and invalid lengths, alongside the corresponding fixes above.

### Changed

- Normalized Rust formatting in the IME example and platform backends. ([`1508486`](https://github.com/two-s-indie/miniquad/commit/15084866b1af2bfe8bf736e1136c38a41545640a))

## 2026-10-31

### Fixed

- **OpenGL uniforms:** Validate the complete uniform block before any upload, including arrays and uniforms optimized out by the driver. Use checked byte-size calculations and byte offsets to reject undersized input before reading it. ([`3d3860f`](https://github.com/two-s-indie/miniquad/commit/3d3860ffcde7f74e755feb03f6ed6a99ba39efd4))
- **OpenGL textures:** Validate creation, mipmap, resize, update, and readback buffer sizes and transfer dimensions with checked size calculations. Upload each mipmap with its own dimensions, use tightly packed rows for resize and readback, and restore the previous pack alignment after readback. ([`9c89903`](https://github.com/two-s-indie/miniquad/commit/9c89903b38c17588c31bdc90c89724d5e60d34b6))
- **Half-float textures and WebGL transfers:** Transfer `RGBA16F` pixels as four 16-bit half-float components (eight bytes per pixel), using `GL_HALF_FLOAT`. Select WebGL typed arrays and lengths from the pixel format and transfer type, including packed formats, and handle unaligned upload and readback buffers. Document the WebGL 2 requirement for `RGBA16F`. ([`d8ab522`](https://github.com/two-s-indie/miniquad/commit/d8ab52233ad61955f7e9f00379d76b3185ed9555))
- **Windows clipboard:** Bound UTF-16 reads by the allocation's size in code units, stop at the first NUL, ignore incomplete trailing code units, and release the global memory lock after reading. ([`bf62b6e`](https://github.com/two-s-indie/miniquad/commit/bf62b6e3464552a0e5a2761312102842d7b5acb8))
- **Windows file drops:** Initialize the path buffer before use, removing the assumption that a partially written buffer was fully initialized. The later long-path fix replaces this fixed-size buffer. ([`8dddd94`](https://github.com/two-s-indie/miniquad/commit/8dddd94b8df31a85457edc960225d0f8a4e0a076))

### Added

- A mock OpenGL test harness and regression tests for uniform bounds, texture buffer bounds, mipmap dimensions, row alignment, and half-float transfers.
- JavaScript regression coverage for WebGL transfer array types, sizes, and unaligned buffers, plus Windows clipboard tests for bounded reads, UTF-16 preservation, and memory unlocking.
