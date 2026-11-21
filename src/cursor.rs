//! Validated image data for a native, window-local mouse cursor.
#![cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]

use std::sync::Arc;

/// An immutable cursor image. Cloning shares the pixels, so keep one per game state.
///
/// Pixels are straight-alpha RGBA8, tightly packed in top-to-bottom row order.
/// Dimensions and the hotspot are in image pixels, not game/world coordinates.
/// Images are not automatically resized for monitor DPI. Supply a larger image
/// (and proportionally larger hotspot) when a larger native cursor is desired.
#[derive(Clone, Debug)]
pub struct CustomCursor(pub(crate) Arc<CursorImage>);

#[derive(Debug)]
pub(crate) struct CursorImage {
    pub width: u32,
    pub height: u32,
    pub hotspot: (u32, u32),
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    InvalidDimensions,
    InvalidPixels,
    InvalidHotspot,
    UnsupportedPlatform,
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidDimensions => "cursor dimensions must each be between 1 and 256 pixels",
            Self::InvalidPixels => {
                "cursor pixels must contain exactly width * height * 4 RGBA bytes"
            }
            Self::InvalidHotspot => "cursor hotspot must be inside the image",
            Self::UnsupportedPlatform => "custom native cursors are supported on Windows and Linux",
        })
    }
}

impl std::error::Error for CursorError {}

impl CustomCursor {
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        hotspot: (u32, u32),
    ) -> Result<Self, CursorError> {
        if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
            return Err(CursorError::InvalidDimensions);
        }
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(CursorError::InvalidPixels);
        }
        if hotspot.0 >= width || hotspot.1 >= height {
            return Err(CursorError::InvalidHotspot);
        }
        Ok(Self(Arc::new(CursorImage {
            width,
            height,
            hotspot,
            rgba: rgba.to_vec(),
        })))
    }

    pub(crate) fn same_image(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl CursorImage {
    /// Native ARGB words (BGRA bytes on little-endian Windows/Linux).
    pub(crate) fn premultiplied_argb(&self) -> Vec<u32> {
        self.rgba
            .chunks_exact(4)
            .map(|p| {
                let a = u32::from(p[3]);
                let premultiply = |c| (u32::from(c) * a + 127) / 255;
                (a << 24) | (premultiply(p[0]) << 16) | (premultiply(p[1]) << 8) | premultiply(p[2])
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_images_before_native_code() {
        assert_eq!(
            CustomCursor::from_rgba(0, 1, &[], (0, 0)).unwrap_err(),
            CursorError::InvalidDimensions
        );
        assert_eq!(
            CustomCursor::from_rgba(u32::MAX, u32::MAX, &[], (0, 0)).unwrap_err(),
            CursorError::InvalidDimensions
        );
        assert_eq!(
            CustomCursor::from_rgba(2, 2, &[0; 15], (0, 0)).unwrap_err(),
            CursorError::InvalidPixels
        );
        assert_eq!(
            CustomCursor::from_rgba(2, 2, &[0; 16], (2, 0)).unwrap_err(),
            CursorError::InvalidHotspot
        );
        assert_eq!(
            CustomCursor::from_rgba(2, 2, &[0; 16], (0, 2)).unwrap_err(),
            CursorError::InvalidHotspot
        );
        assert!(CustomCursor::from_rgba(2, 2, &[0; 16], (1, 1)).is_ok());
    }

    #[test]
    fn converts_alpha_and_channel_order_without_color_fringes() {
        let cursor = CustomCursor::from_rgba(
            3,
            1,
            &[255, 100, 20, 128, 255, 100, 20, 0, 255, 100, 20, 255],
            (0, 0),
        )
        .unwrap();
        assert_eq!(cursor.0.premultiplied_argb(), [0x8080320a, 0, 0xffff6414]);
        assert!(cursor.same_image(&cursor.clone()));
    }
}
