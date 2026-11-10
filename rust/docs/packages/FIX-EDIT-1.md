# FIX-EDIT-1: Editing paths of the board model (generators, dimensions, small items, mutators)

Crate: kicad-pcb (files of finished packages)

The load / save / DRC / plot paths of the board model are verified on the whole corpus
(whole-corpus dump scoreboard: 6.1 million values, re-save byte-identical). What is NOT
verified, and in a few places not ported, are the EDITING paths: mutators that no load
path runs. A client that edits a board through the port (the use case: a web app or an
autorouter that places and moves items, then runs DRC and exports) reaches them. This
package closes the deferred items of the integration steps
(rust/docs/design/integration-2.md: FIX-GEN-1 and the "FIX package (dimensions / small
items)"; rust/docs/design/review-2.md: "Not verified: mutators that no load path runs") and
proves the mutators against the reference.

1. Deferred ports:
   - pcb_generator.rs: the subclass overrides in the generator arms of `Board::move_item` /
     `rotate_item` / `flip_item` / `mirror_item` / `set_item_layer` (PCB_TUNING_PATTERN moves
     m_end and the base lines and its SetLayer sets the members' layer; PCB_VIA_STITCH Move
     moves only its outline and origin; PCB_VIA_STACK transforms hops and flips start / end
     layers - read pcbnew/generators/*.cpp and M23's notes; the typed members live in the
     property map: use the typed views and re-normalise), the default PCB_VIA template of a
     via stitch (the C++ always writes one), the PCB_TUNING_PATTERN constructor defaults for
     the defaults dump, the typed getters the property tables wait for (PROP-1: tuning
     pattern / via stack / via stitch property getters and availability predicates).
   - pcb_dimension.rs: PCB_DIMENSION_BASE::Update() after every EDA_TEXT setter of a
     dimension (ClearRenderCache -> Update in C++; the parser emulates the last call),
     `GetValueFieldText` / `ChangeValue*` (the 'Value' property), i32-backed
     `DimUnitsFormat` / `DimPrecision` / `DimTextPosition` so out-of-range file values
     survive a re-save as the reference writes them (verify with the reference first, as
     was done for zone_connect).
   - pcb_grid_item.rs radius setters / getters and `set_orientation_degrees`;
     `Board::table_autosize` / `table_flip` hooks for generated tables (M26 notes);
     `Pad::padstack()` accessor and TEARDROP_MANAGER::IsUniformlyRound's needs;
     `PcbShape::normalize` of a pad primitive with the pad's primitive lib scale.
2. Mutator oracle: a C++ driver against the reference build (the pattern of
   kicad-pcb tests/oracle/int2a_oracle.cpp and stabpcb_oracle.cpp: links the pcb-dump
   objects, one compile under the machine-wide flock) that loads a board, applies ONE
   scripted edit and dumps the affected items in pcb-dump format - for EVERY item kind
   (track, arc, via, pad, footprint with every child kind, shape of every SHAPE_T, text,
   text box, table, dimension of every kind, zone, group, generator of every kind, barcode,
   reference image, target, point) and every mutator a client can call: Move, Rotate (90 /
   180 / arbitrary angle, around a point), Flip (both directions, with and without
   "flip left-right"), Mirror, SetLayer, SetPosition, SetWidth / SetSize where they exist,
   SetNet, footprint SetOrientation / SetPosition / SetLayerAndFlip / transform scale with
   all children, delete and re-add, on boards with rotated / flipped / scaled footprints.
   The Rust replay applies the same edit through the model API (`Board::move_item` etc.)
   and compares every dumped value. Fix every mismatch (regression case = the scripted
   edit). Then the same edits followed by save: `Board` re-saved by the port must equal the
   driver's `SaveBoard` output byte for byte.
3. Item creation the way a router does it: add tracks / arcs / vias with net and layer,
   check `PCB_VIA` layer pair sanitising, default sizes from netclass, and that connectivity
   (`ConnectivityData::add` / update paths, kicad-connectivity) gives the same nets and
   ratsnest as a full rebuild and as the reference after the same additions (driver:
   BOARD::Add + connectivity Update vs full Build).

Other agents: a DRC stabilisation pass adds a board UUID field to `Board` (kicad-pcb
board/mod.rs); the drawing sheet and fabrication chains do not touch kicad-pcb items.
Re-read a file right before you edit it.

Report: per item kind and mutator: values compared / mismatches before and after; what
remains unported (with the C++ function names) and why.
