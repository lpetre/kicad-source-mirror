# STAB-CN: Connectivity with pads - rerun every oracle, analyse and fix

Crate: kicad-connectivity (all files; the four port packages, the review and the tie fix are
finished - read PORT_STATUS.md and rust/docs/design/review-connectivity.md first)

Until now no board with pads could be loaded, so every pad path of the connectivity port is
unverified (review finding F-10): pad anchors and layers, origin-pad ranking and conflicting
clusters in net propagation, jumper pads, `HasSingleConnection`, the pad branches of
`IsConnectedOnLayer` and of the contact rules, zone islands (without pads every island is
orphaned), `single_connection_outlines`. The pad parser has just landed
(`kicad_pcb::io::parse_board` now loads 359 of 370 corpus boards byte-exactly), and the
goldens for boards with pads have been waiting, committed.

Work:
1. Run `cargo test -p kicad-connectivity -- --nocapture` and the ignored corpus tests. The
   golden tests switch pad comparisons on automatically (cn01 items, cn02 algorithm, cn03
   ratsnest, cn04 data incl. the pad flash goldens under tests/golden/cn04_flash, the review's
   order goldens: 15 of its 22 boards were waiting for pads).
2. Extend the golden set with pads in mind (the generator scripts under tests/oracle run the
   existing reference tools; pcb-dump `connectivity` section and cn-order-oracle; no new C++
   build is needed unless a field is missing): at least thirty more corpus boards with
   footprints (small demos and qa boards: through-hole and SMD pads, pads in zones with
   thermal reliefs, net ties, jumper pad groups, custom-shape pads, pads with
   remove-unused-layers, NPTH), hashed goldens, small files only (PORTING.md section 7).
3. Analyse EVERY mismatch down to the statement: a defect of the connectivity port (fix it,
   with the board as regression case), a defect of the kicad-pcb pad model (pad shapes,
   layers, FlashLayer: prove it with `pcb-dump board --sections pad` vs pcb-dump-rs and
   report it in change_requests with the smallest reproducing board - a pad fix package
   (back-side mask / paste margins, pad primitive parents) is running in kicad-pcb right
   now), or one of the documented heap-order sites of the review (then the comparison must
   be tie-aware, not the port changed).
4. The net propagation result matters most: after `build_connectivity` the net of every
   track, via, zone and copper shape must equal the reference's (the board re-save writes
   them). Compare on the full corpus (every board under 5 MB) through `pcb-dump board
   --loader --sections item` hashed vs the Rust load path of kicad-session
   (`pcb-dump-session`, see kicad-session PORT_STATUS.md PCBIO-15; do not edit
   kicad-session: another package is editing its loader right now) and report the scoreboard.

Verification / report: per test file, boards compared and fields matched / mismatched before
and after; the list of mismatches with cause and status; what is still unverified. All
connectivity tests green at the end, or every red one explained by a named kicad-pcb defect.
