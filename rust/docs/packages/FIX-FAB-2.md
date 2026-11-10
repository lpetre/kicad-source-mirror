# FIX-FAB-2: command line leftovers of the fabrication exports

Crate: kicad-cli (`src/command_pcb_export_drill.rs`, `src/command_pcb_export_gerbers.rs`,
`src/host.rs`) and kicad-session (`src/jobs_handler.rs`, tests). No change in kicad-fab/src.
Can be folded into WIRE-1 if that package is still open.

Origin: findings F-04, F-01 (HPGL part), F-02, F-05 of `rust/docs/design/review-fab.md`
(package REV-FAB).

## 1. `--report-path` into a directory that does not exist (F-04)

`kicad-cli pcb export drill --generate-report --report-path out/rep/r.txt` with no `out/rep`:
`GenDrillReportFile` cannot open the file, wx logs
`Error: can't open file '<path>' (error 2: No such file or directory)` on stderr, the job
returns `ERR_INVALID_OUTPUT_CONFLICT` (exit code 4), no report is written; the drill files and
their `Created file` lines are there (`pcbnew_jobs_handler.cpp` L2425-2430, L2460-2465).
kicad-rs creates the directory, writes the report and exits with 0.

Work: the host must not create parent directories for the report file (only the output
directory of the job is created, by `PATHS::EnsurePathExists`); on failure exit code 4. The wx
timestamped log line need not be reproduced byte for byte (it carries the time of day), but an
error line on stderr should say the same.

Check which other outputs the host writes through the same "create parents" helper and compare
each with the C++ (`-o` of both commands was compared in the review in 13 forms and is right).

## 2. Exit code and job file when a layer cannot be plotted (F-01)

With `--board-plot-params` and a stored `(outputformat 0)` (HPGL) the reference reports
`Failed to plot to '<file>'.` for every layer, continues, writes the `.gbrjob` and exits with
4 (`pcbnew_jobs_handler.cpp` L1893-1911). kicad-rs stops at the first layer with exit code 2
and writes nothing. Port the loop's behaviour for the HPGL message (it is a C++ error text:
`HPGL plotting is no longer supported as of KiCad 10.0`). The other stored formats (PostScript,
DXF, PDF; SVG until FAB-11) stay out of scope: there the reference writes that format into the
`.gbr` files; keep the clear "not part of the port" error, but decide the exit code once
(today 2 = NOT_YET_PORTED) and document it in PORT_STATUS FAB-07.

## 3. Regression coverage that does not exist yet

The review ran these by hand; none of them is in a committed test, and one of them (`-D`) was
wrong in the 05:16 UTC build of 2026-11-10 and right in the 05:50 build:

* `-D KEY=VALUE` on a board whose texts use the variable
  (`qa/data/pcbnew/issue22267.kicad_pcb`, `tools/d_sets.txt`): User.Drawings and F.Silkscreen
  change in the reference.
* The option matrix of `rust/docs/design/review-fab/tools/gerber_sets.txt` and the stored
  parameter variants of `stored_variants.txt` (`mutate.py`): add them to the end-to-end
  scoreboard as hashed goldens (FNV per file, as `kicad-session/tests/golden/gerbers_job`), on
  three or four small boards with footprints (`basic_test`, `issue6945`, `ecc83-pp`) and on a
  copy with DNP footprints (`mutate.py ... @dnp=8`), because no corpus board has one.
* The Excellon factorial of `drill_matrix.py full` on `drill_features.kicad_pcb` (256 small
  files; hash them).
* `--variant` (F-05) as soon as `Board::set_current_variant` exists: the board recipe is in
  the finding; `tools/var_sets.txt`.

## Verification

`tools/fabcmp.py` / `tools/matrix.py` for the cases above report `OK`; `cargo test -p
kicad-session -p kicad-cli` green; the new goldens are produced by a `generate.py` next to
them from the reference kicad-cli.
