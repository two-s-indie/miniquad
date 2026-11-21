use super::*;

pub(super) struct NativeCursor {
    pub image: crate::CustomCursor,
    surface: *mut wl_surface,
    buffer: *mut wl_buffer,
}

impl NativeCursor {
    pub unsafe fn new(
        display: &mut WaylandPayload,
        image: crate::CustomCursor,
    ) -> Result<Self, String> {
        // We must be able to restore the themed system cursor afterwards.
        if display.pointer_context.cursor_shape_device.is_null() {
            return Err("Custom cursors require the Wayland cursor-shape protocol".to_owned());
        }
        if display.shm.is_null() || display.compositor.is_null() {
            return Err("Wayland compositor has no shared-memory cursor support".to_owned());
        }
        let data = &image.0;
        let pixels: Vec<u8> = data
            .premultiplied_argb()
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let buffer = shm::try_create_shm_buffer(
            &mut display.client,
            display.shm,
            data.width as _,
            data.height as _,
            &pixels,
        )
        .map_err(|e| e.to_string())?;
        let surface: *mut wl_surface = wl_request_constructor!(
            display.client,
            display.compositor,
            WL_COMPOSITOR_CREATE_SURFACE,
            display.client.wl_surface_interface
        );
        if surface.is_null() {
            destroy_proxy(&mut display.client, buffer as _, 0);
            return Err("Could not create Wayland cursor surface".to_owned());
        }
        Ok(Self {
            image,
            surface,
            buffer,
        })
    }

    pub unsafe fn apply(
        &self,
        client: &mut LibWaylandClient,
        pointer: *mut wl_pointer,
        serial: u32,
    ) {
        wl_request!(
            client,
            pointer,
            WL_POINTER_SET_CURSOR,
            serial,
            self.surface,
            self.image.0.hotspot.0 as i32,
            self.image.0.hotspot.1 as i32
        );
        wl_request!(client, self.surface, WL_SURFACE_ATTACH, self.buffer, 0, 0);
        wl_request!(
            client,
            self.surface,
            WL_SURFACE_DAMAGE,
            0,
            0,
            self.image.0.width as i32,
            self.image.0.height as i32
        );
        wl_request!(client, self.surface, WL_SURFACE_COMMIT);
    }

    pub unsafe fn destroy(self, client: &mut LibWaylandClient) {
        // Never reuse the shared storage. Destroy the surface before its buffer;
        // the server retains the backing pool for any in-flight compositor work.
        destroy_proxy(client, self.surface as _, WL_SURFACE_DESTROY);
        destroy_proxy(client, self.buffer as _, 0); // wl_buffer.destroy
    }
}

unsafe fn destroy_proxy(client: &mut LibWaylandClient, proxy: *mut wl_proxy, opcode: u32) {
    (client.wl_proxy_marshal)(proxy, opcode);
    (client.wl_proxy_destroy)(proxy);
}
