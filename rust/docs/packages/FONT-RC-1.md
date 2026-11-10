# FONT-RC-1: Outline-font text without a font engine - the stored render cache as stand-in

Crate: kicad-pcb (src/eda_text.rs and the text host files, io/parser/text.rs,
io/writer/text.rs), then the readers in kicad-fab (plot_brditems_text.rs) and kicad-drc
(providers/text_dims.rs) - minimal edits there

Background (review finding R2-06, rust/docs/design/review-2.md; orchestrator decision): for
text in an outline (TrueType) font the reference never uses the `(render_cache ...)` stored
in the board file: loading clears it (EDA_TEXT::SetVisible -> ClearRenderCache and friends)
and every reader - plotting, DRC text checks, bounding boxes, the writer at save -
regenerates the glyphs from the font installed on the host through FreeType / HarfBuzz.
The port has no outline font engine (kicad-font: stroke font only), so today such text is
drawn with the stroke font: wrong shapes in Gerbers and DRC, and the re-save drops or
rewrites the cache.

Decision: until an outline engine exists, the glyphs stored in the file are the stand-in.
They are what the designer's KiCad produced with the real font, i.e. what the reference
produces on a machine that has the font. This is a documented deviation from the C++
control flow, not from its results on such a machine.

Work:
1. Model: keep the render cache read from the file as separate "file glyph" data on the
   text (it must survive the faithful ClearRenderCache calls of the load path), together
   with the key the C++ cache is valid for (EDA_TEXT::GetRenderCache: resolved text, font,
   angle, offset / attributes - read eda_text.cpp for exactly what invalidates it). An
   accessor returns the stand-in glyphs only while that key still matches the text's
   current state (shown text after variable expansion, attributes, angle, position
   offset applied the way GetRenderCache applies it); after an edit that changes the key
   it returns None.
2. Readers: wherever the C++ asks the outline font for glyphs (GetRenderCache,
   TransformTextToPolySet, GetEffectiveTextShape, the text bounding box, plotting in
   kicad-fab, the TrueType branch of the text thickness check in kicad-drc), use the
   stand-in when it is valid; otherwise fall back as today (stroke font) and count the
   fallback in a queryable warning list (the hosts print "text uses font X which is not
   available; drawn with the stroke font").
3. Writer: for outline-font text with a valid stand-in write the stored cache back exactly
   as the reference writes a cache (format(render_cache): number formatting, polygon
   order), so that a board whose fonts the reference has re-saves identically; without a
   valid stand-in write what the reference writes when it cannot produce glyphs (find out
   with the oracle: a text in a font that is not installed).
4. Embedded fonts (KiCad stores font files in the board's embedded_files): out of scope
   here; note in PORT_STATUS.md what an outline engine package would need (FreeType outline
   decomposition and HarfBuzz shaping equivalents, the font lookup order incl. embedded
   fonts, the oracle machine's installed fonts from `fc-list`).

Verification: the oracle machine lacks most fonts, so identical bytes are only reachable
where the reference itself reproduces the stored cache. Establish the cases with the
oracle: (a) craft a board whose text uses a font the oracle machine HAS (`fc-list`), let the
reference save it (it writes its own cache), then compare port vs reference for re-save
(byte-identical), `pcb-dump board --sections text,bbox` (identical), Gerber plot of the
layer (byte-identical), DRC text_height / text_thickness (identical); (b) a font the
machine lacks: document what the reference does and what the port does; (c) the corpus
boards with outline fonts (issue22475, issue14549, the kicad-fab crafted text board User.2 /
User.3, the four boards whose re-save differs only in render caches): report per board.
`cargo test` of kicad-pcb, kicad-fab, kicad-drc green for the touched files.
