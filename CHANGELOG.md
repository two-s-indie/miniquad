# Changelog

Completed changes in the [two-s-indie Miniquad fork](https://github.com/two-s-indie/miniquad), starting after upstream commit [`39c928f`](https://github.com/not-fl3/miniquad/commit/39c928fb7e09fd44e45f37fe9a98503968bca412). Changes inherited from that upstream baseline are excluded. Dated sections record development history, not published releases.

## Unreleased

### Added

- Added this fork changelog and repository instructions in `AGENTS.md` requiring changelog updates with commits and recurring checks for undocumented changes.

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
