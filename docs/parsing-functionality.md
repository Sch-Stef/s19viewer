# S19 File Parsing Documentation

## Overview

This document explains the S19 (Motorola S-record) file format and how the S19 Viewer parses these files in both Rust and Python implementations.

## S19 Format Overview

S-records are a text-based format for representing executable code, often used in embedded systems and firmware updates. Each line represents one record.

### S-Record Structure

```
S1 10 CAFE 0123456789ABCDEF FF
│  │  │    │                 │
│  │  │    │                 └─ Checksum byte
│  │  │    └─ Data bytes (payload + checksum)
│  │  └────── 16-bit address (CAFE)
│  └───────── Byte count (10 hex = 16 decimal)
└──────────── Record type (S1, S2, or S3)
```

### Record Types

| Type | Address Size | Purpose |
|------|--------------|---------|
| **S1** | 16-bit (2 bytes) | Code/data record with 16-bit address |
| **S2** | 24-bit (3 bytes) | Code/data record with 24-bit address |
| **S3** | 32-bit (4 bytes) | Code/data record with 32-bit address |
| S0 | - | Header record (ignored) |
| S7/S8/S9 | - | End record (ignored) |

### Byte Count Field

The byte count field (after the record type) specifies how many bytes follow, including the address and checksum:

- **S1**: byte_count includes 2 bytes address + N bytes data + 1 byte checksum
- **S2**: byte_count includes 3 bytes address + N bytes data + 1 byte checksum
- **S3**: byte_count includes 4 bytes address + N bytes data + 1 byte checksum

### Example

```
S1 10 CAFE 48656C6C 6F205765 726C6421 C8
```

Breaking it down:
- `S1` – Record type (16-bit address)
- `10` – Byte count: 16 bytes (hex) = 22 bytes total in body
- `CAFE` – Address field (16-bit)
- `48656C6C6F205765726C6421` – Data (12 bytes = "Hello World!")
- `C8` – Checksum

## Rust Implementation

The Rust parser is in [src/lib.rs](../src/lib.rs) with the `parse_s19_internal()` function.

### Step-by-Step Parsing

```rust
pub fn parse_s19_internal(content: &str) -> Vec<S19Record> {
    let mut records = Vec::new();
    
    // 1. Process each line
    for line in content.lines() {
        let line = line.trim();
        if line.len() < 4 {
            continue;
        }
        
        // 2. Extract and validate record type
        let rt = &line[0..2];
        let (record_type, addr_bytes): (u8, usize) = match rt {
            "S1" => (1, 2),
            "S2" => (2, 3),
            "S3" => (3, 4),
            _ => continue,
        };
        
        // 3. Parse byte count
        let byte_count = match u8::from_str_radix(&line[2..4], 16) {
            Ok(v) => v as usize,
            Err(_) => continue,
        };
        
        // 4. Extract hex body
        let hex_body = &line[4..];
        if hex_body.len() < byte_count * 2 {
            continue;
        }
        
        // 5. Convert hex string to bytes
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
        
        // 6. Extract address (big-endian)
        let mut address: u32 = 0;
        for i in 0..addr_bytes {
            address = (address << 8) | bytes[i] as u32;
        }
        
        // 7. Extract data (skip address bytes and checksum byte at end)
        let data_slice = &bytes[addr_bytes..bytes.len() - 1];
        let data_len = data_slice.len().min(255) as u32;
        let mut data = [0u8; 255];
        data[..data_len as usize].copy_from_slice(&data_slice[..data_len as usize]);
        
        // 8. Create and store record
        records.push(S19Record { record_type, address, data_len, data });
    }
    records
}
```

### Parsing Steps Explained

**Step 1: Process Lines**
- Read file line by line
- Skip empty/short lines

**Step 2: Extract Record Type**
- Read first 2 characters (S1, S2, or S3)
- Match to get record type ID and address width
- Skip unknown types

**Step 3: Parse Byte Count**
- Characters at positions [2:4] are hexadecimal
- Convert to decimal: `u8::from_str_radix("10", 16)` = 16

**Step 4: Extract Hex Body**
- Everything after byte count is hex data
- Verify enough characters exist

**Step 5: Convert Hex to Bytes**
- Process every 2 hex characters
- `from_str_radix("48", 16)` = 0x48 = 72 decimal
- Accumulate into bytes vector

**Step 6: Extract Address**
- Read first N bytes (2, 3, or 4 depending on type)
- Big-endian: first byte is most significant
- `address = (address << 8) | next_byte`

**Step 7: Extract Data**
- Skip address bytes at start
- Skip checksum byte at end
- Everything in between is data

**Step 8: Store Record**
- Create S19Record struct
- Add to results vector

### Example Trace

Input line: `S1 10 CAFE 48656C6C 6F205765 726C6421 C8`

| Step | Value | Notes |
|------|-------|-------|
| Record type | S1 → type=1, addr_bytes=2 | |
| Byte count | 10 → 16 decimal | |
| Hex body | 48656C6C... | 32 hex chars = 16 bytes |
| Bytes | [CA, FE, 48, 65, 6C, 6C, 6F, 20, 57, 65, 72, 6C, 64, 21, C8] | Converted from hex |
| Address | 0xCAFE | First 2 bytes, big-endian |
| Data | [48, 65, 6C, 6C, 6F, 20, 57, 65, 72, 6C, 64, 21] | 12 data bytes |
| Checksum | 0xC8 | Last byte (not stored in data) |

## Python Implementation

The Python fallback parser is in [s19viewer.py](../s19viewer.py) with the `_parse_s19_python()` function.

```python
def _parse_s19_python(path):
    """Pure-Python S19 parser (fallback when DLL is unavailable)."""
    records = []
    with open(path, "r", errors="replace") as f:
        for line in f:
            line = line.strip()
            if len(line) < 4:
                continue
            
            # 1. Extract record type
            rt = line[:2]
            if rt not in ("S1", "S2", "S3"):
                continue
            
            try:
                # 2. Parse byte count and extract body
                bc   = int(line[2:4], 16)
                body = bytes.fromhex(line[4:4 + bc * 2])
            except ValueError:
                continue
            
            # 3. Get address width based on type
            aw   = {"S1": 2, "S2": 3, "S3": 4}[rt]
            if len(body) < aw + 1:
                continue
            
            # 4. Extract address (big-endian)
            addr = int.from_bytes(body[:aw], "big")
            
            # 5. Extract data (everything except address and checksum)
            data = body[aw:-1]
            
            records.append((rt, addr, data))
    return records
```

### Rust vs Python Comparison

| Aspect | Rust | Python |
|--------|------|--------|
| **Speed** | ~0.1ms for 1000 records | ~1-5ms for 1000 records |
| **Parsing** | Manual bit shifting | Built-in `int.from_bytes()` |
| **Error Handling** | Result types (`Ok/Err`) | Try/except blocks |
| **Code Length** | ~80 lines | ~30 lines |

## Data Structure Returned

Both parsers return the same logical data structure:

### Rust Format (returned to Python)
```rust
S19Record {
    record_type: u8,    // 1, 2, or 3
    address: u32,       // 0x0000 to 0xFFFFFFFF
    data_len: u32,      // 0 to 255
    data: [u8; 255],    // First data_len bytes are valid
}
```

### Python Format
```python
(rt_string, address, data_bytes)
# Example: ("S1", 0xCAFE, b"Hello World!")
```

## File Format Examples

### Minimal S19 File
```
S009000000FCAA
S1083000F0C0E0C1B4
S5030000FB
S9030000FC
```

### With Multiple Records
```
S00F000068656C6C6F202020202020108A
S1134800000123456789ABCDEF0123456789ABCDEF0123A8
S1138000FEDCBA9876543210FEDCBA9876543210FEDCBA8B
S5030002FA
S9030000FC
```

## Common Issues and Solutions

### Invalid Hex Characters
```
S113800000ZZ   // ZZ is not hex
```
**Solution:** Parser skips lines with invalid hex

### Incorrect Byte Count
```
S1 20 8000 AAAA   // Says 20 bytes, but only 2 bytes of data
```
**Solution:** Parser checks if enough hex characters exist

### Checksum Errors
```
S1 10 8000 1234567890ABCDEF FF   // Wrong checksum
```
**Note:** Viewer doesn't validate checksums (for flexibility), but Rust ensures structural integrity

## Validation Performed

The parser validates:
- Record type is S1, S2, or S3  
- Byte count field is valid hex  
- Enough hex characters follow for byte count  
- Each hex pair converts to a valid byte  
- Address bytes exist  
- At least checksum byte exists  

## Display in GUI

Parsed records are formatted in the GUI table:

```
Type | Address   | Hex Data              | ASCII
─────┼───────────┼──────────────────────┼─────────
S1   | 0x0800    | 48 65 6C 6C 6F 20    | Hello 
     | 0x0806    | 57 6F 72 6C 64 21    | World!
S2   | 0x1000    | AA BB CC DD EE       | ....ž
```

Each 16-byte chunk of data becomes a separate row for easy viewing.
