# Rust-Python Integration Documentation

## Overview

The S19 Viewer demonstrates the Rust-Python interoperability by exposing Rust functions as a C-compatible Dynamic Link Library (DLL) that Python can call using `ctypes`. This allows Python to leverage Rust's performance while maintaining ease of development.

## Architecture

```
┌─────────────────────────────────────────┐
│         Python Application              │
│  (s19viewer.py - GUI with tkinter)      │
└────────────────────┬────────────────────┘
                     │ ctypes.CDLL()
                     │ (calls C functions)
                     ▼
┌─────────────────────────────────────────┐
│     Rust DLL (s19parser.dll)            │
│  - s19_parse_file() function            │
│  - s19_free() function                  │
│  - repr(C) struct for data transfer     │
└─────────────────────────────────────────┘
```

## Rust Side: Exposing Functions as C API

### 1. Define C-Compatible Struct

The key is using `repr(C)` to ensure memory layout matches C's expectations:

```rust
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
```

**Why `repr(C)`?**
- Ensures struct fields are laid out in memory exactly as C would arrange them
- Predictable offsets for each field
- Python's ctypes can read the struct byte-by-byte

### 2. Expose C Functions with `#[no_mangle]` and `extern "C"`

```rust
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
#[no_mangle]
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
```

**Key Details:**
- `#[no_mangle]` – Prevents Rust from mangling the function name so C/Python can find it (mangling = rust auto-naming changes during build).
- `extern "C"` – Uses C calling convention instead of Rust's.
- Raw pointers – Low-level control for C interop.
- `*const c_char` – C-style null-terminated string.
- `*mut u32` – Pointer to write output count to (pass by reference).
- Returns `*mut S19Record` – Pointer to heap-allocated array.

### 3. Memory Management Function

Every allocation function needs a deallocation function:

```rust
/// Free an array previously returned by `s19_parse_file`.
///
/// # Parameters
/// - `ptr`:   the pointer returned by `s19_parse_file`
/// - `count`: the count written to `out_count`
#[no_mangle]
pub extern "C" fn s19_free(ptr: *mut S19Record, count: u32) {
    if ptr.is_null() || count == 0 {
        return;
    }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(ptr, count as usize);
        drop(Box::from_raw(slice as *mut [S19Record]));
    }
}
```

**Why it's needed:**
- Rust allocated memory on the heap (`Box::into_raw()`)
- Python cannot directly free Rust-allocated memory
- Explicit deallocation prevents memory leaks

## Python Side: Calling Rust Functions

### 1. Define ctypes Structure

Mirror the Rust struct in Python:

```python
class _S19RecordC(ctypes.Structure):
    """Mirrors the repr(C) S19Record struct from lib.rs."""
    _fields_ = [
        ("record_type", ctypes.c_uint8),   # 1=S1, 2=S2, 3=S3
        ("address",     ctypes.c_uint32),
        ("data_len",    ctypes.c_uint32),
        ("data",        ctypes.c_uint8 * 255),
    ]
```

**Mapping:**
- Rust `u8` → Python `ctypes.c_uint8`
- Rust `u32` → Python `ctypes.c_uint32`
- Rust array `[u8; 255]` → Python `ctypes.c_uint8 * 255`

### 2. Load the DLL and Configure Function Signatures

```python
def _try_load_rust_dll():
    global _rust_lib, _USING_RUST
    dll_path = _find_dll()
    if dll_path is None:
        return False
    try:
        lib = ctypes.CDLL(dll_path)
        
        # Configure s19_parse_file signature
        lib.s19_parse_file.restype  = ctypes.POINTER(_S19RecordC)
        lib.s19_parse_file.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint32)]
        
        # Configure s19_free signature
        lib.s19_free.restype  = None
        lib.s19_free.argtypes = [ctypes.POINTER(_S19RecordC), ctypes.c_uint32]
        
        _rust_lib = lib
        _USING_RUST = True
        return True
    except OSError:
        return False
```

**Configuration details:**
- `restype` – Specifies what the function returns
- `argtypes` – Specifies what types each argument should be
- `ctypes.POINTER(_S19RecordC)` – Pointer to array of structs
- `ctypes.POINTER(ctypes.c_uint32)` – Pointer to u32 for output parameter

### 3. Call the Rust Function

```python
def _parse_s19_rust(path):
    """Parse using the Rust DLL. Returns list of (rt_str, address, data_bytes)."""
    count = ctypes.c_uint32(0)
    ptr   = _rust_lib.s19_parse_file(path.encode("utf-8"), ctypes.byref(count))
    
    if not ptr or count.value == 0:
        return []
    
    try:
        results = []
        for i in range(count.value):
            rec  = ptr[i]  # Index into array
            rt   = RT_NAMES.get(rec.record_type, "S?")
            data = bytes(rec.data[:rec.data_len])
            results.append((rt, rec.address, data))
        return results
    finally:
        _rust_lib.s19_free(ptr, count)
```

**How it works:**
1. `count = ctypes.c_uint32(0)` – Create output parameter
2. `path.encode("utf-8")` – Convert Python string to C string
3. `ctypes.byref(count)` – Pass pointer to count variable
4. `ptr[i]` – Index into returned array
5. `_rust_lib.s19_free()` – Always free memory in finally block

## Data Flow Diagram

```
Python:
    path = "firmware.s19"
         ↓
    Call: s19_parse_file("firmware.s19", &count)
         ↓
Rust:
    Receives: C string path, pointer to u32
         ↓
    Read file, parse records
    Allocate Box<[S19Record]> on heap
         ↓
    Write array count to *out_count
    Return pointer to heap array
         ↓
Python:
    Receives: pointer to array, count value
         ↓
    Loop through ptr[0], ptr[1], ..., ptr[count-1]
    Extract record_type, address, data from each struct
         ↓
    Call: s19_free(ptr, count)
         ↓
Rust:
    Deallocate the heap array
```

## Why This Design?

| Aspect | Benefit |
|--------|---------|
| **repr(C)** | Python can directly read Rust struct fields from memory |
| **Raw Pointers** | Full control over memory allocation/deallocation |
| **extern "C"** | Compatible with ctypes calling convention |
| **#[no_mangle]** | Function names don't get mangled, ctypes can find them |
| **Explicit free()** | Prevents memory leaks; caller controls cleanup |

## Common Pitfalls

**Forgetting `repr(C)`** – Struct layout won't match, data corruption  
**Forgetting `#[no_mangle]`** – Function symbol won't exist in DLL  
**Wrong ctypes signature** – Data passed incorrectly  
**Not calling s19_free()** – Memory leak  
**Mismatched types** – u32 in Rust must be c_uint32 in Python  

## Cargo.toml Configuration

The library must be built as both rlib and cdylib:

```toml
[lib]
name = "s19parser"
crate-type = ["cdylib", "rlib"]
```

- `cdylib` – Builds the .dll file for C/Python interop
- `rlib` – Rust library format (for the CLI binary to use)

## Build Command

```bash
cargo build
```

Creates: `target/x86_64-pc-windows-gnu/debug/s19parser.dll`

This is the file that Python loads with `ctypes.CDLL()`.
