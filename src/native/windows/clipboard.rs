use winapi::shared::minwindef::HGLOBAL;
use winapi::um::winbase::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use winapi::um::winuser::CF_UNICODETEXT;
use winapi::um::winuser::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};

use std::ptr;

struct ClipboardGuard;
impl ClipboardGuard {
    unsafe fn open() -> Option<Self> {
        let result = OpenClipboard(ptr::null_mut());
        if result == false as _ {
            return None;
        }
        Some(ClipboardGuard)
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

unsafe fn get_raw_clipboard() -> Option<Vec<u16>> {
    // https://docs.microsoft.com/en-us/windows/win32/dataxchg/about-the-clipboard

    let guard = ClipboardGuard::open();

    if guard.is_none() {
        eprintln!("Failed to open clipboard");
        return None;
    }

    // Returns a handle to a clipboard object
    let clipboard_data = GetClipboardData(CF_UNICODETEXT);
    if clipboard_data.is_null() {
        return None;
    }

    read_global_text(clipboard_data)
}

struct GlobalMemoryLock(HGLOBAL);

impl Drop for GlobalMemoryLock {
    fn drop(&mut self) {
        unsafe {
            GlobalUnlock(self.0);
        }
    }
}

unsafe fn read_global_text(handle: HGLOBAL) -> Option<Vec<u16>> {
    let data_ptr = GlobalLock(handle) as *const u16;
    if data_ptr.is_null() {
        return None;
    }
    let _lock = GlobalMemoryLock(handle);
    // GlobalSize returns bytes, while from_raw_parts counts u16 elements.
    // Ignore an incomplete trailing code unit in malformed clipboard data.
    let data_len = GlobalSize(handle) / std::mem::size_of::<u16>();

    let slice = std::slice::from_raw_parts(data_ptr, data_len);
    let len = slice.iter().position(|b| *b == 0).unwrap_or(data_len);
    Some(slice[..len].to_vec())
}

unsafe fn set_raw_clipboard(data: *const u8, len: usize) {
    let guard = ClipboardGuard::open();

    if guard.is_none() {
        eprintln!("Failed to open clipboard");
        return;
    }

    let alloc_handle = GlobalAlloc(GMEM_MOVEABLE, len);

    if alloc_handle.is_null() {
        eprintln!("Failed to set clipboard: memory not allocated");
        return;
    }

    let lock = GlobalLock(alloc_handle) as *mut u8;
    ptr::copy_nonoverlapping(data, lock, len);

    GlobalUnlock(lock as _);
    EmptyClipboard();

    SetClipboardData(CF_UNICODETEXT, alloc_handle);
}

pub struct WindowsClipboard {}
impl WindowsClipboard {
    pub fn new() -> WindowsClipboard {
        WindowsClipboard {}
    }
}
impl crate::native::Clipboard for WindowsClipboard {
    fn get(&mut self) -> Option<String> {
        unsafe { get_raw_clipboard().map(|data| String::from_utf16_lossy(&data)) }
    }

    fn set(&mut self, data: &str) {
        unsafe {
            let text_w = format!("{}\0", data).encode_utf16().collect::<Vec<u16>>();
            set_raw_clipboard(text_w.as_ptr() as _, text_w.len() * 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::um::winbase::{GlobalFlags, GlobalFree, GMEM_LOCKCOUNT};

    struct Allocation(HGLOBAL);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                assert!(GlobalFree(self.0).is_null());
            }
        }
    }

    #[test]
    fn unterminated_text_is_bounded_by_the_allocation_and_unlocked() {
        unsafe {
            let allocation = Allocation(GlobalAlloc(GMEM_MOVEABLE, 17));
            assert!(!allocation.0.is_null());
            let size = GlobalSize(allocation.0);
            let pointer = GlobalLock(allocation.0) as *mut u8;
            assert!(!pointer.is_null());
            ptr::write_bytes(pointer, 0x41, size);
            GlobalUnlock(allocation.0);

            assert_eq!(read_global_text(allocation.0), Some(vec![0x4141; size / 2]));
            assert_eq!(GlobalFlags(allocation.0) & GMEM_LOCKCOUNT, 0);
        }
    }

    #[test]
    fn text_ends_at_the_first_nul_and_preserves_utf16() {
        unsafe {
            let allocation = Allocation(GlobalAlloc(GMEM_MOVEABLE, 10));
            assert!(!allocation.0.is_null());
            let pointer = GlobalLock(allocation.0) as *mut u16;
            assert!(!pointer.is_null());
            let size = GlobalSize(allocation.0);
            ptr::write_bytes(pointer as *mut u8, 0x41, size);
            let text = [0x0416, 0xD83D, 0xDE00, 0, 0x42];
            ptr::copy_nonoverlapping(text.as_ptr(), pointer, text.len());
            GlobalUnlock(allocation.0);

            assert_eq!(read_global_text(allocation.0), Some(text[..3].to_vec()));
            assert_eq!(GlobalFlags(allocation.0) & GMEM_LOCKCOUNT, 0);
        }
    }
}
