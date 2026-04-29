// S19 Viewer TUI — uses the s19parser lib for parsing.
// Build: cargo build  (requires GNU target + mingw gcc, no MSVC needed)
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use s19parser::{parse_s19_internal as parse_s19_lib, S19Record as LibRecord};
use std::{env, io::{self, Write}};

// Windows types & constants
type HANDLE = *mut core::ffi::c_void;
type BOOL   = i32;
type DWORD  = u32;
type WORD   = u16;
type SHORT  = i16;
type WCHAR  = u16;

const STD_INPUT_HANDLE:  DWORD = 0xFFFFFFF6;
const STD_OUTPUT_HANDLE: DWORD = 0xFFFFFFF5;
const ENABLE_PROCESSED_INPUT:             DWORD = 0x0001;
const ENABLE_LINE_INPUT:                  DWORD = 0x0002;
const ENABLE_ECHO_INPUT:                  DWORD = 0x0004;
const ENABLE_VIRTUAL_TERMINAL_INPUT:      DWORD = 0x0200;
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: DWORD = 0x0004;
const ENABLE_PROCESSED_OUTPUT:            DWORD = 0x0001;
const KEY_EVENT: WORD = 0x0001;

// Virtual key codes
const VK_PRIOR:  WORD = 0x21;
const VK_NEXT:   WORD = 0x22;
const VK_END:    WORD = 0x23;
const VK_HOME:   WORD = 0x24;
const VK_UP:     WORD = 0x26;
const VK_DOWN:   WORD = 0x28;

#[repr(C)] #[derive(Copy, Clone, Default)]
struct COORD { X: SHORT, Y: SHORT }
#[repr(C)] #[derive(Copy, Clone, Default)]
struct SMALL_RECT { Left: SHORT, Top: SHORT, Right: SHORT, Bottom: SHORT }
#[repr(C)] #[derive(Copy, Clone, Default)]
struct CONSOLE_SCREEN_BUFFER_INFO {
    dwSize: COORD, dwCursorPosition: COORD,
    wAttributes: WORD, srWindow: SMALL_RECT, dwMaximumWindowSize: COORD,
}
#[repr(C)] #[derive(Copy, Clone)]
struct KEY_EVENT_RECORD {
    bKeyDown: BOOL, wRepeatCount: WORD,
    wVirtualKeyCode: WORD, wVirtualScanCode: WORD,
    uChar: WCHAR, dwControlKeyState: DWORD,
}
#[repr(C)] union EVENT_UNION { key: KEY_EVENT_RECORD, _pad: [u8; 20] }
#[repr(C)] struct INPUT_RECORD { EventType: WORD, _pad: WORD, Event: EVENT_UNION }

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(nStdHandle: DWORD) -> HANDLE;
    fn GetConsoleMode(hConsoleHandle: HANDLE, lpMode: *mut DWORD) -> BOOL;
    fn SetConsoleMode(hConsoleHandle: HANDLE, dwMode: DWORD) -> BOOL;
    fn ReadConsoleInputW(hConsoleInput: HANDLE, lpBuffer: *mut INPUT_RECORD,
        nLength: DWORD, lpNumberOfEventsRead: *mut DWORD) -> BOOL;
    fn GetConsoleScreenBufferInfo(hConsoleOutput: HANDLE,
        lpConsoleScreenBufferInfo: *mut CONSOLE_SCREEN_BUFFER_INFO) -> BOOL;
}

struct Console { hin: HANDLE, hout: HANDLE, orig_in: DWORD, orig_out: DWORD }

impl Console {
    fn new() -> Self {
        unsafe {
            let hin  = GetStdHandle(STD_INPUT_HANDLE);
            let hout = GetStdHandle(STD_OUTPUT_HANDLE);
            let (mut orig_in, mut orig_out) = (0u32, 0u32);
            GetConsoleMode(hin,  &mut orig_in);
            GetConsoleMode(hout, &mut orig_out);
            let new_in = (orig_in & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT))
                         | ENABLE_VIRTUAL_TERMINAL_INPUT;
            SetConsoleMode(hin, new_in);
            SetConsoleMode(hout, orig_out | ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT);
            Console { hin, hout, orig_in, orig_out }
        }
    }
    fn size(&self) -> (usize, usize) {
        unsafe {
            let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
            if GetConsoleScreenBufferInfo(self.hout, &mut info) != 0 {
                ((info.srWindow.Right - info.srWindow.Left + 1).max(40) as usize,
                 (info.srWindow.Bottom - info.srWindow.Top + 1).max(5)  as usize)
            } else { (120, 30) }
        }
    }
    fn read_key(&self) -> Option<Key> {
        unsafe {
            loop {
                let mut rec = INPUT_RECORD { EventType: 0, _pad: 0, Event: EVENT_UNION { _pad: [0u8; 20] } };
                let mut n: DWORD = 0;
                ReadConsoleInputW(self.hin, &mut rec, 1, &mut n);
                if n == 0 { continue; }
                if rec.EventType != KEY_EVENT { continue; }
                let k = rec.Event.key;
                if k.bKeyDown == 0 { continue; }
                return Some(Key { vk: k.wVirtualKeyCode, ch: k.uChar });
            }
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        print!("\x1b[?25h\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
        unsafe { SetConsoleMode(self.hin, self.orig_in); SetConsoleMode(self.hout, self.orig_out); }
    }
}

struct Key { vk: WORD, ch: WCHAR }

// Use the parser from the lib crate.
// LibRecord is s19parser::S19Record (repr(C), has address/data_len/data fields).

// Table rows
const BPR: usize = 16;
struct Row { addr: u32, hex: String, ascii: String }

fn build_table(records: &[LibRecord]) -> Vec<Row> {
    let mut rows = Vec::new();
    for rec in records {
        let data = &rec.data[..rec.data_len as usize];
        for (i, chunk) in data.chunks(BPR).enumerate() {
            let addr = rec.address + (i * BPR) as u32;
            let hex: String = chunk.iter().enumerate().map(|(j, b)| {
                let s = format!("{:02X}", b);
                if j > 0 { format!(" {}", s) } else { s }
            }).collect();
            let ascii: String = chunk.iter().map(|&b|
                if b >= 0x20 && b < 0x7F { b as char } else { '.' }
            ).collect();
            rows.push(Row { addr, hex, ascii });
        }
    }
    rows
}

// ANSI helpers
const RST:  &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM:  &str = "\x1b[2m";
const REV:  &str = "\x1b[7m";
const CYN:  &str = "\x1b[36m";
const YLW:  &str = "\x1b[33m";
const GRN:  &str = "\x1b[32m";
const WHT:  &str = "\x1b[37m";

fn render(out: &mut impl Write, rows: &[Row], sel: usize, path: &str,
          nrec: usize, ascii: bool, cols: usize, term_h: usize) {
    const OVERHEAD: usize = 6; // title + col-header + 2 separators + status + 1
    let vis = term_h.saturating_sub(OVERHEAD).max(1);
    let off = if sel < vis { 0 }
              else if rows.len() > vis && sel >= rows.len() - vis { rows.len() - vis }
              else { sel - vis / 2 };

    let aw = 12usize;
    let acw = if ascii { 18usize } else { 0 };
    let hw = cols.saturating_sub(aw + acw + if ascii { 7 } else { 4 }).max(20);
    let tw = (aw + 3 + hw + if ascii { 3 + acw } else { 0 }).min(cols.saturating_sub(1));

    let mut b = String::with_capacity(8192);
    b.push_str("\x1b[?25l\x1b[H\x1b[2J");

    // Title
    b.push_str(&format!(" {}{}S19 Viewer{} | {}{}{} | {}records:{} {}  {}rows:{} {}\r\n",
        BOLD, CYN, RST, YLW, path, RST, GRN, RST, nrec, GRN, RST, rows.len()));

    // Separator
    b.push_str(DIM); b.push_str(&"─".repeat(tw)); b.push_str(RST); b.push_str("\r\n");

    // Column header
    b.push_str(&format!("{}{}{}  {:<aw$}  {:<hw$}{}",
        BOLD, CYN, " ", "Address", "Hex Data (16 bytes/row)", RST, aw=aw-2, hw=hw-1));
    if ascii { b.push_str(&format!("  {}{}{:<acw$}{}", BOLD, CYN, "ASCII", RST, acw=acw)); }
    b.push_str("\r\n");
    b.push_str(DIM); b.push_str(&"─".repeat(tw)); b.push_str(RST); b.push_str("\r\n");

    // Rows
    for i in 0..vis {
        let ri = off + i;
        if ri >= rows.len() { b.push_str("\r\n"); continue; }
        let row = &rows[ri];
        let hi = ri == sel;
        if hi { b.push_str(REV); }

        let addr_s = format!("0x{:08X}", row.addr);
        if !hi { b.push_str(YLW); }
        b.push_str(&format!(" {:>aw$} ", addr_s, aw=aw-1));
        if !hi { b.push_str(RST); }

        let hex_d = if row.hex.len() + 1 > hw { &row.hex[..hw-1] } else { &row.hex };
        if !hi { b.push_str(WHT); }
        b.push_str(&format!("{:<hw$}", hex_d, hw=hw));
        if !hi { b.push_str(RST); }

        if ascii {
            if !hi { b.push_str(GRN); }
            b.push_str(&format!("  {:<acw$}", row.ascii, acw=acw));
            if !hi { b.push_str(RST); }
        }
        if hi { b.push_str(RST); }
        b.push_str("\r\n");
    }

    // Bottom separator
    b.push_str(DIM); b.push_str(&"─".repeat(tw)); b.push_str(RST); b.push_str("\r\n");

    // Status
    let pos = if rows.is_empty() { "─".into() } else { format!("{}/{}", sel+1, rows.len()) };
    let ah = if ascii { "a:hide ASCII" } else { "a:show ASCII" };
    b.push_str(&format!(" {}{}↑↓ scroll  PgUp/PgDn page  g/G top/bot  {}  q:quit   {}{}{}",
        DIM, WHT, ah, CYN, pos, RST));

    let _ = out.write_all(b.as_bytes());
    let _ = out.flush();
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: s19viewer <file.s19>");
        std::process::exit(1);
    }
    let path = &args[1];
    let records = match std::fs::read_to_string(path) {
        Ok(c) => { let r = parse_s19_lib(&c); if r.is_empty() { eprintln!("No S1/S2/S3 records found."); std::process::exit(1); } r }
        Err(e) => { eprintln!("Cannot read file: {e}"); std::process::exit(1); }
    };
    let nrec = records.len();
    let rows = build_table(&records);
    let con  = Console::new();
    let mut out = io::stdout();
    let mut sel:   usize = 0;
    let mut ascii: bool  = true;
    let n = rows.len();

    loop {
        let (cols, rh) = con.size();
        let page = (rh.saturating_sub(7)).max(1);
        render(&mut out, &rows, sel, path, nrec, ascii, cols, rh);
        let Some(k) = con.read_key() else { continue };
        match k.vk {
            VK_UP    => { if sel > 0 { sel -= 1; } }
            VK_DOWN  => { if sel + 1 < n { sel += 1; } }
            VK_PRIOR => { sel = sel.saturating_sub(page); }
            VK_NEXT  => { sel = (sel + page).min(n.saturating_sub(1)); }
            VK_HOME  => { sel = 0; }
            VK_END   => { sel = n.saturating_sub(1); }
            _ => match k.ch as u8 {
                b'q'|b'Q'|0x1B => break,
                b'j' => { if sel + 1 < n { sel += 1; } }
                b'k' => { if sel > 0 { sel -= 1; } }
                b'g' => { sel = 0; }
                b'G' => { sel = n.saturating_sub(1); }
                b'a'|b'A' => { ascii = !ascii; }
                _ => {}
            }
        }
    }
}
