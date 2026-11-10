# FIX-PAD-1: Back-side pad margins, pad primitive parents, PAD::AddPrimitive in the model

Crate: kicad-pcb (src/pad/derive.rs and the pad model files of package M14, the pad
primitive helpers; minimal edits in src/board/containers.rs `footprint_add` or the T_pad
case of src/io/parser/footprint.rs, and in src/io/parser/pad.rs to move its helper)

The pad parser / writer package (PCBIO-07, finished: read its section in
rust/crates/kicad-pcb/PORT_STATUS.md) found two model defects. Both are in finished
packages' files.

1. Back-side margins (affects B.Mask / B.Paste plots and the solder mask DRC):
   `Pad::set_local_solder_mask_margin`, `set_local_solder_paste_margin` and
   `set_local_solder_paste_margin_ratio` write the front outer layer only. pcbnew/pad.h sets
   both (`m_padStack.SolderMaskMargin( F_Mask ) = v; m_padStack.SolderMaskMargin( B_Mask ) =
   v;`, likewise paste with F_Paste / B_Paste, and the ratio). Port the C++ setters and
   getters exactly (read pad.h / pad.cpp / padstack.h for every margin, ratio, tenting,
   covering, plugging, capping and filling accessor: check ALL two-sided properties of the
   pad and padstack model for the same mistake, not only the three named ones, including
   what the copy / flip / mirror paths do with front and back).
2. Pad primitives keep `ParentRef::None` after parsing; in C++ `PAD::AddPrimitive` does
   `SetParent( this )`. Add `Pad::add_primitive` to the model (port of PAD::AddPrimitive with
   OverrideLibCoords / Bezier / Poly and rebakePrimitiveToFootprint via GetPrimitiveLibScale:
   the parser has a faithful private helper `pad_add_primitive` in io/parser/pad.rs - move
   it), and make the primitives' parent the pad once the pad has its handle (re-parent in
   `footprint_add`, or wherever the design - board-model.md sections 4 and 6 - says child
   handles are fixed up). Then check every reader that behaves differently for a detached
   shape: context-free getters (`lib_bezier_c1`, `lib_poly_shape`), `rebake_from_lib` /
   `sync_lib_coords`, the max-error lookup through the board, the pad primitive branches of
   PcbShape::flip / mirror / rebake_with_scale (M9 note: a pad primitive must treat
   transformFp() as null).
   Another package (GLUE-2, running at the same time in other kicad-pcb files) has
   `Pad::delete_primitives_list` / `add_primitive_poly` on its list (kicad-fab has local
   ports in plot_board_layers.rs): if they are not in the model when you get there, add them
   next to `add_primitive` and tell it in your result; if they are, build on them. Re-read
   a file right before you edit it.

Verification: `tools/io-corpus/run.sh --dump <board> --sections item,pad` (pcb-dump --no-init
vs pcb-dump-rs) on the boards PCBIO-07 lists with pad-section mismatches (padstacks,
issue5978, solder_mask_bridge_test, issue22751, the m14 / m15 fixtures, the hand-made pad
boards) and on twenty more corpus boards with pads on both sides: ZERO mismatches in the
item, pad and bbox sections (the pad section includes per-layer effective shapes and
polygons). The byte-level goldens of PCBIO-07 (tests/pcbio07_pads.rs), the io-corpus
scoreboard (359 identical of 370 before you start) and `cargo test -p kicad-pcb --features
dump` must not get worse. Add the smallest board that showed each defect as a committed
regression case.
