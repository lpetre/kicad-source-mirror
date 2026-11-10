# PCBIO-12: Board file other items - reference images, barcodes, targets, points, grid items, tables, generated tables, drill maps (parse + write)

Crate: kicad-pcb

You own src/io/parser/misc_items.rs and src/io/writer/misc_items.rs (staging stubs created by
the parser / writer core packages).

C++: the PCBIO-12 row of rust/docs/analysis/pcb-io.md section 10 lists the exact ranges of
pcb_io_kicad_sexpr_parser.cpp and pcb_io_kicad_sexpr.cpp: parseREFERENCE_IMAGE (image data is
base64 in `(data ...)` lines: concatenate, `wx_base64_decode`, then
`image.read_image_file(&bytes)`; if it returns false the error is 'Failed to read image
data.'; use `set_image_scale` for `(scale)` - the model package M24 documented the exact calls
in the M24 section of rust/crates/kicad-pcb/PORT_STATUS.md), parsePCB_BARCODE, parsePCB_TARGET,
parsePCB_POINT, the grid item and drill map tokens of this tree, parsePCB_TABLE with its
cells (cells reuse the text-box parser of PCBIO-05, which is done: read its section in
PORT_STATUS.md), generated tables (drill chart / stackup table data incl. hidden columns), and
the matching format functions (image data through FormatStreamData with `image_data()`).
Models: small items M23 (done), images M24 (done), barcode data-only (tier 2), tables M11
(landing right now: call it; stubs panic until then).

Board file I/O package rules: specification rust/docs/analysis/pcb-io.md (precedence note;
sections 3-7 for your tokens: the token tree, the list of version-gated branches, tolerant vs
strict places, optional-token rules, number formatting calls). The parser core (PCBIO-01) and
writer core (PCBIO-02) are complete: read their sections in rust/crates/kicad-pcb/PORT_STATUS.md
and the module docs of src/io/parser/mod.rs and src/io/writer/mod.rs (shared parser state and
helpers, the staging-stub convention, allow_pending). Replace the stub bodies of YOUR files
with faithful ports: every token, every version-dependent branch with the same version
boundaries, the same error texts, the same tolerance of unknown tokens; on the writer side the
same token order, optional-token rules and number formatting calls. Model functions that are
still port_pending! stubs are called anyway.

Verification: (1) item level: `pcb-dump board --no-init --sections item,misc,text` on COPIES of
corpus boards containing these items (grep -l for '(image', '(barcode', '(target', '(table',
'(point' under qa/data/pcbnew and demos; small boards only) against
`cargo run -q -p kicad-pcb --features dump --bin pcb-dump-rs -- board --sections item,misc,text`
through rust/tools/pcb-dump/compare.py: zero mismatches; (2) byte level: the corpus harness
rust/tools/io-corpus/run.sh (package PCBIO-17, may still be in progress) or by hand: re-save
COPIES with the reference `kicad-cli pcb upgrade --force` and require the item text to be
reproduced byte for byte on every corpus board that has such items (follow the oracle pattern
of tests/pcbio03_tracks_shapes.rs and tests/oracle/pcbio03_golden.sh: block extraction,
committed small goldens); (3) port the C++ tests the analysis row names
(test_barcode_load_save, reference image / table load-save tests). Report the numbers.
