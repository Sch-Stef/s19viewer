#!/usr/bin/env python3
"""S19 Viewer - GUI using tkinter (stdlib only, no pip required).

Parsing is done by the Rust DLL (s19parser.dll) when available,
falling back to a pure-Python implementation automatically.

Usage:
    python s19viewer.py [file.s19]
"""

import sys
import os
import ctypes
import tkinter as tk
from tkinter import ttk, filedialog, messagebox

# ── Rust DLL integration ──────────────────────────────────────────────────────

class _S19RecordC(ctypes.Structure):
    """Mirrors the repr(C) S19Record struct from lib.rs."""
    _fields_ = [
        ("record_type", ctypes.c_uint8),   # 1=S1, 2=S2, 3=S3
        ("address",     ctypes.c_uint32),
        ("data_len",    ctypes.c_uint32),
        ("data",        ctypes.c_uint8 * 255),
    ]

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

_rust_lib = None
_USING_RUST = False

def _try_load_rust_dll():
    global _rust_lib, _USING_RUST
    dll_path = _find_dll()
    if dll_path is None:
        return False
    try:
        lib = ctypes.CDLL(dll_path)
        # s19_parse_file(path: *const c_char, out_count: *mut u32) -> *mut S19Record
        lib.s19_parse_file.restype  = ctypes.POINTER(_S19RecordC)
        lib.s19_parse_file.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint32)]
        # s19_free(ptr: *mut S19Record, count: u32)
        lib.s19_free.restype  = None
        lib.s19_free.argtypes = [ctypes.POINTER(_S19RecordC), ctypes.c_uint32]
        _rust_lib = lib
        _USING_RUST = True
        return True
    except OSError:
        return False

_try_load_rust_dll()

RT_NAMES = {1: "S1", 2: "S2", 3: "S3"}

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


# ── Pure-Python fallback parser ───────────────────────────────────────────────

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


def parse_s19(path):
    """Parse an S19 file. Uses Rust DLL if available, pure Python otherwise."""
    if _USING_RUST:
        return _parse_s19_rust(path)
    return _parse_s19_python(path)


# ── Parser status label helper ────────────────────────────────────────────────

PARSER_LABEL = "Rust DLL" if _USING_RUST else "Python (DLL not found)"


def build_rows(records, bpr=16):
    rows = []
    for rt, addr, data in records:
        for i in range(0, len(data), bpr):
            chunk    = data[i:i + bpr]
            off_addr = addr + i
            hex_s    = " ".join(f"{b:02X}" for b in chunk)
            hex_s    = f"{hex_s:<{bpr*3 - 1}}"
            ascii_s  = "".join(chr(b) if 0x20 <= b < 0x7F else "." for b in chunk)
            rows.append((rt, f"0x{off_addr:08X}", hex_s, ascii_s))
    return rows


class S19ViewerApp(tk.Tk):
    def __init__(self, initial_file=None):
        super().__init__()
        self.title("S19 Viewer")
        self.geometry("1150x680")
        self.configure(bg="#1e1e1e")
        self._records = []
        self._all_rows = []
        self._build_ui()
        self._bind_keys()
        if initial_file:
            self._load_file(initial_file)

    def _build_ui(self):
        # Toolbar
        toolbar = tk.Frame(self, bg="#2d2d2d", pady=5)
        toolbar.pack(side=tk.TOP, fill=tk.X)

        tk.Button(toolbar, text="📂  Open S19 file", command=self._open_file,
                  bg="#0e639c", fg="white", relief=tk.FLAT, padx=12, pady=4,
                  cursor="hand2", font=("Segoe UI", 10, "bold"),
                  activebackground="#1177bb", activeforeground="white",
                  ).pack(side=tk.LEFT, padx=8)

        ttk.Separator(toolbar, orient=tk.VERTICAL).pack(side=tk.LEFT, fill=tk.Y, padx=6, pady=3)

        self._lbl_file = tk.Label(toolbar, text="No file loaded", fg="#888888",
                                  bg="#2d2d2d", font=("Consolas", 10), anchor="w")
        self._lbl_file.pack(side=tk.LEFT, padx=4, fill=tk.X, expand=True)

        self._lbl_stats = tk.Label(toolbar, text="", fg="#4ec94e",
                                   bg="#2d2d2d", font=("Consolas", 10), anchor="e")
        self._lbl_stats.pack(side=tk.RIGHT, padx=12)

        # Filter / options bar
        fbar = tk.Frame(self, bg="#252525", pady=4)
        fbar.pack(side=tk.TOP, fill=tk.X)

        tk.Label(fbar, text="  Filter address (hex):", fg="#aaaaaa",
                 bg="#252525", font=("Segoe UI", 9)).pack(side=tk.LEFT)

        self._filter_var = tk.StringVar()
        self._filter_var.trace_add("write", lambda *_: self._apply_filter())
        tk.Entry(fbar, textvariable=self._filter_var, bg="#333333", fg="white",
                 insertbackground="white", relief=tk.FLAT, width=16,
                 font=("Consolas", 10)).pack(side=tk.LEFT, padx=6)

        tk.Label(fbar, text="e.g. 0x0800", fg="#555555", bg="#252525",
                 font=("Segoe UI", 8)).pack(side=tk.LEFT)

        tk.Button(fbar, text="✕", command=lambda: self._filter_var.set(""),
                  bg="#3a3a3a", fg="#cccccc", relief=tk.FLAT, width=2,
                  cursor="hand2", font=("Segoe UI", 9),
                  ).pack(side=tk.LEFT, padx=4)

        ttk.Separator(fbar, orient=tk.VERTICAL).pack(side=tk.LEFT, fill=tk.Y, padx=10, pady=2)
        tk.Label(fbar, text="Bytes/row:", fg="#aaaaaa", bg="#252525",
                 font=("Segoe UI", 9)).pack(side=tk.LEFT)

        self._bpr_var = tk.StringVar(value="16")
        bpr_cb = ttk.Combobox(fbar, textvariable=self._bpr_var,
                               values=["8", "16", "32"], width=4,
                               state="readonly", font=("Consolas", 10))
        bpr_cb.pack(side=tk.LEFT, padx=6)
        bpr_cb.bind("<<ComboboxSelected>>", lambda *_: self._reload_rows())

        ttk.Separator(fbar, orient=tk.VERTICAL).pack(side=tk.LEFT, fill=tk.Y, padx=10, pady=2)
        self._ascii_var = tk.BooleanVar(value=True)
        tk.Checkbutton(fbar, text="Show ASCII", variable=self._ascii_var,
                       command=self._toggle_ascii, bg="#252525", fg="#cccccc",
                       selectcolor="#333333", activebackground="#252525",
                       activeforeground="#cccccc", font=("Segoe UI", 9),
                       ).pack(side=tk.LEFT, padx=4)

        # Table
        tframe = tk.Frame(self, bg="#1e1e1e")
        tframe.pack(fill=tk.BOTH, expand=True, padx=6, pady=(2, 0))

        style = ttk.Style(self)
        style.theme_use("clam")
        style.configure("Treeview", background="#1e1e1e", foreground="#d4d4d4",
                         fieldbackground="#1e1e1e", rowheight=22, font=("Consolas", 10))
        style.configure("Treeview.Heading", background="#2d2d2d", foreground="#569cd6",
                         relief=tk.FLAT, font=("Consolas", 10, "bold"))
        style.map("Treeview",
                  background=[("selected", "#264f78")],
                  foreground=[("selected", "#ffffff")])

        self._tree = ttk.Treeview(tframe, show="headings", selectmode="browse")
        self._tree.tag_configure("even", background="#1e1e1e")
        self._tree.tag_configure("odd",  background="#252526")

        vsb = ttk.Scrollbar(tframe, orient=tk.VERTICAL,   command=self._tree.yview)
        hsb = ttk.Scrollbar(tframe, orient=tk.HORIZONTAL, command=self._tree.xview)
        self._tree.configure(yscrollcommand=vsb.set, xscrollcommand=hsb.set)
        vsb.grid(row=0, column=1, sticky="ns")
        hsb.grid(row=1, column=0, sticky="ew")
        self._tree.grid(row=0, column=0, sticky="nsew")
        tframe.rowconfigure(0, weight=1)
        tframe.columnconfigure(0, weight=1)

        self._setup_columns()

        # Status bar
        self._status = tk.Label(self, text="Open an .s19 file to begin.",
                                 fg="#666666", bg="#1a1a1a",
                                 font=("Segoe UI", 8), anchor="w", pady=3)
        self._status.pack(side=tk.BOTTOM, fill=tk.X, padx=8)

    def _setup_columns(self):
        show_ascii = self._ascii_var.get()
        cols = ("type", "address", "hex", "ascii") if show_ascii else ("type", "address", "hex")
        self._tree["columns"] = cols
        self._tree.heading("type",    text="Type",    anchor="w")
        self._tree.heading("address", text="Address", anchor="w")
        self._tree.heading("hex",     text="Hex Data", anchor="w")
        self._tree.column("type",    width=52,  minwidth=40,  stretch=False, anchor="w")
        self._tree.column("address", width=115, minwidth=90,  stretch=False, anchor="w")
        self._tree.column("hex",     width=460, minwidth=200, stretch=True,  anchor="w")
        if show_ascii:
            self._tree.heading("ascii", text="ASCII", anchor="w")
            self._tree.column("ascii", width=170, minwidth=80, stretch=False, anchor="w")

    def _bind_keys(self):
        self.bind("<Control-o>", lambda _: self._open_file())
        self._tree.bind("<Control-c>", self._copy_row)

    def _open_file(self):
        path = filedialog.askopenfilename(
            title="Open S19 / Motorola SREC file",
            filetypes=[("S19 files", "*.s19 *.S19 *.mot *.MOT *.srec *.SREC"),
                       ("All files", "*.*")])
        if path:
            self._load_file(path)

    def _load_file(self, path):
        try:
            records = parse_s19(path)
        except OSError as e:
            messagebox.showerror("Error", f"Cannot read file:\n{e}")
            return
        if not records:
            messagebox.showwarning("No data", "No S1/S2/S3 records found.")
            return
        self._records = records
        self._lbl_file.config(text=path, fg="#4fc1ff")
        self.title(f"S19 Viewer — {os.path.basename(path)}")
        self._reload_rows()

    def _reload_rows(self):
        bpr = int(self._bpr_var.get())
        self._all_rows = build_rows(self._records, bpr)
        self._lbl_stats.config(text=f"{len(self._records)} records  |  {len(self._all_rows)} rows  |  {PARSER_LABEL}")
        self._apply_filter()

    def _apply_filter(self):
        raw = self._filter_var.get().strip().lstrip("0x").lstrip("0X")
        filter_prefix = None
        if raw:
            try:
                filter_prefix = raw.upper()
            except ValueError:
                pass

        visible = []
        for row in self._all_rows:
            if filter_prefix is not None:
                addr_hex = row[1][2:].upper()   # strip "0x"
                if not addr_hex.startswith(filter_prefix):
                    continue
            visible.append(row)

        self._populate_table(visible)
        note = f"  (filter: 0x{raw.upper()})" if filter_prefix else ""
        self._status.config(text=f"Showing {len(visible)} of {len(self._all_rows)} rows{note}")

    def _populate_table(self, rows):
        self._tree.delete(*self._tree.get_children())
        show_ascii = self._ascii_var.get()
        for i, (rt, addr, hex_s, ascii_s) in enumerate(rows):
            tag = "even" if i % 2 == 0 else "odd"
            vals = (rt, addr, hex_s, ascii_s) if show_ascii else (rt, addr, hex_s)
            self._tree.insert("", tk.END, values=vals, tags=(tag,))

    def _toggle_ascii(self):
        self._setup_columns()
        self._apply_filter()

    def _copy_row(self, _=None):
        sel = self._tree.selection()
        if not sel:
            return
        vals = self._tree.item(sel[0], "values")
        text = "\t".join(str(v) for v in vals)
        self.clipboard_clear()
        self.clipboard_append(text)
        self._status.config(text=f"Copied to clipboard: {text[:100]}")


if __name__ == "__main__":
    initial = sys.argv[1] if len(sys.argv) > 1 else None
    app = S19ViewerApp(initial_file=initial)
    app.mainloop()
    app.mainloop()
