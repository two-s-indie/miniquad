use super::*;

/// Owns only a private cursor; shared system cursors must never be destroyed.
pub(super) struct NativeCursor {
    pub image: crate::CustomCursor,
    pub handle: HCURSOR,
}

impl NativeCursor {
    pub unsafe fn new(image: crate::CustomCursor) -> Result<Self, std::io::Error> {
        let data = &image.0;
        let mut header: BITMAPV5HEADER = std::mem::zeroed();
        header.bV5Size = std::mem::size_of::<BITMAPV5HEADER>() as _;
        header.bV5Width = data.width as i32;
        header.bV5Height = -(data.height as i32);
        header.bV5Planes = 1;
        header.bV5BitCount = 32;
        header.bV5Compression = BI_BITFIELDS;
        header.bV5RedMask = 0x00ff0000;
        header.bV5GreenMask = 0x0000ff00;
        header.bV5BlueMask = 0x000000ff;
        header.bV5AlphaMask = 0xff000000;

        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &header as *const _ as *const BITMAPINFO,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if color.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let pixels = data.premultiplied_argb();
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());

        // CreateBitmap expects WORD-aligned monochrome scan lines. Explicitly
        // set transparent bits, including the all-transparent-image case.
        let stride = (data.width as usize).div_ceil(16) * 2;
        let mut mask_bits = vec![0u8; stride * data.height as usize];
        for y in 0..data.height as usize {
            for x in 0..data.width as usize {
                if data.rgba[(y * data.width as usize + x) * 4 + 3] == 0 {
                    mask_bits[y * stride + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        let mask = CreateBitmap(
            data.width as _,
            data.height as _,
            1,
            1,
            mask_bits.as_ptr() as _,
        );
        if mask.is_null() {
            let error = std::io::Error::last_os_error();
            DeleteObject(color as _);
            return Err(error);
        }
        let mut info = ICONINFO {
            fIcon: 0,
            xHotspot: data.hotspot.0,
            yHotspot: data.hotspot.1,
            hbmMask: mask,
            hbmColor: color,
        };
        let handle = CreateIconIndirect(&mut info) as HCURSOR;
        let error = std::io::Error::last_os_error();
        // CreateIconIndirect copies both bitmaps.
        DeleteObject(mask as _);
        DeleteObject(color as _);
        if handle.is_null() {
            Err(error)
        } else {
            Ok(Self { image, handle })
        }
    }
}

impl Drop for NativeCursor {
    fn drop(&mut self) {
        unsafe {
            // Windows cannot destroy the cursor while it is selected.
            if GetCursor() == self.handle {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_ARROW));
            }
            DestroyCursor(self.handle);
        }
    }
}

impl WindowsDisplay {
    pub(super) unsafe fn set_custom_mouse_cursor(this: *mut Self, image: crate::CustomCursor) {
        if (*this)
            .custom_cursor
            .as_ref()
            .is_some_and(|c| c.image.same_image(&image))
        {
            return;
        }
        let cursor = match NativeCursor::new(image) {
            Ok(cursor) => cursor,
            Err(error) => {
                eprintln!("Could not create native mouse cursor: {error}");
                return;
            }
        };
        (*this).cursor = cursor.handle;
        (*this).user_cursor = true;
        // Update a stationary pointer immediately, without changing the cursor
        // over another application, a title bar, or a resize border.
        let mut point: POINT = std::mem::zeroed();
        let mut rect: RECT = std::mem::zeroed();
        if GetCursorPos(&mut point) != 0
            && WindowFromPoint(point) == (*this).wnd
            && ScreenToClient((*this).wnd, &mut point) != 0
            && GetClientRect((*this).wnd, &mut rect) != 0
            && PtInRect(&rect, point) != 0
        {
            SetCursor(cursor.handle);
        }
        (*this).custom_cursor = Some(cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_real_color_cursor_with_requested_hotspot_and_releases_it() {
        unsafe {
            let image = crate::CustomCursor::from_rgba(
                3,
                2,
                &[
                    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255,
                    0, 0, 0, 255,
                ],
                (2, 1),
            )
            .unwrap();
            let cursor = NativeCursor::new(image).unwrap();
            let mut info: ICONINFO = std::mem::zeroed();
            assert_ne!(GetIconInfo(cursor.handle, &mut info), 0);
            assert_eq!(info.fIcon, 0);
            assert_eq!((info.xHotspot, info.yHotspot), (2, 1));
            DeleteObject(info.hbmColor as _);
            DeleteObject(info.hbmMask as _);
            drop(cursor);
        }
    }
}
