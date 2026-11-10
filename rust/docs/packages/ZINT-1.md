# ZINT-1: Barcode symbol encoder (port of the zint subset KiCad uses)

Crate: kicad-zint (NEW crate rust/crates/kicad-zint; the workspace takes `crates/*`
automatically - create a valid Cargo.toml (workspace package fields and lints like the
other crates, no dependencies) and src/lib.rs FIRST, so nobody's cargo run trips over a
half-made crate), then the one call site in kicad-pcb (src/pcb_barcode.rs `encode_symbol`
and the dependency line in kicad-pcb/Cargo.toml)

PCB_BARCODE (pcbnew/pcb_barcode.cpp ComputeBarcode, L500-600) encodes its text with the
bundled zint 2.15.0 (thirdparty/zint, BSD-3-Clause: keep its copyright notice and licence
text in the header of every ported file next to the header PORTING.md section 10 asks for;
BSD-3 is compatible with the workspace licence) and turns `ZBarcode_Buffer_Vector`'s
rectangle (and hexagon) list into polygons. The board model's barcode (package M25a) is
complete except this call: read the "Package M25a" section of
rust/crates/kicad-pcb/PORT_STATUS.md - it lists what a faithful encoder needs, the
`ZintOptions` / `ModuleMatrix` types already defined in pcb_barcode.rs, and the ported zint
settings (`zint_options`). Without the encoder a non-empty barcode has an empty symbol:
bounding boxes, hulls, DRC and plots of every barcode differ from the reference.

Port, statement by statement from thirdparty/zint/backend (C, ~12,000 lines in the files
concerned; port only what the five symbologies reach, and say which functions you left
out and why they are unreachable from KiCad's settings):
- library.c: `ZBarcode_Create` defaults, `ZBarcode_Encode` / `ZBarcode_Encode_Segs` with
  every input check, escape / ECI / UNICODE_MODE handling that applies, the warning and
  error levels, and the exact `errtxt` strings (KiCad shows them: `GetLastError`);
- common.c, reedsol.c, eci.c (UTF-8 and the conversions UNICODE_MODE tries first: zint
  converts UTF-8 input to ISO 8859-1 / Shift JIS for QR etc. before falling back - port the
  tables it really reaches: sjis.h, gb2312.h ... only if the QR / Data Matrix paths with
  KiCad's settings (eci = 26 for non-ASCII text, UNICODE_MODE) reach them; prove what is
  reached with the C code, not by guessing), large.c if needed;
- code.c `code39` (BARCODE_CODE39, option_2 0), code128.c (BARCODE_CODE128 with zint's
  automatic code-set selection), qr.c (BARCODE_QRCODE with option_1 = ECC 1..4: segment /
  mode optimisation, version selection, mask evaluation - the mask choice must be
  bit-identical; and BARCODE_MICROQR with option_1 = ECC), dmatrix.c (BARCODE_DATAMATRIX
  ECC200: encodation look-ahead, automatic size selection, placement);
- vector.c `ZBarcode_Buffer_Vector( symbol, 0 )`: the rectangle list in zint's order with
  the float (C `float`) coordinates, quiet zones / borders as the defaults give them, the
  bar-height logic for the two linear codes (default heights, `output.c` helpers such as
  `out_large_bar_height` if reached). Use `f32` where the C uses `float` and keep the
  operation order (PORTING.md floating-point rules).

API: one entry `encode(options, utf8_text) -> Result<Symbol, EncodeError>` with
`Symbol { rows, width, row heights, the module bitmap, vector: rectangles / hexagons, scale,
warning text }` and the error carrying zint's error level and `errtxt`. Then wire
`kicad_pcb::pcb_barcode::encode_symbol` to it (map the result into the existing
`ModuleMatrix`; error text into `last_error` exactly where the C++ assigns it) and remove
the ENCODER_NOT_AVAILABLE stand-in. Another agent (STAB-PCB) is working in other kicad-pcb
files right now: re-read pcb_barcode.rs and Cargo.toml right before you edit them, and
make the wiring one small atomic edit after the crate is finished and verified.

Verification (mandatory, differential against the real C code):
1. Oracle driver in C (rust/crates/kicad-zint/tests/oracle/: compile the bundled
   thirdparty/zint/backend sources for the five symbologies, or link the zint library of the
   reference build tree /home/user/kicad-oracle/build if it has one; build under the
   machine-wide flock, see PORTING.md section 7 and rust/docs/ORACLE.md). For each case it
   prints: the return code, errtxt, rows, width, the encoded bitmap rows, and the vector
   rectangles (hex of the float bits) and hexagons.
2. Cases: for each symbology and each ECC level - all lengths from 1 to the capacity limit
   and beyond (the error path), digits / upper case / mixed / punctuation / control
   characters / high Latin-1 / multi-byte UTF-8 (with and without eci = 26 as KiCad sets
   it) / kanji; several hundred thousand pseudo-random inputs from a fixed seed
   (deterministic generator shared by the C driver and the Rust test: the cases are
   generated, the golden stores a running hash per block of 1,000 cases plus the first
   failing case when regenerated - keep committed data under 300 KB); every input that
   makes zint switch code set / mode / mask / size at a boundary.
3. End to end: `pcb-dump board --sections misc,bbox` vs pcb-dump-rs on the barcode boards
   (qa/data/pcbnew/issue24525, api_kitchen_sink, project_barcode, the M25a test board
   tests/data/m25a): barcode `poly`, `symbol_poly`, `text_poly`, `last_error`, bounding box,
   and the footprint boxes around barcodes - zero mismatches (GLUE-2 made the dump print
   these as pending for non-empty texts: switch them back on); the two barcode layers the
   Gerber corpus of kicad-fab skips today (project_barcode F.Silkscreen ...) byte-identical
   (`cargo test -p kicad-fab` plot corpus: remove them from its skip / known list only if
   they match, and report).
The wasm32-unknown-unknown build of kicad-zint and kicad-pcb must pass; no `unsafe`, no
dependencies.
