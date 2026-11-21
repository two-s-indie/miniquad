use super::libwayland_client::{
    wl_buffer, wl_proxy, wl_shm, wl_shm_format_WL_SHM_FORMAT_ARGB8888, wl_shm_pool,
    LibWaylandClient, WL_SHM_CREATE_POOL, WL_SHM_POOL_CREATE_BUFFER, WL_SHM_POOL_DESTROY,
};
use crate::wl_request_constructor;

unsafe fn wl_shm_pool_destroy(libwayland: &mut LibWaylandClient, wl_shm_pool: *mut wl_shm_pool) {
    (libwayland.wl_proxy_marshal)(wl_shm_pool as _, WL_SHM_POOL_DESTROY);
    (libwayland.wl_proxy_destroy)(wl_shm_pool as _);
}

unsafe extern "C" fn create_tmpfile_cloexec(tmpname: *mut libc::c_char) -> libc::c_int {
    let fd = libc::mkostemp(tmpname, libc::O_CLOEXEC);
    if fd >= 0 {
        libc::unlink(tmpname);
    }
    fd
}

unsafe fn create_anonymous_file(size: usize) -> std::io::Result<libc::c_int> {
    let xdg_folder_path = std::env::var("XDG_RUNTIME_DIR").map_err(std::io::Error::other)?;
    let filepath = format!("{}/miniquad-shared-XXXXXX", xdg_folder_path);
    let mut c_filepath = std::ffi::CString::new(filepath)
        .map_err(std::io::Error::other)?
        .into_bytes_with_nul();
    let fd = create_tmpfile_cloexec(c_filepath.as_mut_ptr() as _);

    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let ret = libc::posix_fallocate(fd, 0, size as _);

    if ret != 0 {
        libc::close(fd);
        return Err(std::io::Error::from_raw_os_error(ret));
    }
    Ok(fd)
}

pub unsafe fn create_shm_buffer(
    libwayland: &mut LibWaylandClient,
    shm: *mut wl_shm,
    width: i32,
    height: i32,
    pixels: &[u8],
) -> *mut wl_buffer {
    try_create_shm_buffer(libwayland, shm, width, height, pixels)
        .expect("Could not create shared-memory buffer")
}

pub unsafe fn try_create_shm_buffer(
    libwayland: &mut LibWaylandClient,
    shm: *mut wl_shm,
    width: i32,
    height: i32,
    pixels: &[u8],
) -> std::io::Result<*mut wl_buffer> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid shared-memory image",
        )
    };
    let stride = width.checked_mul(4).ok_or_else(invalid)?;
    let length = stride.checked_mul(height).ok_or_else(invalid)?;
    if width <= 0 || height <= 0 || pixels.len() != length as usize {
        return Err(invalid());
    }
    let fd = create_anonymous_file(length as _)?;
    let data = libc::mmap(
        std::ptr::null_mut(),
        length as _,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
    );

    if data == libc::MAP_FAILED {
        let error = std::io::Error::last_os_error();
        libc::close(fd);
        return Err(error);
    }

    let pool: *mut wl_shm_pool = wl_request_constructor!(
        libwayland,
        shm,
        WL_SHM_CREATE_POOL,
        libwayland.wl_shm_pool_interface,
        fd,
        length
    );
    libc::close(fd);
    if pool.is_null() {
        libc::munmap(data, length as _);
        return Err(std::io::Error::other(
            "Could not create Wayland shared-memory pool",
        ));
    }

    let target = data as *mut u8;
    for i in 0..width * height * 4 {
        *target.offset(i as _) = pixels[i as usize];
    }

    let buffer: *mut wl_buffer = wl_request_constructor!(
        libwayland,
        pool,
        WL_SHM_POOL_CREATE_BUFFER,
        libwayland.wl_buffer_interface,
        0,
        width,
        height,
        stride,
        wl_shm_format_WL_SHM_FORMAT_ARGB8888
    );

    libc::munmap(data, length as _);

    wl_shm_pool_destroy(libwayland, pool);
    if buffer.is_null() {
        Err(std::io::Error::other(
            "Could not create Wayland shared-memory buffer",
        ))
    } else {
        Ok(buffer)
    }
}
