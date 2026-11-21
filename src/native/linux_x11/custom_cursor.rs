use super::*;

#[repr(C)]
struct XcursorImage {
    version: u32,
    size: u32,
    width: u32,
    height: u32,
    xhot: u32,
    yhot: u32,
    delay: u32,
    pixels: *mut u32,
}

pub(super) struct NativeCursor {
    pub image: crate::CustomCursor,
    pub handle: libx11::Cursor,
}

impl NativeCursor {
    pub unsafe fn new(display: *mut Display, image: crate::CustomCursor) -> Result<Self, String> {
        let library = module::Module::load("libXcursor.so.1")
            .or_else(|_| module::Module::load("libXcursor.so"))
            .map_err(|e| e.to_string())?;
        let create: unsafe extern "C" fn(i32, i32) -> *mut XcursorImage = library
            .get_symbol("XcursorImageCreate")
            .map_err(|e| e.to_string())?;
        let destroy: unsafe extern "C" fn(*mut XcursorImage) = library
            .get_symbol("XcursorImageDestroy")
            .map_err(|e| e.to_string())?;
        let load: unsafe extern "C" fn(*mut Display, *const XcursorImage) -> libx11::Cursor =
            library
                .get_symbol("XcursorImageLoadCursor")
                .map_err(|e| e.to_string())?;
        let data = &image.0;
        let native = create(data.width as _, data.height as _);
        if native.is_null() {
            return Err("XcursorImageCreate failed".to_owned());
        }
        (*native).xhot = data.hotspot.0;
        (*native).yhot = data.hotspot.1;
        let pixels = data.premultiplied_argb();
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), (*native).pixels, pixels.len());
        let handle = load(display, native);
        destroy(native);
        if handle == 0 {
            Err("XcursorImageLoadCursor failed".to_owned())
        } else {
            Ok(Self { image, handle })
        }
    }
}

impl X11Display {
    pub(super) unsafe fn release_custom_cursor(&mut self) {
        if let Some(cursor) = self.custom_cursor.take() {
            (self.libx11.XFreeCursor)(self.display, cursor.handle);
        }
    }

    pub(super) unsafe fn set_custom_mouse_cursor(&mut self, image: crate::CustomCursor) {
        if self
            .custom_cursor
            .as_ref()
            .is_some_and(|c| c.image.same_image(&image))
        {
            return;
        }
        match NativeCursor::new(self.display, image) {
            Ok(cursor) => {
                self.release_custom_cursor();
                self.custom_cursor = Some(cursor);
                self.set_cursor(self.window, self.cursor_visible.then_some(self.cursor_icon));
                (self.libx11.XFlush)(self.display);
            }
            Err(error) => eprintln!("Could not create native mouse cursor: {error}"),
        }
    }
}
