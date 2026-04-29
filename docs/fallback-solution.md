# Fallback Solution Documentation

## Overview

The S19 Viewer implements a **fallback mechanism** that allows the application to function with or without the Rust DLL. This ensures robustness and enables development even before the Rust component is fully compiled.

## How It Works

### 1. Initialization Phase

When `s19viewer.py` starts, it attempts to load the Rust DLL:

```python
def _try_load_rust_dll():
    global _rust_lib, _USING_RUST
    dll_path = _find_dll()
    if dll_path is None:
        return False
    try:
        lib = ctypes.CDLL(dll_path)
        # Configure function signatures
        lib.s19_parse_file.restype  = ctypes.POINTER(_S19RecordC)
        lib.s19_parse_file.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint32)]
        lib.s19_free.restype  = None
        lib.s19_free.argtypes = [ctypes.POINTER(_S19RecordC), ctypes.c_uint32]
        _rust_lib = lib
        _USING_RUST = True
        return True
    except OSError:
        return False

_try_load_rust_dll()
```

**What happens:**
- Searches for `s19parser.dll` in multiple locations
- If found, loads it and configures the C function signatures
- If not found or loading fails, **silently** continues with `_USING_RUST = False`

### 2. DLL Search Locations

The `_find_dll()` function searches in this order:

```python
def _find_dll():
    """Search for s19parser.dll next to this script or in the Rust target dirs."""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.path.join(script_dir, "s19parser.dll"),
        os.path.join(script_dir, "target", "x86_64-pc-windows-gnu", "debug",   "s19parser.dll"),
        os.path.join(script_dir, "target", "x86_64-pc-windows-gnu", "release", "s19parser.dll"),
        os.path.join(script_dir, "target", "debug",   "s19parser.dll"),
        os.path.join(script_dir, "target", "release", "s19parser.dll"),
    ]
    for p in candidates:
        if os.path.isfile(p):
            return p
    return None
```

**Search paths:**
- Script directory (if DLL is copied there)
- Rust GNU target debug folder
- Rust GNU target release folder
- Standard debug folder
- Standard release folder

### 3. Smart Parser Selection

When parsing a file, the application automatically selects the appropriate parser:

```python
def parse_s19(path):
    """Parse an S19 file. Uses Rust DLL if available, pure Python otherwise."""
    if _USING_RUST:
        return _parse_s19_rust(path)
    return _parse_s19_python(path)
```

### 4. User Feedback

The application displays which parser is being used:

```python
PARSER_LABEL = "Rust DLL" if _USING_RUST else "Python (DLL not found)"
```

This label is shown in the toolbar, making it transparent to users.

## The Two Implementations

### Rust Parser (`_parse_s19_rust()`)

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
            rec  = ptr[i]
            rt   = RT_NAMES.get(rec.record_type, "S?")
            data = bytes(rec.data[:rec.data_len])
            results.append((rt, rec.address, data))
        return results
    finally:
        _rust_lib.s19_free(ptr, count)
```

**Advantages:**
- **Fast**: Compiled C code, typically 10-100x faster than Python
- **Efficient**: Optimized machine code


**Disadvantages:**
- **Memory-Safe-Bypass**: The implementation via ctypes does bypass Rust memory-safe-mechanisms. 

### Python Fallback (`_parse_s19_python()`)

```python
def _parse_s19_python(path):
    """Pure-Python S19 parser (fallback when DLL is unavailable)."""
    records = []
    with open(path, "r", errors="replace") as f:
        for line in f:
            line = line.strip()
            if len(line) < 4:
                continue
            rt = line[:2]
            if rt not in ("S1", "S2", "S3"):
                continue
            try:
                bc   = int(line[2:4], 16)
                body = bytes.fromhex(line[4:4 + bc * 2])
            except ValueError:
                continue
            aw   = {"S1": 2, "S2": 3, "S3": 4}[rt]
            if len(body) < aw + 1:
                continue
            addr = int.from_bytes(body[:aw], "big")
            data = body[aw:-1]
            records.append((rt, addr, data))
    return records
```

**Advantages:**
- **No compilation needed**: Works immediately
- **Debugging**: Easier to understand and modify
- **Universal**: Runs on any Python installation

## Benefits of This Approach

**Robustness** – Application never completely fails  
**Development-Friendly** – Test GUI without Rust compilation  
**Transparent** – Users know which parser is active  
**Performance** – Gets speed benefits when available  
**Flexibility** – Works in both scenarios seamlessly  

## Typical Workflows

### Scenario 1: Rust DLL Not Compiled Yet
1. Developer clones repo
2. Runs `python s19viewer.py test.s19`
3. Python fallback parser activates
4. GUI works immediately ✓
5. Status shows "Python (DLL not found)"

### Scenario 2: After `cargo build`
1. Rust DLL is built and placed in `target/` folder
2. User runs `python s19viewer.py test.s19`
3. DLL is found and loaded
4. Rust parser is used (faster) ✓
5. Status shows "Rust DLL"

## Key Code Structure

| Component | Location | Purpose |
|-----------|----------|---------|
| DLL Loading | `_try_load_rust_dll()` | Attempts to load Rust DLL |
| DLL Search | `_find_dll()` | Locates DLL in filesystem |
| C Bindings | `_S19RecordC` | Maps Rust struct to Python |
| Smart Router | `parse_s19()` | Routes to correct parser |
| Status Label | `PARSER_LABEL` | Shows which parser is active |

This fallback mechanism is the backbone of the S19 Viewer's reliability and developer experience.
