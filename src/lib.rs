/// S19 parser exposed as a C-compatible DLL.
///
/// Python usage (ctypes):
///   lib = ctypes.CDLL("s19parser.dll")
///   count = ctypes.c_uint32(0)
///   ptr   = lib.s19_parse_file(b"firmware.s19", ctypes.byref(count))
///   # iterate count.value records via S19Record array at ptr
///   lib.s19_free(ptr, count)

use std::ffi::CStr;
use std::os::raw::c_char;

/// One parsed S19 record returned to the caller.
/// Layout is repr(C) so Python ctypes can map it directly.
#[repr(C)]
pub struct S19Record {
    /// Record type: 1 = S1, 2 = S2, 3 = S3
    pub record_type: u8,
    /// Address (always 32-bit, zero-extended for S1/S2)
    pub address: u32,
    /// Number of valid bytes in `data`
    pub data_len: u32,
    /// Up to 255 data bytes (max one S-record payload)
    pub data: [u8; 255],
}

// ── Internal parser (also pub for use by the bin crate) ──────────────────────

pub fn parse_s19_internal(content: &str) -> Vec<S19Record> {
    let mut records = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.len() < 4 {
            continue;
        }
        let rt = &line[0..2];
        let (record_type, addr_bytes): (u8, usize) = match rt {
            "S1" => (1, 2),
            "S2" => (2, 3),
            "S3" => (3, 4),
            _ => continue,
        };
        let byte_count = match u8::from_str_radix(&line[2..4], 16) {
            Ok(v) => v as usize,
            Err(_) => continue,
        };
        let hex_body = &line[4..];
        if hex_body.len() < byte_count * 2 {
            continue;
        }
        let mut bytes: Vec<u8> = Vec::with_capacity(byte_count);
        let mut ok = true;
        for i in 0..byte_count {
            match u8::from_str_radix(&hex_body[i * 2..i * 2 + 2], 16) {
                Ok(b) => bytes.push(b),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || bytes.len() < addr_bytes + 1 {
            continue;
        }
        let mut address: u32 = 0;
        for i in 0..addr_bytes {
            address = (address << 8) | bytes[i] as u32;
        }
        // data = bytes after address, minus checksum byte at end
        let data_slice = &bytes[addr_bytes..bytes.len() - 1];
        let data_len = data_slice.len().min(255) as u32;
        let mut data = [0u8; 255];
        data[..data_len as usize].copy_from_slice(&data_slice[..data_len as usize]);
        records.push(S19Record { record_type, address, data_len, data });
    }
    records
}

// ── Public C API ──────────────────────────────────────────────────────────────

/// Parse an S19 file and return a heap-allocated array of `S19Record`.
///
/// # Parameters
/// - `path_ptr`: null-terminated UTF-8 file path
/// - `out_count`: receives the number of records written
///
/// # Returns
/// Pointer to a `Box<[S19Record]>` (heap-allocated slice).
/// Returns NULL on error or if no records were found.
/// **Must** be freed with `s19_free()`.
#[unsafe(no_mangle)]
pub extern "C" fn s19_parse_file(
    path_ptr: *const c_char,
    out_count: *mut u32,
) -> *mut S19Record {
    // Safety: caller must pass a valid null-terminated string.
    let path = unsafe {
        if path_ptr.is_null() {
            return std::ptr::null_mut();
        }
        match CStr::from_ptr(path_ptr).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return std::ptr::null_mut(),
        }
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    let records = parse_s19_internal(&content);
    if records.is_empty() {
        unsafe { *out_count = 0; }
        return std::ptr::null_mut();
    }

    let count = records.len() as u32;
    unsafe { *out_count = count; }

    // Leak the Vec into a raw pointer that the caller owns.
    let boxed = records.into_boxed_slice();
    Box::into_raw(boxed) as *mut S19Record
}

/// Free an array previously returned by `s19_parse_file`.
///
/// # Parameters
/// - `ptr`:   the pointer returned by `s19_parse_file`
/// - `count`: the count written to `out_count`
#[unsafe(no_mangle)]
pub extern "C" fn s19_free(ptr: *mut S19Record, count: u32) {
    if ptr.is_null() || count == 0 {
        return;
    }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(ptr, count as usize);
        drop(Box::from_raw(slice as *mut [S19Record]));
    }
}
