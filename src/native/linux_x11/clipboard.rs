#![allow(non_upper_case_globals, non_snake_case)]

//! Clipboard implementation for X11
//!
//! Clipboard API on X11 is pretty weird
//! <https://www.uninformativ.de/blog/postings/2017-04-02/0/POSTING-en.html>
//! so use this with caution.

use super::libx11::*;
use std::convert::TryFrom;

fn property_byte_len(format: libc::c_int, item_count: libc::c_ulong) -> Option<usize> {
    // Xlib stores format-32 values as native longs, even on 64-bit systems.
    let element_size = match format {
        8 => std::mem::size_of::<libc::c_char>(),
        16 => std::mem::size_of::<libc::c_short>(),
        32 => std::mem::size_of::<libc::c_long>(),
        _ => return None,
    };
    usize::try_from(item_count)
        .ok()?
        .checked_mul(element_size)
        .filter(|&len| len <= isize::MAX as usize)
}

#[cfg(test)]
mod property_buffer_tests {
    use super::property_byte_len;

    #[test]
    fn format_16_stops_at_the_reported_item_count() {
        // Trailing values make a doubled byte count fail without an invalid read.
        let values = [0x1234u16, 0xabcd, 0xdead, 0xbeef];
        let len = property_byte_len(16, 2).unwrap();
        let bytes = unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), len) };
        let expected: Vec<u8> = values[..2].iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn formats_use_xlib_element_sizes() {
        assert_eq!(property_byte_len(8, 3), Some(3));
        assert_eq!(property_byte_len(16, 3), Some(6));
        assert_eq!(
            property_byte_len(32, 3),
            Some(3 * std::mem::size_of::<libc::c_long>())
        );
        for format in [8, 16, 32] {
            assert_eq!(property_byte_len(format, 0), Some(0));
        }
    }

    #[test]
    fn rejects_invalid_formats_and_oversized_lengths() {
        assert_eq!(property_byte_len(0, 1), None);
        assert_eq!(property_byte_len(24, 1), None);
        assert_eq!(property_byte_len(16, libc::c_ulong::MAX), None);
        assert_eq!(property_byte_len(32, libc::c_ulong::MAX), None);
        let too_many_shorts = (isize::MAX as libc::c_ulong / 2) + 1;
        assert_eq!(property_byte_len(16, too_many_shorts), None);
    }
}

unsafe fn get_clipboard(
    libx11: &mut LibX11,
    display: *mut Display,
    window: Window,
    format: Atom,
) -> Option<Vec<u8>> {
    // We ask X server to ask current clipboard owner to convert selection to the desired format
    // Selection owner is other X11 application and if it will not satisfy our request for any reason -
    // we will wait for appropriate event forever
    // but OK lets believe that all other linux apps will gracefully send us nice utf-8 strings
    (libx11.XConvertSelection)(
        display,
        libx11.extensions.clipboard,
        format,
        libx11.extensions.xsel_data,
        window,
        CurrentTime as Time,
    );

    // And now X server will respond with clipboard data
    // But we want "get_clipboard" to be blocking function and get result asap
    // So we just start to wait for the event right here
    // In case that our app already is clipboard owner - we need to handle SelectionNotify for data response
    // and SelectionRequest - to handle this request we just did couple of lines above
    let mut event = XEvent { type_0: 0 };
    loop {
        (libx11.XNextEvent)(display, &mut event);
        if event.type_0 == SelectionNotify
            && event.xselection.selection == libx11.extensions.clipboard
        {
            // The property should be XSEL_DATA, otherwise we return None
            return (event.xselection.property == libx11.extensions.xsel_data)
                .then(|| get_property_bytes(libx11, display, window, event.xselection.property));
        }
        if event.type_0 == SelectionRequest {
            respond_to_clipboard_request(libx11, display, &mut event as *mut _);
        }
    }
}

/// Get the bytes from the window property
/// INCR protocol is implemented; will block until all data are received
/// Useful for getting data following a SelectionNotify event
//
//  X11 has the INCR protocol to transfer data in several batches
//  https://www.x.org/releases/X11R7.7/doc/xorg-docs/icccm/icccm.html#INCR_Properties
//
//  The data are sequentially written to the given property on the window
//  After each read, we delete the property to notify X that we are ready to receive more data
//  Used both for the clipboard and the drag-n-drop protocol
pub(crate) unsafe fn get_property_bytes(
    libx11: &mut LibX11,
    display: *mut Display,
    window: Window,
    property: Atom,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut incr_mode = false;
    loop {
        let mut buf = std::ptr::null_mut::<libc::c_char>();
        let mut size: libc::c_ulong = 0;
        let mut actual_type = 0 as Atom;
        let mut actual_format: libc::c_int = 0;
        let mut _size_after: libc::c_ulong = 0;
        let status = (libx11.XGetWindowProperty)(
            display,
            window,
            property,
            0 as _,
            libc::c_long::MAX,
            false as _,
            AnyPropertyType,
            &mut actual_type,
            &mut actual_format,
            &mut size,
            &mut _size_after,
            &mut buf as *mut *mut libc::c_char as _,
        );

        if status != 0 {
            (libx11.XFree)(buf as *mut libc::c_void);
            return Vec::new();
        }

        if actual_type == libx11.extensions.incr {
            // we are in INCR mode
            incr_mode = true;
        } else if size == 0 {
            // no more data to read
            (libx11.XFree)(buf as *mut libc::c_void);
            return bytes;
        } else {
            let byte_len = match property_byte_len(actual_format, size) {
                Some(len) if !buf.is_null() => len,
                _ => {
                    (libx11.XFree)(buf as *mut libc::c_void);
                    return Vec::new();
                }
            };
            bytes.extend_from_slice(std::slice::from_raw_parts(buf as *const _, byte_len));
        }

        // Release each property allocation, including intermediate INCR chunks.
        (libx11.XFree)(buf as *mut libc::c_void);
        if !incr_mode {
            return bytes;
        }

        (libx11.XDeleteProperty)(display, window, property);
        // wait until we get a new batch
        let mut event = XEvent { type_0: 0 };
        loop {
            (libx11.XNextEvent)(display, &mut event);
            if event.type_0 == PropertyNotify && event.xproperty.state == PropertyNewValue {
                break;
            }
        }
    }
}

// Next message for clipboard request
static mut MESSAGE: Option<String> = None;

/// Claim that our app is X11 clipboard owner
/// Now when some other linux app will ask X11 for clipboard content - it will be redirected to our app
unsafe fn claim_clipboard_ownership(
    libx11: &mut LibX11,
    display: *mut Display,
    window: Window,
    message: String,
) {
    (libx11.XSetSelectionOwner)(
        display,
        libx11.extensions.clipboard,
        window,
        CurrentTime as Time,
    );

    MESSAGE = Some(message);
}

/// this function is supposed to be called from sapp's event loop
/// when XSelectionEvent received.
/// It will parse event and call XSendEvent with event response
pub(crate) unsafe fn respond_to_clipboard_request(
    libx11: &mut LibX11,
    display: *mut Display,
    event: *const XEvent,
) {
    assert!((*event).type_0 == SelectionRequest); // is it really SelectionRequest

    let empty_message = String::new();
    let message = MESSAGE.as_ref().unwrap_or(&empty_message);

    let utf8_string = libx11.extensions.utf8_string;
    let xselectionrequest = (*event).xselectionrequest;
    let mut ev = XSelectionEvent {
        type_0: SelectionNotify,
        serial: 0,
        send_event: 0,
        display: xselectionrequest.display,
        requestor: xselectionrequest.requestor,
        selection: xselectionrequest.selection,
        target: xselectionrequest.target,
        property: xselectionrequest.property,
        time: xselectionrequest.time,
    };

    // only UTF8 requests are supported
    if xselectionrequest.target == utf8_string {
        (libx11.XChangeProperty)(
            xselectionrequest.display,
            xselectionrequest.requestor,
            xselectionrequest.property,
            utf8_string,
            8 as libc::c_int,
            PropModeReplace,
            message.as_bytes().as_ptr(),
            message.len() as _,
        );

        (libx11.XSendEvent)(
            display,
            ev.requestor,
            false as _,
            NoEventMask,
            &mut ev as *mut XSelectionEvent as *mut XEvent,
        );
    } else {
        // signal X that request is denied
        ev.property = 0 as Atom;

        (libx11.XSendEvent)(
            display,
            ev.requestor,
            false as _,
            NoEventMask,
            &mut ev as *mut XSelectionEvent as *mut XEvent,
        );
    }
}

pub struct X11Clipboard {
    libx11: LibX11,
    display: *mut Display,
    window: Window,
}
unsafe impl Send for X11Clipboard {}
unsafe impl Sync for X11Clipboard {}

impl X11Clipboard {
    pub fn new(libx11: LibX11, display: *mut Display, window: Window) -> X11Clipboard {
        X11Clipboard {
            libx11,
            display,
            window,
        }
    }
}
impl crate::native::Clipboard for X11Clipboard {
    fn get(&mut self) -> Option<String> {
        let utf8_string = self.libx11.extensions.utf8_string;
        let bytes =
            unsafe { get_clipboard(&mut self.libx11, self.display, self.window, utf8_string)? };
        Some(std::str::from_utf8(&bytes).ok()?.to_string())
    }

    fn set(&mut self, data: &str) {
        unsafe {
            claim_clipboard_ownership(&mut self.libx11, self.display, self.window, data.to_owned());
        };
    }
}
