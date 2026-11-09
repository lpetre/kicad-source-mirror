# Project, net and board design settings - porting spec

Analyst document for the port of "how a board gets its rules and net classes": the project file
(`.kicad_pro`), net settings and net classes, `BOARD_DESIGN_SETTINGS`, net info, board stackup,
component classes, text variables and the glue that binds all of it to a loaded board. KiCad
master, board file format `20261031`. Everything below was verified against the C++ tree in this
repository; line numbers refer to the files as checked in. Abbreviations:

- **BDS** = `pcbnew/board_design_settings.cpp` (2093 lines), class `BOARD_DESIGN_SETTINGS`
- **NS** = `common/project/net_settings.cpp` (1906 lines), class `NET_SETTINGS`
- **PF** = `common/project/project_file.cpp` (1039 lines), class `PROJECT_FILE`
- **JS** = `common/settings/json_settings.cpp` (1075), **NST** = `common/settings/nested_settings.cpp` (198),
  **PRM** = `include/settings/parameters.h` (868)
- **B** = `pcbnew/board.cpp` (4428), **P** = `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (10757)
- IU = internal unit = 1 nm (`pcbIUScale`); "oracle" = reference `kicad-cli`
  (`/home/user/kicad-oracle/bin/kicad-cli`, wxWidgets 3.2.4, i.e. `wxRegEx` = PCRE2).

Read sections 0, 1, 4 and 11 before touching any work package. Sections 2, 3, 5 are lookup tables,
section 10 is the work breakdown. Sibling documents: `pcb-io.md` (board file reader/writer; it owns
the `(setup ...)` grammar) and `drc.md` (DRC engine; it owns error codes, exclusions JSON and the
rule file grammar). Where this document disagrees with them about settings semantics, this
document is the one that was checked against the settings code (see 11.1 for two corrections).

---------------------------------------------------------------------------------------------------

## 0. Seven facts that are easy to get wrong

1. **A key missing from `board.design_settings` does NOT fall back to the parameter default.**
   `BOARD_DESIGN_SETTINGS` sets `m_resetParamsIfMissing = false` (BDS:52). A missing key leaves the
   member as it is: the constructor value, or the value the board parser stored before the project
   was bound. The parameter's own default is used only when the value in the file is out of range.
   Exception: `PARAM_LAMBDA` parameters ignore the flag and always call their setter (4.2).
2. **"No `.kicad_pro`" means two different things.** `kicad-cli` always creates a `PROJECT` (with
   an empty, all-default `PROJECT_FILE`) and binds the board to it. The C++ QA helper
   `KI_TEST::LoadBoard` binds a project only when a `.kicad_pro`/`.pro` exists; otherwise the board
   has **no project**, and then net classes are never synchronised onto nets (1.4).
3. **Two legacy flags short-circuit the project file.** A board whose `(setup ...)` contains any
   pre-V6 design-rule token sets `m_LegacyDesignSettingsLoaded`; then `board.design_settings` is
   **not read at all** (B:398). A board with `(net_class ...)` sections sets
   `m_LegacyNetclassesLoaded`; then the board's net classes and patterns **replace** the project's
   (B:403-412).
4. **The effective net class of a net is usually a composite**, even with a single assignment: any
   parameter the assigned class lacks (including the schematic-only `wire_width` / `bus_width`) is
   filled from the Default class and the result is a new class named `"<Class>,Default"`
   (NS:1141-1165, NS:1318-1409).
5. **Net class patterns are matched twice**: as a PCRE2 regular expression anchored with `^...$`
   and as an anchored wildcard (`*`, `?`). A net matches if either does. `+3V3` fails to compile as
   a regex in PCRE2 and therefore matches literally only; `Net-(D1-A)` as a regex also matches the
   net `Net-D1-A` (4.6).
6. **`.kicad_prl` is not irrelevant.** `board.visible_layers` / `board.visible_items` feed
   `BOARD::IsLayerVisible`, `GetVisibleLayers`, `IsElementVisible`, which reach DRC (text mirroring
   provider), `BOARD::ComputeBoundingBox` and `FOOTPRINT::GetBoundingBox` (3.11).
7. **The rule file is `<board base name>.kicad_dru` next to the board**, independent of the
   project's name (4.15).

---------------------------------------------------------------------------------------------------

## 1. Purpose and entry points

### 1.1 What the subsystem provides

For a loaded board: the design-rule minima and DRC severities (`BOARD_DESIGN_SETTINGS`), the net
classes and the net -> effective-net-class resolution (`NET_SETTINGS`, `NETCLASS`), the net list
(`NETINFO_LIST`), the stackup model, component classes, tuning profiles, and text-variable
resolution. Consumers are DRC (all rule minima become implicit rules, severities filter
violations), zone fill (clearances, `m_MaxError`, zone defaults), plotting/Gerber (`m_MaxError`,
solder mask/paste margins, via tenting, aux origin, text variables), drill files (aux origin,
stackup for backdrill depth), gbrjob (stackup, board thickness) and the router (track/via size
lists, net class sizes).

### 1.2 Callers

There is no dedicated kicad-cli command for settings; **every** `kicad-cli pcb ...` job goes
through it:

| Step | Code | What happens |
|---|---|---|
| CLI command | `kicad/cli/command_pcb_*.cpp` (`doPerform`) fills a `JOB_*` (`common/jobs/`) | shared options: `-D/--define-var KEY=VALUE` (repeatable; exactly one `=`, otherwise exit `ERR_ARGS`; `kicad/cli/command.cpp:80-98`), `--variant`, `--drawing-sheet` |
| Job dispatch | `PCBNEW_JOBS_HANDLER::Job*` (`pcbnew/pcbnew_jobs_handler.cpp`) | each calls `getBoard( path )` |
| Project | `getBoard` (L540-630) -> `getProjectForBoard`: board path with extension `kicad_pro`, made absolute -> `SETTINGS_MANAGER::LoadProject( path, true )` (`common/settings/settings_manager.cpp:1020-1134`) -> `loadProjectFile` (L1420-1439) -> `PROJECT_FILE::LoadFromFile( projectDir )`; then `PROJECT_LOCAL_SETTINGS` (`<name>.kicad_prl`) is registered and loaded | the `PROJECT` object exists even when no file exists |
| Board | `BOARD_LOADER::Load` (`pcbnew/board_loader.cpp:43-88`) -> parser -> `initializeLoadedBoard` (L99-170) | binds project, creates DRC engine, builds nets (1.3) |
| Text var overrides | `resolveJobOutputPath` (L3698-3713): `project->ApplyTextVars( job var overrides )` then `board->SynchronizeProperties()` | `-D` values overwrite/extend project text variables |
| DRC job | `JobExportDrc` (L2852): `drcEngine->InitEngine( brd->GetDesignRulesPath() )` (L2907) | second rule-file load, same path in practice (4.15) |
| Stackup job | `kicad-cli pcb export stackup INPUT [-o OUT] [--format csv|json] [--units mm|in] [--exclude-color|-material|-thickness|-epsilon-r|-loss-tangent|-finish|-board-options]` -> `JOB_EXPORT_PCB_STACKUP` -> `JobExportStackup` (L2106-2211) | defaults: format `csv`, units `mm`, everything included. CSV = `BuildStackupCsv` (`board_stackup_reporter.cpp:193-297`) on a copy of the stackup after `SynchronizeWithBoard`. The only CLI output produced directly by this subsystem (tier 3) |
| QA | `KI_TEST::LoadBoard` (`qa/pcbnew_utils/board_test_utils.cpp:90-181`) | different sequence, see 1.4 |
| GUI | `PCB_EDIT_FRAME::OpenProjectFiles` (`pcbnew/files.cpp`), setup dialogs, panels | OUT of scope. Note the GUI does extra work the CLI does not: `inferLegacyEdgeClearance` for legacy boards without `(defaults (edge_clearance))` and moving legacy visible layers into the local settings (`files.cpp:825-848`) |

kicad-cli never writes the `.kicad_pro`: `SETTINGS_MANAGER::Save()` skips `PROJECT_FILE`
(`managerMayAutoSave`, `settings_manager.cpp:165`), and projects are unloaded with `aSave = false`
(`kicad/kicad_cli.cpp:705-708`). It **may** write `<name>.kicad_prl` at exit (when that file was
migrated or modified). Differential tests must therefore run the oracle on a copy of the data.

### 1.3 Load sequence of the CLI, in execution order

This is the sequence the port's "load board with project" entry point must reproduce.

1. `PROJECT` is created: full name = `<board dir>/<board base name>.kicad_pro`; `GetProjectName()`
   = board base name; `GetProjectPath()` = board dir with trailing separator.
2. `PROJECT_FILE` constructor (PF:43-325): registers its parameters, creates the three nested
   settings it owns - `NET_SETTINGS( this, "net_settings" )`,
   `COMPONENT_CLASS_SETTINGS( this, "component_class_settings" )`,
   `TUNING_PROFILES( this, "tuning_profiles" )` - none of them loads yet (`aLoadFromFile = false`).
3. `PROJECT_FILE::LoadFromFile` -> `JSON_SETTINGS::LoadFromFile` (JS:162-345): clear JSON, load
   every parameter (defaults), read + parse the file if it exists, run root migrations, load every
   parameter again, then `LoadFromFile()` of each nested settings object in registration order
   (4.1-4.3). File missing: if `<name>.pro` exists the legacy INI migration runs (7.4), else all
   defaults and return `false` (ignored by the caller).
4. `.kicad_prl` is loaded the same way (3.11).
5. The board file is parsed. The parser writes into the board's own `BOARD_DESIGN_SETTINGS`
   (created by the `BOARD` constructor with parent `nullptr`) and into that object's private
   `NET_SETTINGS( nullptr, "" )` (4.11). It also builds the net list (4.10).
6. `initializeLoadedBoard` (`board_loader.cpp:99-170`), in this order:
   1. drawing sheet load (not this subsystem), layer enum map for the property system;
   2. `BOARD::SetProject( project )` (B:379-417, see 4.9): registers the board's BDS as nested
      settings of the project at path `board.design_settings` and - unless
      `m_LegacyDesignSettingsLoaded` - runs `BOARD_DESIGN_SETTINGS::LoadFromFile` (4.4); moves
      legacy net classes into the project; points `bds.m_NetSettings` at the project's
      `NET_SETTINGS`;
   3. DRC engine created, `InitEngine( <board path with ext kicad_dru> )`, exceptions swallowed;
   4. `ResolveDRCExclusions( true )` and the resulting markers added (see `drc.md` 6.4);
   5. `BuildConnectivity()`;
   6. `BuildListOfNets()` (4.10): reset nets, `SynchronizeNetsAndNetClasses( false )`,
      `SetAreasNetCodesFromNetNames()`;
   7. `SynchronizeNetsAndNetClasses( true )` (again; additionally resets the custom track/via
      sizes from the Default class);
   8. `SynchronizeComponentClasses( {} )` (4.13);
   9. `SynchronizeTuningProfileProperties()` (tier 3);
   10. `UpdateUserUnits`.

### 1.4 The QA harness sequence and how it differs

`KI_TEST::LoadBoard( settingsManager, relPath, board )` (`board_test_utils.cpp:90-181`):

1. If `<path>.kicad_pro` exists: `LoadProject`; else if `<path>.pro` exists: `LoadProject` of that
   (legacy migration); else **no project is loaded**.
2. `ReadBoardFromFileOrStream` = bare parser run.
3. `SetProject( &Prj() )` **only if** one of the two project files exists.
4. DRC engine: `InitEngine( <path>.kicad_dru )` if that file exists, else `InitEngine( wxFileName() )`.
5. `BuildListOfNets()`, then `BuildConnectivity()` (reverse order compared with the CLI).
6. `SynchronizeTuningProfileProperties()`; `SynchronizeComponentClasses( {} )` if a project is set.

| Aspect | CLI (`BOARD_LOADER`) | QA `LoadBoard` |
|---|---|---|
| project when no `.kicad_pro`/`.pro` | empty default project, board bound to it | none (`GetProject() == nullptr`) |
| `SynchronizeNetsAndNetClasses` | runs (twice) | returns immediately without project (B:3242): every net keeps the class assigned by `NETINFO_ITEM::Clear()`, i.e. the current Default class - legacy `(net_class ...)` assignments are **not** applied |
| `IsLayerVisible` / `IsElementVisible` | from `.kicad_prl` (all visible when absent) | always visible without project |
| `ResolveDRCExclusions` | yes | no |
| custom track/via reset (`aResetTrackAndViaSizes`) | yes | no |
| text variables `PROJECTNAME`, project vars | available | unresolved without project |
| `KICAD_INHIBIT_SETTINGS_WRITES` | not set | set by the test runner (no write-back, JS:321) |

When porting a C++ test, reproduce the harness sequence; when comparing with the oracle,
reproduce the CLI sequence. Provide both as explicit entry points (10.1).

### 1.5 Options, environment and constants that parameterise the subsystem

| Input | Default | Where |
|---|---|---|
| `-D/--define-var` | none | `kicad/cli/command.cpp:80-98`; applied by `PROJECT::ApplyTextVars` (`common/project.cpp:229-241`) |
| `--variant` | empty = default variant | `BOARD::SetCurrentVariant`; only matters here for `${VARIANT}` / `${VARIANT_DESC}` |
| `ADVANCED_CFG::m_DRCEpsilon` | 0.0005 mm -> `GetDRCEpsilon()` = 500 IU | `common/advanced_config.cpp:257`, BDS:2054 |
| `ADVANCED_CFG::m_HoleWallThickness` | 0.020 mm -> `GetHolePlatingThickness()` = 20000 IU | `advanced_config.cpp:263`, BDS:2060 |
| `ADVANCED_CFG::m_ResolveTextRecursionDepth` | 6 | `advanced_config.cpp:326`; text variable expansion depth |
| `MAXIMUM_CLEARANCE` | `mmToIU( 500 )` = 500 000 000 IU | `include/board_design_settings_defaults.h` |
| current date/time, VCS hash | environment | `${CURRENT_DATE}` etc.; the port takes them as explicit parameters (PORTING.md section 5) |

The advanced-config values come from a user file (`kicad_advanced`) in C++; the port uses the
compiled-in defaults above as constants (overridable through an options struct).

---------------------------------------------------------------------------------------------------

## 2. File inventory

### 2.1 In scope

| File | Lines | Role |
|---|---:|---|
| `include/settings/json_settings.h`, `common/settings/json_settings.cpp` | 469 / 1075 | JSON document, dotted-path access, parameter list, migrations. Port: `Load`, `LoadFromBuffer`-like entry, `parseAndMigrate`, `Migrate`, `Get<T>`, `Set`, `Contains`, `At`. Skip file I/O, `SaveToFile`, legacy `wxConfig` helpers |
| `include/settings/json_settings_internals.h` | 82 | `PointerFromString` ("a.b.c" -> `/a/b/c`), `m_original` copy |
| `include/settings/nested_settings.h`, `common/settings/nested_settings.cpp` | 69 / 198 | settings object stored inside a parent document at a path; own schema version and migrations |
| `include/settings/parameters.h`, `common/settings/parameters.cpp` | 868 / 137 | `PARAM`, `PARAM_ENUM`, `PARAM_LAMBDA`, `PARAM_SCALED`, `PARAM_LIST`, `PARAM_SET`, `PARAM_MAP`, `PARAM_WXSTRING_MAP` load semantics (4.2) |
| `include/project/project_file.h`, `common/project/project_file.cpp` | 316 / 1039 | `.kicad_pro` root object. In scope: `text_variables`, `boards`, nested settings, root migrations (no-ops for the port), `LoadFromFile`. Legacy `.pro` migration L379-819 is tier 3 |
| `include/project.h`, `common/project.cpp` | 419 / 603 | `PROJECT`: name/path (L244-298), `TextVarResolver` (L178-220), `GetTextVars`, `ApplyTextVars`, `AbsolutePath` (L501-517). Rest (git, lib tables, elems) out |
| `include/project/net_settings.h`, `common/project/net_settings.cpp` | 428 / 1906 | net classes, assignments, patterns, effective net class, bus-pattern expansion, migrations 0..5 |
| `include/netclass.h`, `common/netclass.cpp` | 326 / 435 | `NETCLASS` (protobuf `Serialize`/`Deserialize` L160-310 out) |
| `include/eda_pattern_match.h`, `common/eda_pattern_match.cpp` | 257 / 530 | only `EDA_PATTERN_MATCH_REGEX(_ANCHORED)`, `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`, `EDA_COMBINED_MATCHER` with `CTX_NETCLASS` (L77-116, L125-151, L203-241, L386-473) |
| `include/board_design_settings.h`, `pcbnew/board_design_settings.cpp`, `include/board_design_settings_defaults.h` | 804 / 2093 / ~100 | design settings, JSON keys, migrations, severities, size lists |
| `pcbnew/netinfo.h`, `pcbnew/netinfo_item.cpp`, `pcbnew/netinfo_list.cpp` | 422 / 237 / 389 | nets (may be owned by the board-model package, see 10.2) |
| `pcbnew/board.cpp` (parts) | - | ctor defaults L99-178; `SetProject`/`ClearProject`/`GetDesignRulesPath` L379-445; `ResolveTextVar` L650-795; visibility L1180-1245; `GetMaxClearanceValue` L1325-1354; `FindNet`/`FindOrAddNet`/`MatchDpSuffix`/`DpCoupledNet` L2805-2895; `SynchronizeProperties` L2968; `ApplyNetChainNetclasses`, `SynchronizeNetsAndNetClasses`, `SynchronizeComponentClasses`, `SetAreasNetCodesFromNetNames` L3200-3325; `GetStackupOrDefault` L3478 |
| `pcbnew/board_loader.cpp` | 215 | post-load glue (1.3) |
| `pcbnew/board_stackup_manager/board_stackup.h/.cpp`, `stackup_predefined_prms.h` | 378 / 1050 / 150 | stackup model, default stackup, layer distance, drill depth; `FormatBoardStackup` (the `(stackup ...)` writer, L821-915) belongs to the pcb-io writer package; protobuf L471-596 out |
| `pcbnew/board_stackup_manager/board_stackup_reporter.cpp/.h` | 297 / 51 | text + CSV report (tier 3, `pcb export stackup`) |
| `pcbnew/component_classes/*` | 520+100+106+153+38+60+49+72 | component class manager, classes, assignment rules, cache proxy (tier 2) |
| `include/project/component_class_settings.h`, `common/project/component_class_settings.cpp` | 171 / 393 | JSON for assignment rules + conversion to a DRC-language rule (tier 2) |
| `include/project/tuning_profiles.h`, `common/project/tuning_profiles.cpp` | 235 / 270 | tuning profiles (tier 3) |
| `include/project/project_local_settings.h`, `common/project/project_local_settings.cpp`, `common/settings/layer_settings_utils.cpp` | 175 / 662 / 151 | only `board.visible_layers`, `board.visible_items` and the `visible_items` migrations (tier 2) |
| `common/common.cpp` (L60-360) | - | `ExpandTextVars`, `ResolveTextVars`, `FinalizeTextVarExpansion` |
| `common/title_block.cpp` (L72-200) | - | `TITLE_BLOCK::TextVarResolver`, `GetContextualTextVars` |
| `pcbnew/teardrop/teardrop_parameters.h/.cpp` | ~160 / 58 | parameter defaults and canonical target names (data only) |
| `pcbnew/zone_settings.h/.cpp` (ctor), `pcbnew/zones.h` | - | `ZONE_SETTINGS` defaults used by `defaults.zones.*` |

### 2.2 Out of scope (record in `PORT_STATUS.md`)

| File / class | Why |
|---|---|
| `common/settings/settings_manager.cpp` (1868) | process-wide registry, locks, backups, color themes, migration of user config. The port has no global state: the caller passes file contents |
| `JSON_SETTINGS::SaveToFile`, `formatFileContents`, `NESTED_SETTINGS::SaveToFile`, `Store` | writing `.kicad_pro` (tier 3 if ever; 6.1 documents the format) |
| `common/project/project_file_downgrade.cpp` (1162), `project_archiver.cpp` (398) | save-as-older-version, zip archive |
| `include/project/board_project_settings.h`, `board_project_settings.cpp`, `board_project_settings_params.h` | layer presets, viewports, layer pairs: GUI state |
| `PROJECT_FILE` parameters `sheets`, `schematic.*`, `libraries.*`, `cvpcb.*`, `pcbnew.last_paths.*`, `board.layer_presets`, `board.viewports`, `board.3dviewports`, `board.layer_pairs`, `board.ipc2581.*`, `board.idf_export.*`, `pcbnew.page_layout_descr_file` (only the drawing sheet path; plotting may want it later) | GUI / schematic / other exporters |
| BOM members of BDS (`bom_export_filename`, `bom_settings`, `bom_presets`, `bom_fmt_settings`, `bom_fmt_presets`; base class `FIELDS_TABLE_BOM_SETTINGS`) | BOM export |
| `BOARD_DESIGN_SETTINGS::ValidateDesignRules`, `GetDesignRuleWarnings` (BDS:1616-1776) | setup dialog validation (translated messages) |
| `pcbnew/board_stackup_manager/panel_*`, `dialog_*`, `dielectric_material.*`, `*.fbp` | wx dialogs |
| `NETINFO_ITEM::GetMsgPanelInfo`, `GetBoundingBox`, `NETINFO_LIST::RebuildDisplayNetnames` | GUI (display net names are only used by the painter) |
| `PROJECT_LOCAL_SETTINGS` everything except the two visibility keys | GUI state |
| `NETCLASS::Serialize/Deserialize`, `BOARD_STACKUP::Serialize/Deserialize` | protobuf API |
| `common/text_eval/*` (4314 lines, `@{...}` math in text) | tier 3; `ExpandTextVars` must still pass `@{...}` through unchanged (4.14) |
| `KIGFX::COLOR4D` JSON conversion for `pcb_color`, `schematic_color`, `net_colors` | colours do not reach any in-scope output; keep the raw strings only if a writer is added |

---------------------------------------------------------------------------------------------------

## 3. Data structures

### 3.1 JSON settings mechanics

`JSON_SETTINGS` owns one `nlohmann::json` document (`m_internals`), a list of parameters
(`m_params`, in registration order), a schema version and a map `old version -> ( new version,
migrator )`. A parameter binds a dotted path to a member through a pointer or a getter/setter
pair. `NESTED_SETTINGS` is a `JSON_SETTINGS` whose document is a copy of the sub-object found at
`m_path` in the parent's document.

Lifetimes: `PROJECT_FILE` owns `NET_SETTINGS`, `COMPONENT_CLASS_SETTINGS`, `TUNING_PROFILES`
(`shared_ptr`). The `BOARD` owns its `BOARD_DESIGN_SETTINGS` (`m_designSettings`, `shared_ptr`);
`PROJECT_FILE::m_BoardSettings` is a second reference set in `SetProject`. `bds.m_NetSettings` is
a `shared_ptr<NET_SETTINGS>`: the private instance until `SetProject`, the project's afterwards.

Port recommendation: no parameter objects and no back-pointers. Each settings struct gets a
`load_from_json( &serde_json::Value ) ` function written as a straight list of "read key X with
rule R into field F" calls in **registration order**, using four small helpers that implement the
rules of 4.2. Keep the document (`serde_json::Value`) only for the duration of the load (and for
`m_original` if the writer is ever ported).

### 3.2 `PROJECT` and `PROJECT_FILE`

`PROJECT` (in the port: plain data): full file name, derived `project_name()` (file base name),
`project_path()` (directory + separator), `is_null_project()` (empty name), the `PROJECT_FILE`,
the local settings. `PROJECT_FILE` fields the port needs: `m_TextVars: std::map<wxString,
wxString>` (-> `BTreeMap<String, String>`), `m_boards` (list of `(KIID, name)`; unused by the
flows in scope), `m_NetSettings`, `m_ComponentClassSettings`, `m_tuningProfileParameters`,
`m_BoardSettings`. Root schema version: 4 (`projectFileSchemaVersion`, PF:40).

### 3.3 `NETCLASS` (`include/netclass.h`, `common/netclass.cpp`)

Fields (all sizes in IU, `std::optional<int>` -> `Option<i32>`; getters return `-1` when unset
except `GetLineStyle()` which returns `0`):

| Member | JSON key (`net_settings.classes[]`) | Value when `aInitWithDefaults` (netclass.cpp:35-49) |
|---|---|---|
| `m_Name` | `name` | ctor argument |
| `m_Priority` | `priority` | **-1** (ctor; see below) |
| `m_Description` | (not stored) | empty |
| `m_Clearance` | `clearance` (mm) | 0.2 mm = 200000 |
| `m_TrackWidth` | `track_width` | 0.2 mm = 200000 |
| `m_ViaDia` | `via_diameter` | 0.6 mm = 600000 |
| `m_ViaDrill` | `via_drill` | 0.3 mm = 300000 |
| `m_uViaDia` | `microvia_diameter` | 0.3 mm = 300000 |
| `m_uViaDrill` | `microvia_drill` | 0.1 mm = 100000 |
| `m_diffPairWidth` | `diff_pair_width` | 0.2 mm = 200000 |
| `m_diffPairGap` | `diff_pair_gap` | 0.25 mm = 250000 |
| `m_diffPairViaGap` | `diff_pair_via_gap` | 0.25 mm = 250000 |
| `m_wireWidth` | `wire_width` (mils, schematic IU) | 6 mils = 1524 (sch IU = 100 nm) |
| `m_busWidth` | `bus_width` (mils) | 12 mils = 3048 |
| `m_lineStyle` | `line_style` (int) | 0 |
| `m_pcbColor`, `m_schematicColor` | `pcb_color`, `schematic_color` (strings) | `COLOR4D::UNSPECIFIED` always |
| `m_tuningProfile` | `tuning_profile` | empty (`HasTuningProfile()` = non-empty) |
| `m_isDefault` | - | set by `SetName` when the name equals `"Default"` (case sensitive) |

Without `aInitWithDefaults` every optional is unset. Besides the values, each parameter has a
"parent" pointer (`m_clearanceParent`, ...): the root net class the value came from; a root class
is its own parent (`ResetParents`, L85-102). `m_constituents: vector<NETCLASS*>`: `[this]` for a
root class, the sorted constituents for a composite; `m_constituentOwners` keeps them alive.

Priorities in the wild: `NET_SETTINGS` ctor Default = `INT_MAX` (NS:69); implicit classes =
`INT_MAX - 1` (NS:1025); classes read from JSON = the `priority` key, or -1 when the key is
absent; the Default class created by the `BOARD` ctor (B:160) and every class parsed from a
legacy board = **-1**. Lower number = higher priority.

Names (netclass.cpp:347-435):

- `GetName()`: root -> `m_Name`; composite -> constituents' `m_Name` joined with `","` in
  constituent order (e.g. `"HV,Power,Default"`).
- `NameEquals( s )`: same comparison without building the string.
- `GetHumanReadableName()`: root -> name; 2 -> `"%s and %s"`; 3 -> `"%s, %s and %s"`; more ->
  `"%s, %s and %d more"` (first two names, count - 2). Translated strings; the port uses the
  English originals.
- `ContainsNetclassWithName( s )`: any constituent whose `GetName()` matches the **wildcard
  pattern** `s` (`wxString::Matches`: `*` and `?`, case sensitive).
- `operator==` compares the constituent pointer vectors. `EqualsByPersistedFields` compares all
  JSON-persisted fields (not the description).
- `ResetParameters()` (L105-123) clears every optional and both colours but **not** the tuning
  profile.

Port: store net classes in an arena inside `NetSettings` (root, implicit and composite classes)
and refer to them by id; parents and constituents become ids. A net holds the id of its effective
class.

### 3.4 `NET_SETTINGS` (`include/project/net_settings.h:359-423`)

| Member | Type | Persisted | Meaning |
|---|---|---|---|
| `m_defaultNetClass` | `shared_ptr<NETCLASS>` | `classes[]` entry named `Default` | always non-null |
| `m_netClasses` | `std::map<wxString, shared_ptr<NETCLASS>>` | `classes[]` | user classes by name (Default not included) |
| `m_netClassLabelAssignments` | `std::map<wxString, std::set<wxString>>` | `netclass_assignments` | net name -> class names (from schematic labels) |
| `m_netClassPatternAssignments` | `vector<pair<unique_ptr<EDA_COMBINED_MATCHER>, wxString>>` | `netclass_patterns` | ordered (pattern, class name) |
| `m_netClassChainPatternAssignments` | `std::map<NET_CHAIN_SOURCE, vector<...>>` | no (derived) | chain-derived patterns per source (`SCHEMATIC` < `BOARD`) |
| `m_netChainClasses` | `std::map<wxString, wxString>` | `net_chain_classes` | chain -> chain class (DRC `inNetChainClass`) |
| `m_netChainNetClasses` | `std::map<wxString, wxString>` | `net_chain_netclasses` | chain -> net class applied to member nets |
| `m_netColorAssignments` | `std::map<wxString, COLOR4D>` | `net_colors` | GUI only |
| `m_compositeNetClasses` | `std::map<wxString, shared_ptr<NETCLASS>>` | no (cache) | composite name -> composite |
| `m_impicitNetClasses` | `std::map<wxString, shared_ptr<NETCLASS>>` | no | classes referenced by an assignment but not defined |
| `m_effectiveNetclassCache` | `std::map<wxString, shared_ptr<NETCLASS>>` | no (cache) | net name -> effective class |
| `m_netclassBusMembers` | `std::map<wxString, std::set<wxString>>` | no (cache) | bus name -> members consulted |

Schema version 5 (`netSettingsSchemaVersion`, NS:41). `ClearAllCaches()` clears the effective
cache, the composite map and the bus-member map but **not** the implicit classes.

### 3.5 `NETINFO_ITEM` / `NETINFO_LIST` (`pcbnew/netinfo.h`)

`NETINFO_ITEM`: `m_netCode` (0 = unconnected, -1 = orphaned), `m_netname` (full, escaped form as
stored in the file), `m_shortNetname` (= text after the last `/`, whole name when there is none),
`m_displayNetname` (`UnescapeString( short )`; GUI only), `m_netChain` (name, empty = none),
`m_terminalPads[2]` + `m_terminalPadUuids[2]`, `m_netClass` (`shared_ptr<NETCLASS>`; initialised
to the Default class of the board's net settings at construction time), `m_isCurrent`.

`NETINFO_LIST`: `m_netNames: std::map<wxString, NETINFO_ITEM*>` (owner, **iteration order of the
list = name order**), `m_netCodes: std::map<int, NETINFO_ITEM*>`, `m_newNetCode`. The constructor
always inserts net 0 with the empty name. Constants `UNCONNECTED = 0`, `ORPHANED = -1`.

### 3.6 `BOARD_DESIGN_SETTINGS`: members and constructor defaults (BDS:47-266)

"JSON" column: the key under `board.design_settings` (see 4.4 for load rules); "board" = written
by the board-file parser instead.

Design-rule minima:

| Member | Ctor default (IU) | mm | JSON |
|---|---:|---:|---|
| `m_MinClearance` | 0 | 0 | `rules.min_clearance` |
| `m_MinConn` | 0 | 0 | `rules.min_connection` |
| `m_TrackMinWidth` | 200000 | 0.2 | `rules.min_track_width` |
| `m_ViasMinAnnularWidth` | **100000** | 0.1 = `mmToIU( 0.5 - 0.3 ) / 2` | `rules.min_via_annular_width` |
| `m_ViasMinSize` | 500000 | 0.5 | `rules.min_via_diameter` |
| `m_MinThroughDrill` | 300000 | 0.3 | `rules.min_through_hole_diameter` |
| `m_MicroViasMinSize` | 200000 | 0.2 | `rules.min_microvia_diameter` |
| `m_MicroViasMinDrill` | 100000 | 0.1 | `rules.min_microvia_drill` |
| `m_CopperEdgeClearance` | 500000 | 0.5 | `rules.min_copper_edge_clearance` (legacy boards: `(defaults (edge_clearance ...))`) |
| `m_HoleClearance` | 250000 | 0.25 | `rules.min_hole_clearance` |
| `m_HoleToHoleMin` | 250000 | 0.25 | `rules.min_hole_to_hole` |
| `m_SilkClearance` | 0 | 0 | `rules.min_silk_clearance` |
| `m_MinGrooveWidth` | 0 | 0 | `rules.min_groove_width` |
| `m_MinResolvedSpokes` | 2 | (count) | `rules.min_resolved_spokes` |
| `m_MinSilkTextHeight` | 800000 | 0.8 = `1.0 * 0.8` | `rules.min_text_height` |
| `m_MinSilkTextThickness` | 80000 | 0.08 = `0.1 * 0.8` | `rules.min_text_thickness` |
| `m_MaxError` | 5000 | 0.005 (`ARC_HIGH_DEF`) | `rules.max_error` (legacy boards: `(max_error ...)`) |
| `m_UseHeightForLengthCalcs` | true | | `rules.use_height_for_length_calcs` |
| `m_ZoneKeepExternalFillets` | false | | `zones_allow_external_fillets` |

Mask / paste / via protection (all **board file**, `(setup ...)`; none is in the project file any
more, but see migration 1->2 in 4.4):

| Member | Ctor default | Board token |
|---|---|---|
| `m_SolderMaskExpansion` | 0 | `pad_to_mask_clearance` |
| `m_SolderMaskMinWidth` | 0 (and reset to 0 at the start of `parseSetup`, P:3044) | `solder_mask_min_width` |
| `m_SolderMaskToCopperClearance` | 0 | JSON `rules.solder_mask_to_copper_clearance` |
| `m_SolderPasteMargin` | 0 | `pad_to_paste_clearance` |
| `m_SolderPasteMarginRatio` | 0.0 | `pad_to_paste_clearance_ratio` |
| `m_AllowSoldermaskBridgesInFPs` | false | `allow_soldermask_bridges_in_footprints` |
| `m_TentViasFront`, `m_TentViasBack` | true, true | `tenting` (also legacy plot option, P:3423-3428) |
| `m_CoverViasFront/Back`, `m_PlugViasFront/Back`, `m_CapVias`, `m_FillVias` | false | `covering`, `plugging`, `capping`, `filling` |

Layer bookkeeping and board-file state:

| Member | Ctor default | Notes |
|---|---|---|
| `m_enabledLayers` | all bits set, then `SetCopperLayerCount( 2 )`, `SetUserDefinedLayerCount( 4 )` | `SetCopperLayerCount( n )`: clear copper bits, OR `LSET::AllCuMask( n )` (n > 0). `SetUserDefinedLayerCount( n )`: clear user-defined bits, OR `LSET::UserDefinedLayersMask( n )`. `SetEnabledLayers( mask )`: store mask, force `B_Cu, F_Cu, B_CrtYd, F_CrtYd, Edge_Cuts, Margin`, recount copper and user-defined layers **from the argument mask** (BDS:2013-2032) |
| `m_copperLayerCount`, `m_userDefinedLayerCount` | 2, 4 | |
| `m_boardThickness` | 1600000 (1.6 mm) | board `(general (thickness ...))` |
| `m_HasStackup` | false | set by `parseBoardStackup` |
| `m_stackup` | empty `BOARD_STACKUP` | 3.8 |
| `m_auxOrigin`, `m_gridOrigin` | (0, 0) | board `aux_axis_origin`, `grid_origin` |
| `m_drillSymbolProfile` | default | board `drill_symbol_profile` (pcb-io) |
| `m_ZoneLayerProperties` | empty `std::map<PCB_LAYER_ID, ZONE_LAYER_PROPERTIES>` | board `zone_defaults` |
| `m_NetSettings` | new `NET_SETTINGS( nullptr, "" )` | replaced in `SetProject` |
| `m_DRCEngine` | null | set by the loader |
| `m_DrcExclusions` | empty set | `drc_exclusions` |
| `m_DRCSeverities` | 3.7 | `rule_severities` |

Interactive routing state (tier 3 consumers, cheap to carry):

| Member | Ctor default |
|---|---|
| `m_TrackWidthList`, `m_ViasDimensionsList`, `m_DiffPairDimensionsList`, `m_ViaStackPresets` | empty (index 0 of the first three is the "use net class" placeholder once populated) |
| `m_trackWidthIndex`, `m_viaSizeIndex`, `m_diffPairIndex`, `m_viaStackIndex` | 0 |
| `m_useCustomTrackVia`, `m_useCustomDiffPair` | false |
| `m_customTrackWidth` | 200000 |
| `m_customViaSize` | diameter 500000, drill 300000 |
| `m_customDiffPair` | width 125000, gap 180000, via gap 180000 |
| `m_CurrentViaType` | `VIATYPE::THROUGH` |
| `m_UseConnectedTrackWidth`, `m_TempOverrideTrackWidth` | false |
| `m_currentNetClassName` | empty |
| `m_SingleTrackMeanderSettings`, `m_SkewMeanderSettings` | `PNS::MEANDER_SETTINGS()` with `m_spacing` = 600000 |
| `m_DiffPairMeanderSettings` | same with `m_spacing` = 1000000 |
| `PNS::MEANDER_SETTINGS()` defaults (`pcbnew/router/pns_meander.cpp:40-64`) | min amplitude 200000, max amplitude 1000000, step 50000, spacing 600000, corner style ROUND, corner radius 80 %, single sided false |

Defaults for new items (`m_LineThickness[]`, `m_TextSize[]`, `m_TextThickness[]`,
`m_TextItalic[]`, `m_TextUpright[]`, indexed by layer class; `GetLayerClass`, BDS:2037-2051:
`F/B_SilkS` -> SILK, copper -> COPPER, `Edge_Cuts` -> EDGES, `F/B_CrtYd` -> COURTYARD,
`F/B_Fab` -> FAB, else OTHERS):

| Layer class | Line width | Text size (x = y) | Text thickness | Italic | Upright (ctor) |
|---|---:|---:|---:|---|---|
| `LAYER_CLASS_SILK` (0) | 100000 | 1000000 | 100000 | false | false |
| `LAYER_CLASS_COPPER` (1) | 200000 | 1500000 | 300000 | false | false |
| `LAYER_CLASS_EDGES` (2) | 50000 | 1000000 | 150000 | false | false |
| `LAYER_CLASS_COURTYARD` (3) | 50000 | 1000000 | 150000 | false | false |
| `LAYER_CLASS_FAB` (4) | 100000 | 1000000 | 150000 | false | false |
| `LAYER_CLASS_OTHERS` (5) | 100000 | 1000000 | 150000 | false | false |

Other defaults: `m_DefaultFPTextItems` = `("REF**", visible, F_SilkS)`, `("", visible, F_Fab)`,
`("${REFERENCE}", visible, F_Fab)`; `m_StyleFPFields/Text/Shapes/Dimensions/Barcodes` = false;
`m_DimensionPrecision` = `X_XXXX` (4), `m_DimensionUnitsMode` = `AUTOMATIC` (3),
`m_DimensionUnitsFormat` = `NO_SUFFIX` (0), `m_DimensionSuppressZeroes` = true,
`m_DimensionTextPosition` = `OUTSIDE` (0), `m_DimensionKeepTextAligned` = true,
`m_DimensionArrowLength` = `MilsToIU( 50 )` = 1270000, `m_DimensionExtensionOffset` = 500000.

`m_defaultZoneSettings` = `ZONE_SETTINGS()` (`pcbnew/zone_settings.cpp`, ctor): clearance 500000,
min thickness 250000, fill mode POLYGONS, hatch thickness 1000000, hatch gap 1500000, hatch
orientation 0, smoothing level 0, smoothing value 0.1, hole min area 0.15, border algorithm 1,
border display `DIAGONAL_EDGE`, border hatch pitch 500000, thermal gap 500000, thermal spoke
width 500000, pad connection `THERMAL`, corner smoothing none, corner radius 0, island removal
`ALWAYS`, min island area `10 * 1e6 * 1e6` IU^2, priority 0, layers `{F_Cu}`, keepout flags
(vias, tracks, pads = true; fills, footprints = false).

`m_TeardropParamsList` (`pcbnew/teardrop/teardrop_parameters.h`): targets `m_TargetVias` = true,
`m_TargetPTHPads` = true, `m_TargetSMDPads` = true, `m_TargetTrack2Track` = false,
`m_UseRoundShapesOnly` = false; three `TEARDROP_PARAMETERS` (index 0 `td_round_shape`, 1
`td_rect_shape`, 2 `td_track_end`), each: max length 1000000, max width 2000000, best length
ratio 0.5, best width ratio 1.0, width-to-size filter ratio 0.9, curved edges false, enabled
false, allow two tracks true, on pads in zones false.

Trap: `initFromOther` (copy ctor / assignment, BDS:1210-1314) does **not** copy
`m_TeardropParamsList`, the three meander settings, `m_drillSymbolProfile`,
`m_ZoneLayerProperties` and `m_DRCEngine` (everything else, including `m_ViaStackPresets` and the
`m_NetSettings` pointer, is copied). `operator==` (BDS:1317-1428) compares the same subset. If
`BOARD::SetDesignSettings` or equality is ported, write them by hand with the same member list
instead of `#[derive]`.

### 3.7 DRC severities: `m_DRCSeverities` defaults (BDS:169-219)

`std::map<int, SEVERITY>` with one entry for **every** code `DRCE_FIRST ..= DRCE_LAST` (1..=78),
`RPT_SEVERITY_ERROR` unless listed otherwise. Severity strings in JSON: `"error"`, `"warning"`,
`"ignore"`; `SeverityFromString` maps `"warning"` and `"ignore"` and treats **anything else as
error** (`common/widgets/ui_common.cpp:58-66`). `GetSeverity( code )` returns `ERROR` for a code
not in the map (BDS:1597-1607).

"Kind": `json` = listed in `DRC_ITEM::GetItemsWithSeverities()` (everything in
`DRC_ITEM::allItemTypes` before `heading_internal`, `pcbnew/drc/drc_item.h:172-188`), i.e. loaded
from `rule_severities.<key>`; `fixed` = has a key but is never loaded from JSON (stays at the
constructor value); `no item` = enum value without a `DRC_ITEM`.

| # | `DRCE_...` | settings key | default | kind |
|---:|---|---|---|---|
| 1 | `UNCONNECTED_ITEMS` | `unconnected_items` | error | json |
| 2 | `SHORTING_ITEMS` | `shorting_items` | error | json |
| 3 | `ALLOWED_ITEMS` | `items_not_allowed` | error | json |
| 4 | `TEXT_ON_EDGECUTS` | `text_on_edge_cuts` | error | json |
| 5 | `CLEARANCE` | `clearance` | error | json |
| 6 | `CREEPAGE` | `creepage` | error | json |
| 7 | `TRACKS_CROSSING` | `tracks_crossing` | error | json |
| 8 | `EDGE_CLEARANCE` | `copper_edge_clearance` | error | json |
| 9 | `ZONES_INTERSECT` | (none) | error | no item |
| 10 | `ISOLATED_COPPER` | `isolated_copper` | warning | json |
| 11 | `STARVED_THERMAL` | `starved_thermal` | error | json |
| 12 | `DANGLING_VIA` | `via_dangling` | warning | json |
| 13 | `DANGLING_TRACK` | `track_dangling` | warning | json |
| 14 | `DRILLED_HOLES_TOO_CLOSE` | `hole_to_hole` (legacy key `hole_near_hole`) | warning | json |
| 15 | `DRILLED_HOLES_COLOCATED` | `holes_co_located` | warning | json |
| 16 | `HOLE_CLEARANCE` | `hole_clearance` | error | json |
| 17 | `TRACK_WIDTH` | `track_width` | error | json |
| 18 | `TRACK_ANGLE` | `track_angle` | error | json |
| 19 | `TRACK_SEGMENT_LENGTH` | `track_segment_length` | error | json |
| 20 | `ANNULAR_WIDTH` | `annular_width` | error | json |
| 21 | `CONNECTION_WIDTH` | `connection_width` | warning | json |
| 22 | `DRILL_OUT_OF_RANGE` | `drill_out_of_range` | error | json |
| 23 | `VIA_DIAMETER` | `via_diameter` | error | json |
| 24 | `PADSTACK` | `padstack` | warning | json |
| 25 | `PADSTACK_INVALID` | `padstack_invalid` | error | fixed |
| 26 | `MICROVIA_DRILL_OUT_OF_RANGE` | `microvia_drill_out_of_range` | error | json |
| 27 | `MALFORMED_MICROVIA_STACK_SPAN` | `malformed_microvia_stack_span` | error | json |
| 28 | `MICROVIA_STACK_NOT_FILLED` | `microvia_stack_not_filled` | warning | json |
| 29 | `MICROVIA_STACK_DEPTH` | `microvia_stack_depth` | error | json |
| 30 | `HOLE_ASPECT_RATIO` | `hole_aspect_ratio` | error | json |
| 31 | `MICROVIA_CROSSES_CORE` | `microvia_crosses_core` | warning | json |
| 32 | `OVERLAPPING_FOOTPRINTS` | `courtyards_overlap` | error | json |
| 33 | `MISSING_COURTYARD` | `missing_courtyard` | ignore | json |
| 34 | `MALFORMED_COURTYARD` | `malformed_courtyard` | error | json |
| 35 | `PTH_IN_COURTYARD` | `pth_inside_courtyard` | error | json |
| 36 | `NPTH_IN_COURTYARD` | `npth_inside_courtyard` | error | json |
| 37 | `DISABLED_LAYER_ITEM` | `item_on_disabled_layer` | error | json |
| 38 | `INVALID_OUTLINE` | `invalid_outline` | error | json |
| 39 | `MISSING_FOOTPRINT` | `missing_footprint` | warning | json |
| 40 | `DUPLICATE_FOOTPRINT` | `duplicate_footprints` | warning | json |
| 41 | `EXTRA_FOOTPRINT` | `extra_footprint` | warning | json |
| 42 | `NET_CONFLICT` | `net_conflict` | warning | json |
| 43 | `SCHEMATIC_PARITY` | `footprint_symbol_mismatch` | warning | json |
| 44 | `FOOTPRINT_FILTERS` | `footprint_filters_mismatch` | ignore | json |
| 45 | `FOOTPRINT_TYPE_MISMATCH` | `footprint_type_mismatch` | ignore | json |
| 46 | `LIB_FOOTPRINT_ISSUES` | `lib_footprint_issues` | warning | json |
| 47 | `LIB_FOOTPRINT_MISMATCH` | `lib_footprint_mismatch` | warning | json |
| 48 | `PAD_TH_WITH_NO_HOLE` | `through_hole_pad_without_hole` | error | json |
| 49 | `FOOTPRINT` | `footprint` | error | json |
| 50 | `FOOTPRINT_SCALED_WITH_PADS` | `footprint_scaled_with_pads` | warning | json |
| 51 | `UNRESOLVED_VARIABLE` | `unresolved_variable` | error | json |
| 52 | `ASSERTION_FAILURE` | `assertion_failure` | error | json |
| 53 | `GENERIC_WARNING` | `generic_warning` | error | fixed |
| 54 | `GENERIC_ERROR` | `generic_error` | error | fixed |
| 55 | `COPPER_SLIVER` | `copper_sliver` | warning | json |
| 56 | `SOLDERMASK_BRIDGE` | `solder_mask_bridge` | error | json |
| 57 | `SILK_MASK_CLEARANCE` | `silk_over_copper` | warning | json |
| 58 | `SILK_EDGE_CLEARANCE` | `silk_edge_clearance` | warning | json |
| 59 | `SILK_CLEARANCE` | `silk_overlap` | warning | json |
| 60 | `TEXT_HEIGHT` | `text_height` | warning | json |
| 61 | `TEXT_THICKNESS` | `text_thickness` | warning | json |
| 62 | `LENGTH_OUT_OF_RANGE` | `length_out_of_range` | error | json |
| 63 | `NET_CHAIN_STUB_TOO_LONG` | `net_chain_stub_length` | error | json |
| 64 | `NET_CHAIN_RETURN_PATH_BREAK` | `net_chain_return_path` | error | json |
| 65 | `NET_CHAIN_TUNING_PROFILES` | `net_chain_tuning_profiles` | error | json |
| 66 | `SKEW_OUT_OF_RANGE` | `skew_out_of_range` | error | json |
| 67 | `VIA_COUNT_OUT_OF_RANGE` | `too_many_vias` | error | json |
| 68 | `DP_GAP_OUT_OF_RANGE` | `diff_pair_gap_out_of_range` | error | json |
| 69 | `DP_UNCOUPLED_LENGTH_TOO_LONG` | `diff_pair_uncoupled_length_too_long` | error | json |
| 70 | `MIRRORED_TEXT_ON_FRONT_LAYER` | `mirrored_text_on_front_layer` | warning | json |
| 71 | `UNMIRRORED_TEXT_ON_BACK_LAYER` | `nonmirrored_text_on_back_layer` | warning | json |
| 72 | `MISSING_TUNING_PROFILE` | `missing_tuning_profile` | warning | json |
| 73 | `TUNING_PROFILE_IMPLICIT_RULES` | `tuning_profile_track_geometries` | ignore | json |
| 74 | `TRACK_ON_POST_MACHINED_LAYER` | `track_on_post_machined_layer` | error | json |
| 75 | `TRACK_NOT_CENTERED_ON_VIA` | `track_not_centered_on_via` | ignore | json |
| 76 | `SCHEMATIC_FIELDS_PARITY` | `footprint_symbol_field_mismatch` | warning | json |
| 77 | `VIA_STITCH_OVERLAP` | `via_stitch_overlap` | error | json |
| 78 | `COPPER_OUTSIDE_BOARD` | `copper_outside_board` | warning | json |

(Table generated from `drc_item.h`, `drc_item.cpp` and BDS:169-219 with a script; 33 codes are
not `error`. The ctor assigns `PTH_IN_COURTYARD` / `NPTH_IN_COURTYARD` = error explicitly.)
Keys found in existing project files that no longer exist (`zones_intersect`, `keepout`,
`drill_too_small`, `microvia_aspect_ratio`, `zone_has_empty_net`, `overlapping_pads`,
`thermal_via`, `custom_rule`, `aperture_clearance`, `via_hole_larger_than_pad`, ...) are ignored.

### 3.8 `BOARD_STACKUP` (`pcbnew/board_stackup_manager/board_stackup.h`)

- `BOARD_STACKUP`: `m_list: vector<BOARD_STACKUP_ITEM*>` (top to bottom), `m_FinishType`
  (default `"None"`), `m_HasDielectricConstrains` (false), `m_HasThicknessConstrains` (false),
  `m_EdgeConnectorConstraints` (`NONE`/`IN_USE`/`BEVELLED` = 0/1/2), `m_EdgePlating` (false).
- `BOARD_STACKUP_ITEM`: `m_Type` (`UNDEFINED, COPPER, DIELECTRIC, SOLDERPASTE, SOLDERMASK,
  SILKSCREEN`), `m_LayerName`, `m_TypeName`, `m_LayerId` (`UNDEFINED_LAYER` for dielectrics),
  `m_DielectricLayerId` (1-based index of the dielectric, ctor 1), `m_enabled` (true),
  `m_DielectricPrmsList: vector<DIELECTRIC_PRMS>` (always >= 1 element; > 1 only for dielectric
  sub-layers).
- `DIELECTRIC_PRMS`: `m_Material`, `m_Thickness` (0), `m_ThicknessLocked` (false), `m_EpsilonR`
  (1.0), `m_LossTangent` (0.0), `m_SpecFreq` (0.0), `m_DielectricModel` (`CONSTANT`), `m_Color`.

Item constructor defaults per type (board_stackup.cpp:54-103):

| Type | `m_TypeName` | thickness | material | colour | epsilon_r | loss tg | spec freq |
|---|---|---:|---|---|---:|---:|---:|
| COPPER | `"copper"` | 35000 (0.035 mm) | "" | "" | 1.0 | 0 | 0 |
| DIELECTRIC | `"core"` | 0 | `"FR4"` | `"Not specified"` | 4.5 | 0.02 | 1e9 |
| SOLDERPASTE | `"solderpaste"` | 0 | "" | "" | 1.0 | 0 | 0 |
| SOLDERMASK | `"soldermask"` | 10000 (0.01 mm) | `"Not specified"` | `"Not specified"` | 3.3 | 0 | 0 |
| SILKSCREEN | `"silkscreen"` | 0 | `"Not specified"` | `"Not specified"` | 1.0 | 0 | 0 |

Predicates (L317-365): `HasEpsilonRValue`/`HasLossTangentValue` = DIELECTRIC or SOLDERMASK;
`HasSpecFreqValue` = DIELECTRIC; `IsMaterialEditable`/`IsColorEditable` = DIELECTRIC, SOLDERMASK,
SILKSCREEN; `IsThicknessEditable` = COPPER, DIELECTRIC, SOLDERMASK; `HasMaterialValue( i )` =
material editable and `IsPrmSpecified( material )`; `IsPrmSpecified( s )` = non-empty and not
equal (case-insensitive) to `"Not specified"` (L1038-1050; the translated-string comparison is
irrelevant in the C locale).

Trap: the copy constructor of `BOARD_STACKUP_ITEM` (L106-115) copies everything; `BOARD_STACKUP`
copy/assignment deep-copies the items. `operator==` compares all fields.

### 3.9 Component classes (tier 2; `pcbnew/component_classes/`)

`COMPONENT_CLASS`: name, usage (`STATIC`, `DYNAMIC`, `STATIC_AND_DYNAMIC`, `EFFECTIVE`),
`m_constituentClasses: vector<COMPONENT_CLASS*>` (a constituent class contains itself; an
effective class contains its constituents in sorted-name order; the "none" class has none).
`COMPONENT_CLASS_MANAGER`: `m_constituentClasses`, `m_effectiveClasses`
(`unordered_map<wxString, unique_ptr<COMPONENT_CLASS>>` - never iterated for output, only looked
up and erased), `m_assignmentRules: vector<shared_ptr<COMPONENT_CLASS_ASSIGNMENT_RULE>>`,
`m_noneClass` (empty name), `m_ticker` (cache generation). Each footprint has a
`COMPONENT_CLASS_CACHE_PROXY` holding the static class (from the board file), the dynamic class
(from rules) and the combined class, recomputed lazily when the ticker advances.
`COMPONENT_CLASS_SETTINGS` (project JSON, schema version 0): `m_enableSheetComponentClasses`,
`m_componentClassAssignments: vector<COMPONENT_CLASS_ASSIGNMENT_DATA>`; assignment data = class
name, operator `ALL`/`ANY`, ordered list of `(CONDITION_TYPE, primary, secondary)`.

### 3.10 Tuning profiles (tier 3; `include/project/tuning_profiles.h`)

`TUNING_PROFILES` (nested at `tuning_profiles`, schema version 2): list of `TUNING_PROFILE`
(`profile_name`, `type`, `target_impedance`, `frequency`, `model_solder_mask`,
`enable_time_domain_tuning`, `layer_entries[]`, `via_prop_delay`, `via_overrides[]`,
`net_chain_bridge_prop_delay`) under the key `tuning_profiles_impedance_geometric`. Consumers:
length/delay calculation, DRC `missing_tuning_profile` (`drc_test_provider_misc.cpp:589-606`,
needs only the set of profile names) and tuning-profile implicit rules (`drc_engine.cpp:566-`).
For tiers 1-2 the port needs only: the list of profile names (so that `missing_tuning_profile`
can be evaluated) - everything else can be a stub recorded in `PORT_STATUS.md`.

### 3.11 `PROJECT_LOCAL_SETTINGS` (`.kicad_prl`), the visibility part (tier 2)

Schema version 7. Relevant members: `m_VisibleLayers: LSET` and `m_VisibleItems: GAL_SET`.

- `board.visible_layers`: string, `LSET::FmtHex()` format, parsed with `LSET::ParseHex`
  (`include/base_set.h:372-420`: hex nibbles read from the **end** of the string, `_` ignored,
  stops at the first other character; if nothing was parsed the set is left unchanged). Default
  when the key is missing: `LSET::AllLayersMask().FmtHex()`. There is **no migration** for the
  V9 layer renumbering: an old string such as `"fffffff_ffffffff"` (60 bits) is applied to the new
  numbering as is, which leaves the layers with id >= 60 (`In29_Cu`, `In30_Cu`, high user layers)
  invisible.
- `board.visible_items`: array of visibility-layer names (strings; `"none"` marks an explicitly
  empty selection). Setter (project_local_settings.cpp:68-121): not an array or empty array ->
  `m_VisibleItems |= UserVisbilityLayers()`; otherwise clear the user-visibility bits, set those
  named; if no name was recognised and `"none"` is absent -> all user-visibility layers again.
  `UserVisbilityLayers()` (`common/settings/layer_settings_utils.cpp:27-58`) includes
  `LAYER_FP_VALUES`, `LAYER_FP_REFERENCES`, `LAYER_FP_TEXT`, `LAYER_FOOTPRINTS_FR/BK`, `LAYER_PADS`,
  `LAYER_TRACKS`, `LAYER_VIAS`, `LAYER_ZONES`, ... Migrations 1->2 ... 6->7
  (project_local_settings.cpp:367-568) convert the pre-V5 integer encoding; port them only if old
  `.kicad_prl` files must be honoured (the QA data has one `.kicad_prl`).
- No file: all layers visible, all user-visibility items visible.

Consumers in scope: `BOARD::IsLayerVisible( l )` = enabled and (no project or
`m_VisibleLayers[l]`) (B:1180-1185) - used by `drc_test_provider_text_mirroring.cpp:114`;
`BOARD::GetVisibleLayers()` (B:1188) - used by `BOARD::ComputeBoundingBox` (B:2544-2611) and
`GENDRILL_WRITER_BASE` (`gendrill_writer_base.cpp:524-527`, which forces `Edge_Cuts` on);
`BOARD::IsElementVisible` - used by `FOOTPRINT::GetBoundingBox` for value/reference text
(`footprint.cpp:2430-2439`).

---------------------------------------------------------------------------------------------------

## 4. Control flow and algorithms

### 4.1 Loading a JSON settings document (`JSON_SETTINGS::LoadFromFile`, JS:162-345; `parseAndMigrate`, JS:348-404)

1. Clear the document; `Load()` all parameters (with an empty document this assigns defaults for
   objects whose `m_resetParamsIfMissing` is true - `PROJECT_FILE`, `PROJECT_LOCAL_SETTINGS`).
2. Parse with `nlohmann::json::parse( stream, nullptr, ALLOW_EXCEPTIONS, IGNORE_COMMENTS )`
   (`include/json_common.h:159-160`). Consequences the Rust parser must match:
   - `//` and `/* */` comments are accepted (treated as whitespace) - `serde_json` rejects them,
     so strip comments outside of strings first;
   - a UTF-8 byte order mark is skipped;
   - objects are `std::map<std::string, json>`: **duplicate keys - the last one wins; iteration
     is in byte order of the UTF-8 keys**, not file order. `serde_json::Map` without the
     `preserve_order` feature (a `BTreeMap`) behaves the same. Do not enable `preserve_order`;
   - numbers: integers that fit `int64`/`uint64` stay integers, everything else is a double.
3. Parse error: C++ re-reads the same file as a legacy INI file (`migrateFromLegacy`), which for
   a broken JSON file yields nothing useful. Port: treat the project as absent (all defaults) and
   return a warning. (Deviation to record; no in-scope data depends on it.)
4. `m_original = document`. Read `meta.version` as `int`. Missing / wrong type: `success = false`,
   **no migration**, continue. `0 <= version < schema`: run `Migrate()`. `version > schema`:
   "future format" flag only, the document is used as is.
5. `Migrate()` (JS:813-846): `while( filever < schema )`: no migrator registered for `filever` ->
   stop and return false (document stays partially migrated); run it; on `true` set `filever` to
   the migrator's target and write it to `meta.version`.
6. `Load()` all parameters again, in registration order; an exception thrown while loading one
   parameter is caught and that parameter is skipped (JS:147-158; side effects already made by
   the parameter's setter stay).
7. For each nested settings object, in registration order: `LoadFromFile()` (4.3).

The `PROJECT_FILE` root migrations 1->2, 2->3, 3->4 (PF:328-376) only rewrite
`board.layer_presets` and `schematic.bus_aliases`; there is no migrator for version 0. None of
this reaches the port's scope: **the root `meta.version` can be ignored**.

### 4.2 Parameter load rules (PRM)

`Get<T>( "a.b.c" )` (JS:690-724): the dotted path becomes the JSON pointer `/a/b/c`; if the
document does not contain it, or the nlohmann conversion `get<T>()` throws, the result is
"missing". Conversions:

| Requested | Accepted JSON types | Notes |
|---|---|---|
| `bool` | boolean | |
| `int` | integer, unsigned, float, **boolean** | float -> truncation toward zero (`static_cast`); `true` -> 1 |
| `double` | integer, unsigned, float | not boolean |
| `std::string` / `wxString` | string | |
| `nlohmann::json` (`GetJson`) | anything, including `null` | a key whose value is `null` is *present* |

| Parameter class | Rule (`reset` = the owner's `m_resetParamsIfMissing`) |
|---|---|
| `PARAM<T>( path, ptr, default[, min, max] )` (PRM:118-139) | value present: if a range is given and `max < v || v < min` then `v = default`; assign. Missing: assign `default` only if `reset` |
| `PARAM_ENUM<E>( path, ptr, default, min, max )` (PRM:244-261) | `Get<int>`; present and `min <= v <= max`: assign; present but out of range: assign `default` only if `reset`; missing: assign `default` only if `reset` |
| `PARAM_SCALED<T>( path, ptr, default, min, max, scale )` (PRM:436-457) | `Get<double>`; missing and not `reset`: return. `v = KiROUND<double, T>( d * ( 1.0 / scale ) )`; if range given and `v > max || v < min` then `v = default`; assign. With `scale = pcbIUScale.MM_PER_IU` (= `1 / 1e6`) the factor `1.0 / scale` is exactly `1e6` in IEEE double (verified), so this is `KiROUND( mm * 1e6 )` |
| `PARAM_LAMBDA<T>( path, getter, setter, default )` (PRM:311-330) | **ignores `reset`**: present -> `setter( value )`, missing -> `setter( default )`. For `T = nlohmann::json` every default in the files in scope is `{}` which is a **null** JSON value; all those setters start with "if not array/object: return", so a missing key is a no-op - except where noted in 4.4 |
| `PARAM_LIST<T>` (PRM:512-536) | present: build a new vector from the array elements (not an array -> empty vector), assign (an element conversion error aborts and leaves the target unchanged); missing: `default` only if `reset` |
| `PARAM_MAP<V>` (PRM:764-781), `PARAM_WXSTRING_MAP` (`parameters.cpp:58-78`) | present and an object: clear the target, insert every member (a conversion error aborts with the map partially filled); present but not an object: unchanged; missing: `default` only if `reset` |

The `meta.version` parameter is read-only and never loaded into a member.

Unit conversions used by setters (they differ from `PARAM_SCALED`):
`pcbIUScale.mmToIU( double )` = `(int)( mm < 0 ? mm * 1e6 - 0.5 : mm * 1e6 + 0.5 )`
(`include/base_units.h:92-95`; C cast, no clamping - use the ported `mm_to_iu`);
`pcbIUScale.IUTomm( int )` = `iu / 1e6`. For decimal mm values with up to six fractional digits
`mmToIU` and `KiROUND( mm * 1e6 )` give the same result (checked on 200 000 random values); they
differ in overflow/NaN behaviour (`KiROUND` saturates, the C cast is undefined - use a
saturating cast) and for pathological inputs within one ulp of a half. Port both, do not merge
them.

### 4.3 Nested settings load (`NESTED_SETTINGS::LoadFromFile`, NST:45-126)

1. Clear own document. If the parent's document contains the object at `m_path`, copy its members
   into the own document (`update`).
2. If that succeeded: read own `meta.version` (int). Missing/wrong type -> no migration. If
   `0 <= version < schema`: `Migrate()` inside a `try`: **an exception thrown by a migrator
   aborts the migration chain but keeps every modification made so far** (and `meta.version` is
   not advanced). Port each migrator so that a conversion failure stops at the same statement.
3. `Load()` all parameters (always, also when the parent has no such object).

Nested objects in scope and their schema versions: `net_settings` 5, `board.design_settings` 3,
`component_class_settings` 0, `tuning_profiles` 2.

### 4.4 `board.design_settings`: keys, load rules, migrations

`BOARD_DESIGN_SETTINGS::LoadFromFile` (BDS:1524-1594) = nested load (4.3) + a post-step (below).
`reset` is **false** for this object. Parameters in registration order (BDS:276-1145); "fallback"
is the parameter default, used only for out-of-range values (and, for `PARAM_LAMBDA`, passed to
the setter when the key is missing). Ranges are inclusive, in mm unless noted.

| # | Key | Class | Member | Fallback | Range |
|---:|---|---|---|---|---|
| 1-5 | `bom_export_filename`, `bom_settings`, `bom_presets`, `bom_fmt_settings`, `bom_fmt_presets` | - | BOM (skip) | | |
| 6 | `rules.use_height_for_length_calcs` | `PARAM<bool>` | `m_UseHeightForLengthCalcs` | true | |
| 7 | `rules.min_clearance` | `PARAM_SCALED<int>` | `m_MinClearance` | 0 | 0 .. 25 |
| 8 | `rules.min_connection` | scaled | `m_MinConn` | 0 | 0 .. 100 |
| 9 | `rules.min_track_width` | scaled | `m_TrackMinWidth` | 0.2 | 0 .. 25 |
| 10 | `rules.min_via_annular_width` | scaled | `m_ViasMinAnnularWidth` | **0.5** (ctor value is 0.1) | 0 .. 25 |
| 11 | `rules.min_via_diameter` | scaled | `m_ViasMinSize` | 0.5 | 0 .. 25 |
| 12 | `rules.min_through_hole_diameter` | scaled | `m_MinThroughDrill` | 0.3 | 0 .. 25 |
| 13 | `rules.min_microvia_diameter` | scaled | `m_MicroViasMinSize` | 0.2 | 0 .. 10 |
| 14 | `rules.min_microvia_drill` | scaled | `m_MicroViasMinDrill` | 0.1 | 0 .. 10 |
| 15 | `rules.min_hole_to_hole` | scaled | `m_HoleToHoleMin` | 0.25 | 0 .. 10 |
| 16 | `rules.min_hole_clearance` | scaled | `m_HoleClearance` | 0.25 | 0 .. 100 |
| 17 | `rules.min_silk_clearance` | scaled | `m_SilkClearance` | 0 | -10 .. 100 |
| 18 | `rules.min_groove_width` | scaled | `m_MinGrooveWidth` | 0 | 0 .. 25 |
| 19 | `rules.min_resolved_spokes` | `PARAM<int>` | `m_MinResolvedSpokes` | 2 | 0 .. 99 (count) |
| 20 | `rules.min_text_height` | scaled | `m_MinSilkTextHeight` | 0.8 (800000 IU) | 0 .. 100 |
| 21 | `rules.min_text_thickness` | scaled | `m_MinSilkTextThickness` | 0.08 (80000 IU) | 0 .. 25 |
| 22 | `rules.min_copper_edge_clearance` | scaled | `m_CopperEdgeClearance` | 0.5 | **-0.01** .. 25 (-0.01 mm = -10000 IU is the "legacy: use Edge.Cuts line width" flag) |
| 23 | `rule_severities` | lambda(json) | `m_DRCSeverities` | null | see below |
| 24 | `drc_exclusions` | lambda(json) | `m_DrcExclusions` | null | see below |
| 25 | `track_widths` | lambda(json) | `m_TrackWidthList` | null | |
| 26 | `via_dimensions` | lambda(json) | `m_ViasDimensionsList` | null | |
| 27 | `via_stack_presets` | lambda(json) | `m_ViaStackPresets` | null | tier 3 |
| 28 | `diff_pair_dimensions` | lambda(json) | `m_DiffPairDimensionsList` | null | |
| 29 | `teardrop_options` | lambda(json) | `m_TeardropParamsList` targets | null | |
| 30 | `teardrop_parameters` | lambda(json) | `m_TeardropParamsList` items | null | |
| 31 | `tuning_pattern_settings` | lambda(json) | three meander settings | null | |
| 32-37 | `defaults.silk_line_width`, `defaults.silk_text_size_v` (-> `.y`), `defaults.silk_text_size_h` (-> `.x`), `defaults.silk_text_thickness`, `defaults.silk_text_italic`, `defaults.silk_text_upright` | scaled / `PARAM<bool>` | `[LAYER_CLASS_SILK]` | 0.1, 1.0, 1.0, 0.1, false, **true** | line width and thickness: 1 IU .. 100 mm; text size: 0.001 .. 250 mm |
| 38-43 | `defaults.copper_*` (same six) | | `[LAYER_CLASS_COPPER]` | 0.2, 1.5, 1.5, 0.3, false, true | same |
| 44 | `defaults.board_outline_line_width` | scaled | `m_LineThickness[EDGES]` | 0.05 | 1 IU .. 100 mm |
| 45 | `defaults.courtyard_line_width` | scaled | `m_LineThickness[COURTYARD]` | 0.05 | same |
| 46-51 | `defaults.fab_*` (six) | | `[LAYER_CLASS_FAB]` | 0.1, 1.0, 1.0, 0.15, false, true | |
| 52-57 | `defaults.other_*` (six) | | `[LAYER_CLASS_OTHERS]` | 0.1, 1.0, 1.0, 0.15, false, true | |
| 58 | `defaults.dimension_units` | `PARAM_ENUM` | `m_DimensionUnitsMode` | AUTOMATIC (3) | 0 (INCH) .. 3 |
| 59 | `defaults.dimension_precision` | enum | `m_DimensionPrecision` | X_XXXX (4) | 0 .. 9 (V_VVVVV) |
| 60 | `defaults.dimensions.units_format` | enum | `m_DimensionUnitsFormat` | NO_SUFFIX (0) | 0 .. 2 |
| 61 | `defaults.dimensions.suppress_zeroes` | bool | | true | |
| 62 | `defaults.dimensions.text_position` | enum | | OUTSIDE (0) | 0 .. 1 (MANUAL excluded) |
| 63 | `defaults.dimensions.keep_text_aligned` | bool | | true | |
| 64 | `defaults.dimensions.arrow_length` | `PARAM<int>` (**IU**, not mm) | | 1270000 | none |
| 65 | `defaults.dimensions.extension_offset` | `PARAM<int>` (IU) | | 500000 | none |
| 66-70 | `defaults.apply_defaults_to_fp_fields`, `_text`, `_shapes`, `_dimensions`, `_barcodes` | bool | `m_StyleFP*` | false | |
| 71 | `defaults.zones.min_clearance` | scaled | zone `m_ZoneClearance` | 0.5 | 0 .. 25 |
| 72 | `defaults.zones.min_thickness` | scaled | `m_ZoneMinThickness` | 0.25 | 0.025 .. 25 |
| 73 | `defaults.zones.fill_mode` | enum | `m_FillMode` | POLYGONS (0) | 0 .. 2 |
| 74 | `defaults.zones.hatch_thickness` | scaled | `m_HatchThickness` | 1.0 | 0 .. 25 |
| 75 | `defaults.zones.hatch_gap` | scaled | `m_HatchGap` | 1.5 | 0 .. 25 |
| 76 | `defaults.zones.hatch_orientation` | lambda(double, degrees) | `m_HatchOrientation` | 0.0 | **reset to 0 when missing** |
| 77 | `defaults.zones.hatch_smoothing_level` | `PARAM<int>` | | 0 | 0 .. 3 |
| 78 | `defaults.zones.hatch_smoothing_value` | `PARAM<double>` | | 0.1 | 0 .. 1 |
| 79 | `defaults.zones.border_display_style` | enum | `m_ZoneBorderDisplayStyle` | DIAGONAL_EDGE (2) | 0 .. 3 |
| 80 | `defaults.zones.border_hatch_pitch` | scaled | `m_BorderHatchPitch` | 0.5 | 0.1 .. 2.0 |
| 81 | `defaults.zones.thermal_relief_gap` | `PARAM_SCALED<long>` | `m_ThermalReliefGap` | 0.5 | 0 .. 25 |
| 82 | `defaults.zones.thermal_relief_spoke_width` | `PARAM_SCALED<long>` | `m_ThermalReliefSpokeWidth` | 0.5 | 0 .. 25 |
| 83 | `defaults.zones.pad_connection` | lambda(int) | `SetPadConnection( (ZONE_CONNECTION) v )` | THERMAL (1) | **reset when missing**; no validation |
| 84 | `defaults.zones.corner_smoothing` | lambda(int) | `SetCornerSmoothingType` | 0 | invalid int -> NO_SMOOTHING; reset when missing |
| 85 | `defaults.zones.corner_radius` | lambda(double mm) | `SetCornerRadius( mmToIU( v ) )` | 0.0 | reset when missing |
| 86 | `defaults.zones.remove_islands` | lambda(int) | `SetIslandRemovalMode` | ALWAYS (0) | reset when missing |
| 87 | `defaults.zones.min_island_area` | lambda(double mm^2) | `SetMinIslandArea( (long long)( v * 1e12 ) )` | 10.0 | reset when missing |
| 88 | `rules.max_error` | scaled | `m_MaxError` | 0.005 (5000 IU) | 0.0001 .. 1.0 |
| 89 | `rules.solder_mask_to_copper_clearance` | scaled | `m_SolderMaskToCopperClearance` | 0 | 0 .. 25 |
| 90 | `zones_allow_external_fillets` | bool | `m_ZoneKeepExternalFillets` | false | |

Keys that occur in real project files and are **not read**: `rules.allow_blind_buried_vias`,
`rules.allow_microvias`, `rules.min_via_annulus`, `rules.min_via_drill`,
`rules.min_aperture_clearance`, `zones_use_no_outline`, `defaults.pads`,
`rule_severitieslegacy_no_courtyard_defined`, `rule_severitieslegacy_courtyards_overlap` (sic: a
missing dot in the legacy migration, PF:631-635), and the four `rules.solder_*` keys outside of
migration 1->2.

JSON-lambda setters (all start with a type check and return on mismatch; a missing key therefore
changes nothing unless stated):

- **`rule_severities`** (BDS:379-400): not an object -> return. If the object has
  `hole_near_hole`, set `DRCE_DRILLED_HOLES_TOO_CLOSE` from it first. Then for each item of
  `GetItemsWithSeverities()` (kind `json` in 3.7): if the object contains the item's key, set the
  severity with `SeverityFromString`. A non-string value throws out of the setter (parameter
  aborted, earlier assignments kept). Keys not present keep the current value.
- **`drc_exclusions`** (BDS:412-429): `m_DrcExclusions.clear()` **first, also when the key is
  missing**; not an array -> return; each object entry is converted to a `DRC_EXCLUSION` and
  inserted when its sort key is non-empty (format: `drc.md` 6.4).
- **`track_widths`** (BDS:442-456): not an array -> return; clear the list; for each entry: skip
  `null`/empty containers (`entry.empty()`), else push `mmToIU( entry.get<double>() )`.
  The file's first entry is the `0.0` placeholder for "net class width" and is kept as index 0.
- **`via_dimensions`** (BDS:476-496): array of `{ "diameter": mm, "drill": mm }`; entries that
  are empty, not objects, or lack either key are skipped.
- **`diff_pair_dimensions`** (BDS:627-652): array of `{ "width", "gap", "via_gap" }` (all three
  required).
- **`via_stack_presets`** (BDS:525-606, tier 3): array of objects with a string `name`;
  `start_layer`/`end_layer` = layer names that must resolve to an even copper layer id
  (fallbacks `F_Cu` / `In1_Cu`), booleans with fallbacks (`staggered` false, `use_netclass`
  false, `filled` true, `capped` false), `via_size`/`via_drill` (mm, non-numbers and values
  outside `0 ..= IUTomm( INT_MAX )` -> 0), `pitch` clamped to `0 ..= 25 mm` in mm before
  conversion; duplicates by case-insensitive name are dropped.
- **`teardrop_options`** (BDS:672-715): array; for each object entry copy the booleans
  `td_onvia`, `td_onpthpad`, `td_onsmdpad`, `td_ontrackend`, `td_onroundshapesonly` when present;
  legacy keys applied to all three parameter sets: `td_allow_use_two_tracks`,
  `td_curve_segcount` (> 0 sets `m_CurvedEdges = true`, never clears it), `td_on_pad_in_zone`.
- **`teardrop_parameters`** (BDS:745-797): array; entry needs `td_target_name`
  (`td_round_shape` / `td_rect_shape` / `td_track_end`, else skipped); optional `td_enabled`,
  `td_maxlen` (mm), `td_maxheight` (mm), `td_length_ratio`, `td_height_ratio`,
  `td_curve_segcount` (> 0 -> curved, never cleared), `td_width_to_size_filter_ratio`,
  `td_allow_use_two_tracks`, `td_on_pad_in_zone`.
- **`tuning_pattern_settings`** (BDS:826-865): object with `single_track_defaults`,
  `diff_pair_defaults`, `diff_pair_skew_defaults`; each present sub-object **replaces** the
  member with a fresh `MEANDER_SETTINGS()` overlaid with `min_amplitude`, `max_amplitude`,
  `spacing` (mm), `corner_style` (0 = chamfer, anything else = round), `corner_radius_percentage`
  (int), `single_sided` (bool). Note the fresh object has spacing 600000, so the diff-pair
  default of 1000000 is lost when the sub-object has no `spacing`. `aObj` being `null` makes
  `contains` false three times: no change.

Migrations (registered BDS:1148-1181; run only when the nested `meta.version` exists and is < 3):

- **0 -> 1** (`migrateSchema0to1`, BDS:1431-1480): if both `defaults.dimension_units` and
  `defaults.dimension_precision` exist and are integers: `precision += 3` when units == 0
  (inches), `+= 2` when units == 2 (mm), `+= 0` otherwise; write back.
- **1 -> 2** (BDS:1150-1179): if present as numbers, **overwrite board-file values**:
  `m_SolderMaskExpansion = (int)( rules.solder_mask_clearance * 1e6 )`,
  `m_SolderMaskMinWidth = (int)( rules.solder_mask_min_width * 1e6 )`,
  `m_SolderPasteMargin = (int)( rules.solder_paste_clearance * 1e6 )` (**truncation**, not
  rounding: 0.0638 -> 63799, 0.0643 -> 64299), `m_SolderPasteMarginRatio =
  rules.solder_paste_margin_ratio`; then
  erase the four keys. This runs after the board was parsed, so for a project whose
  `board.design_settings.meta.version` is 0 or 1 the project values win over the board's
  `(setup (pad_to_mask_clearance ...))`. Affects mask/paste plotting - tier 1.
- **2 -> 3** (`migrateSchema2to3`, BDS:1483-1521): `drc_exclusions` entries that are strings or
  `[ string, comment ]` arrays are converted from the legacy pipe-separated form (`drc.md` 7).

Post-step of `LoadFromFile` (BDS:1532-1593), only when the parent is a `PROJECT_FILE`:

1. In the **project** document, object `/board/design_settings/rule_severities`: boolean
   `legacy_no_courtyard_defined` -> set nested `rule_severities.missing_courtyard` to `"error"` /
   `"ignore"` and erase the legacy key; boolean `legacy_courtyards_overlap` -> same for
   `courtyards_overlap`. Any exception (object missing, value not boolean) is swallowed.
2. If the **nested** document contains a key `legacy` (i.e. `board.design_settings.legacy`; this
   does not occur in any test file): `m_ZoneKeepExternalFillets = true` and erase
   `legacy.pcbnew` in the project document.
3. If step 1 changed something: `Load()` all parameters again.

### 4.5 `net_settings`: keys, class reader, migrations

Document shape (version 5):

```json
"net_settings": {
  "meta": { "version": 5 },
  "classes": [
    { "name": "Default", "priority": 2147483647, "clearance": 0.2, "track_width": 0.2,
      "via_diameter": 0.6, "via_drill": 0.3, "microvia_diameter": 0.3, "microvia_drill": 0.1,
      "diff_pair_width": 0.2, "diff_pair_gap": 0.25, "diff_pair_via_gap": 0.25,
      "wire_width": 6, "bus_width": 12, "line_style": 0,
      "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)",
      "tuning_profile": "" },
    { "name": "HV", "priority": 0, "clearance": 1.5, "...": "only the fields that are set" } ],
  "net_colors": null,
  "netclass_assignments": { "/net/name": [ "HV", "Power" ] },
  "netclass_patterns": [ { "netclass": "HV", "pattern": "/HV*" } ],
  "net_chain_classes": {}, "net_chain_netclasses": {} }
```

Parameters in registration order (NS:189-408), all JSON lambdas:

1. **`classes`** (NS:202-221): not an array -> return. `m_netClasses.clear()` (the Default class
   object is kept until replaced). For each entry that is an object with a `name`:
   `readNetClass` (NS:128-187): `name` must be a string (else exception -> parameter aborted);
   `NETCLASS( name, aInitWithDefaults = ( name == "Default" ) )`; then, each only when the key
   exists with the right JSON type: `priority` (number -> `get<int>`), `tuning_profile` (string),
   `clearance`, `track_width`, `via_diameter`, `via_drill`, `microvia_diameter`,
   `microvia_drill`, `diff_pair_width`, `diff_pair_gap`, `diff_pair_via_gap` (number ->
   `mmToIU`), `wire_width`, `bus_width` (number -> `schIUScale.MilsToIU( int )`: the double is
   first truncated to `int` mils, then `(int)( mils * 254 + 0.5 )`), `line_style` (number ->
   int), `pcb_color`, `schematic_color` (strings). If the class is the Default
   (`nc->IsDefault()`), it **replaces** `m_defaultNetClass`; otherwise
   `m_netClasses[name] = nc` (a later duplicate replaces an earlier one).
   Consequences: a Default read from the file has an empty description and the priority from the
   file (-1 if the key is missing); a file without a `Default` entry keeps the constructor
   Default (3.3 defaults, priority `INT_MAX`).
2. `net_colors` (GUI; skip).
3. **`net_chain_classes`**, 4. **`net_chain_netclasses`** (NS:268-318): not an object -> return;
   clear; insert every member whose string value is non-empty.
5. **`netclass_assignments`** (NS:344-358): not an object -> return; clear; for each member
   (key = net name): for each element of the value (an array of strings; a bare string value
   iterates as one element, `null` as none): insert into the set.
6. **`netclass_patterns`** (NS:381-407): not an array -> return; clear; for each object entry
   with string `pattern` and string `netclass`: `ForEachBusMember( pattern, add )` (4.7) where
   `add` = `addSinglePatternAssignment` (NS:762-775): skip if an identical (pattern text, class)
   pair already exists, else append `( EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS ), class )`.

Migrations (NS:543-655), operating on the nested document:

- **0 -> 1**: for every class with an array `nets`: replace each element by
  `ConvertToNewOverbarNotation( name )` (`common/string_utils.cpp:87`).
- **1 -> 2**: nothing.
- **2 -> 3**: if `classes` is an array: build `netclass_patterns` = for every class with `name`
  and an array `nets`, one `{ "pattern": net, "netclass": name }` per net, in class order then
  net order; **overwrite** `netclass_patterns` with it (even if empty).
- **3 -> 4**: if `classes` is an array: assign `priority` = `INT_MAX` to the class named
  `Default` and `0, 1, 2, ...` to the others in array order (a class entry without a string
  `name` throws: migration aborted, see 4.3). If `netclass_assignments` is an object: replace
  each value `v` (a string) by `[]` if `v == ""` else `[ v ]` (a non-string value throws).
- **4 -> 5**: add `"tuning_profile": ""` to every class.

### 4.6 The pattern matcher (`EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`, `eda_pattern_match.cpp`)

Two matchers are tried to be built from the pattern text; a matcher whose `SetPattern` fails is
not added (L525-530):

1. `EDA_PATTERN_MATCH_REGEX_ANCHORED` (L105-116 + L77-102): prepend `^` unless the pattern starts
   with `^`, append `$` unless it ends with `$`; compile with `wxRegEx( ..., wxRE_ADVANCED )`
   (= PCRE2, case sensitive, UTF, "dot matches everything" since `wxRE_NEWLINE` is not given).
   Compile failure -> no regex matcher.
2. `EDA_PATTERN_MATCH_WILDCARD_ANCHORED` (L203-241): build `^` + translated pattern + `$` where
   `?` -> `.`, `*` -> `.*`, each of ``.*+?^${}()|[]/\`` (after the two wildcard cases) is escaped
   with a backslash, everything else copied; compile as above. Equivalent to a plain glob match
   over the whole string where `?` matches exactly one code point and `*` any sequence.

`StartsWith( term )` (L463-473) - the only entry point used by net class resolution - is true
when for **any** matcher the leftmost regex match of `term` starts at offset 0 (`Find`, L125-151).
Because of the anchors this is normally a full-string match, but with a top-level alternation
only the first/last alternatives are anchored: the pattern `/MOSI|/MISO|/CLK` becomes
`^/MOSI|/MISO|/CLK$`, which matches `/MISO_2` at offset 0 (`qa/data/pcbnew/connect/connect.kicad_pro`
has such patterns). Port `StartsWith` literally as "leftmost match start == 0".

Port guidance (see also 11.2 R1): implement the wildcard matcher by hand (exact, no regex
engine). For the regex matcher use the `regex` crate with `dot_matches_new_line( true )`, guarded
by a pre-check that rejects what PCRE2 rejects and normalises what PCRE2 accepts:

| Pattern (after anchoring) | PCRE2 (oracle) | `regex` 1.x as is | Required port behaviour |
|---|---|---|---|
| `^+3V3$`, `^*GND$`, `^?x$` (quantifier after `^`, `(`, `|`) | compile error "quantifier does not follow a repeatable item" -> wildcard only: matches `+3V3` literally | compiles; `^+3V3$` matches `3V3` | reject (no regex matcher) |
| `^/SIG_{OPEN}$`, `^~{RESET}$` (braces that are not a valid `{n}`, `{n,}`, `{n,m}`) | braces are literal | compile error | escape such braces before compiling (or accept "wildcard only", which gives the same result when the rest of the pattern has no regex syntax) |
| `^Net-(D10-A)$` | group: matches `Net-D10-A`; the wildcard matcher matches `Net-(D10-A)` | same | same |
| `^/A/TEMP1+$` | matches `/A/TEMP1`, `/A/TEMP11`; wildcard matches `/A/TEMP1+` | same | same |
| lookaround `(?=..)`, backreferences, `\Q..\E`, `(?#..)` | supported | compile error | deviation: no regex matcher (record in `PORT_STATUS.md`) |
| `\<`, `\>` | literal `<`, `>` | word boundaries | deviation (rare) |
| `\d`, `\w` | ASCII only unless wx sets UCP (verify with the oracle) | Unicode | irrelevant for ASCII net names |

Patterns in the test data that exercise this: `+12V`, `+3.3V`, `+3V3` (dozens of projects),
`Net-(D10-A)`, `Net-(U4-FB+)` (`issue24211`), `/SIG_50_{OPEN}` (`issue11814`),
`/StepperTemp/TEMP1+` (`issue15280`), `.*_[PN]` (`issue17967`), `in\d+`, `.*out\d+`
(`demos/tiny_tapeout`), `.*/\+.*|\+.*`, `/MOSI|/MISO|/CLK|/CS-?[0-9]+|` (`connect`).

### 4.7 Bus-pattern expansion (`NET_SETTINGS::ParseBusVector`, `ParseBusGroup`, `ForEachBusMember`, NS:1423-1906)

`ForEachBusMember( s, f )`: if `ParseBusVector( s )` succeeds call `f` for each member; else if
`ParseBusGroup( s )` succeeds recurse into each member; else call `f( s )`.

- `ParseBusVector` (NS:1456-1684) accepts `PREFIX[a..b]SUFFIX`: prefix may contain quoted parts,
  `\ ` escaped spaces and formatting groups `^{}`, `_{}`, `~{}` (a bare `{` not preceded by one
  of `_ ^ ~` rejects; unescaped space or `]` rejects); `a`, `b` decimal (`ToLong`), must differ;
  suffix only from `+ - P N` and closing braces; members are generated from `min( a, b )` up to
  `max( a, b )` (so `OUT[7..0]` yields `OUT0 .. OUT7`), zero-padded to the widest bound when
  either bound has a leading zero. Port the state machine statement by statement; the four
  `SharedParser*` tests in `qa/tests/common/test_net_settings.cpp:757-800` pin the corner cases
  (including `long` overflow of the bound and an end bound of `LONG_MAX`: use `i64`).
- `ParseBusGroup` (NS:1687-1881) accepts `PREFIX{m1 m2,m3}`; members separated by unescaped
  space or comma; each member is returned as `EscapeString( escape_spaces( m ), CTX_NETNAME )`.

Used in three places: loading `netclass_patterns`, `SetNetclassPatternAssignment` (legacy
`add_net`), and inside `GetEffectiveNetClass` for nets that match nothing (4.8 step 4).

### 4.8 Effective net class resolution

`GetEffectiveNetClass( netName )` (NS:997-1166):

1. Empty name -> the Default class (not cached).
2. Cached -> return the cached class.
3. Collect the set of resolved classes (C++: `std::unordered_set<shared_ptr<NETCLASS>>`, i.e. a
   set by identity; iteration order never reaches the result because the list is sorted in step
   5 - use a `Vec` with identity dedup):
   - label assignments: for each class name in `m_netClassLabelAssignments[netName]` (set order):
     `getExplicitNetclass( name )` = the Default object if `name == "Default"`, else the entry of
     `m_netClasses`, else none; if none: `getOrAddImplicitNetclass( name )` = entry of
     `m_impicitNetClasses`, created on demand as `NETCLASS( name, false )` with priority
     `INT_MAX - 1`;
   - pattern assignments, in list order: for each `( matcher, className )` with
     `matcher.StartsWith( netName )`: explicit class or implicit class as above;
   - chain pattern assignments: the same for every source in map order (`SCHEMATIC`, `BOARD`).
4. If nothing was resolved (NS:1093-1139): `ForEachBusMember( netName, ... )`: for a name that
   parses as a bus, record members in `m_netclassBusMembers[netName]`, resolve each member
   recursively and check whether all members share one class name (members after the first
   mismatch are neither recorded nor resolved; those before it were resolved and cached). If it
   is a bus, all members agree and the shared class is not named `Default`: cache and return
   that class. Otherwise cache and return the Default class. (For ordinary board net names this
   is just "cache and return Default"; the bus part matters only for names containing `[a..b]`
   or `{...}`.)
5. Otherwise build the effective class: `effective = NETCLASS( "Effective for net: <name>",
   false )`; `makeEffectiveNetclass( effective, list )` (NS:1192-1315):
   - sort the list by `( priority ascending, name ascending by wxString::Cmp )`;
   - iterate from the **last** (lowest priority) to the first and, for each parameter the
     constituent has (`HasClearance`, `HasTrackWidth`, `HasViaDiameter`, `HasViaDrill`,
     `HasuViaDiameter`, `HasuViaDrill`, `HasDiffPairWidth`, `HasDiffPairGap`,
     `HasDiffPairViaGap`, `HasWireWidth`, `HasBusWidth`, `HasLineStyle`, pcb colour !=
     UNSPECIFIED, schematic colour != UNSPECIFIED, `HasTuningProfile`), copy value and set the
     parameter's parent to that constituent. Net effect: each parameter comes from the
     highest-priority constituent that defines it. Colours are read through `GetPcbColor()` /
     `GetSchematicColor()`, which return UNSPECIFIED for a class with `m_isDefault`;
   - `addMissingDefaults( effective )` (NS:1318-1409): for each of clearance, track width, via
     diameter, via drill, uvia diameter, uvia drill, diff pair width, diff pair gap, **wire
     width, bus width** that is still unset: copy from the Default class, parent = Default,
     `added = true`; tuning profile: only if unset and the Default has one. (**Not** filled:
     diff pair via gap, line style, colours.) If anything was added, append the Default class to
     the list.
6. If the list has exactly one element (one class, nothing added): cache and return **that
   class itself**. Otherwise `effective.SetConstituentOwners( list )`, register it in
   `m_compositeNetClasses[ effective.GetName() ]` (name = constituent names joined with `,`, e.g.
   `"HV,Default"`; a later composite with the same name replaces the map entry but earlier nets
   keep their own object), cache and return it.

Consequences worth knowing:

- A class defined in the project with only PCB fields (no `wire_width` / `bus_width`) always
  yields a composite `"X,Default"`. A class written by current KiCad has all fields and yields
  itself.
- `GetName()` of the effective class is what custom-rule expressions and reports see
  (`A.NetClass`), `GetHumanReadableName()` is used in messages; value getters (`GetClearance()`
  ...) are what implicit DRC rules and `PCB_VIA::GetDrillValue` use; `Get*Parent()` gives the
  root class for per-class implicit rules.
- If `Default` itself is assigned through a label/pattern together with other classes, it takes
  part in the sort with its own priority (`INT_MAX` normally, -1 for legacy boards and for the
  project-less board ctor Default), then `addMissingDefaults` usually adds nothing.

Worked example (`qa/data/pcbnew/multinetclasses_drc.kicad_pro`, `net_settings` version 4): the
only class in `classes[]` is `Default` (with `"priority": -1`); `netclass_assignments` is
`{ "Net-(R1-Pad2)": [ "CLASS1" ], "Net-(R3-Pad1)": [ "CLASS1", "CLASS2" ] }`. `CLASS1` and
`CLASS2` are not defined, so they become implicit classes (no parameters, priority
`INT_MAX - 1`). `Net-(R1-Pad2)` resolves to a composite of `CLASS1` plus the Default (all values
from Default), named `"CLASS1,Default"`; `Net-(R3-Pad1)` to `"CLASS1,CLASS2,Default"`. The rule
file of that test relies on exactly these names (`A.NetClass == 'CLASS1,Default'`,
`A.hasNetclass('CLASS2')`).

`RecomputeEffectiveNetclasses` (NS:1169-1189), `GetNetClassByName( name )` (NS:1412-1420: user
class by name, else the Default class), `GetCachedEffectiveNetClass`, `HasEffectiveNetClass`:
straightforward.

### 4.9 Binding a board to a project (`BOARD::SetProject`, B:379-417)

```
if same project: return;  if a project is set: ClearProject()
m_project = project
if project && !referenceOnly:
    projectFile.m_BoardSettings = board.m_designSettings
    bds.SetParent( &projectFile, load = !board.m_LegacyDesignSettingsLoaded )   // 4.4
    if board.m_LegacyNetclassesLoaded:
        projectNS.SetDefaultNetclass( boardNS.GetDefaultNetclass() )
        projectNS.SetNetclasses( boardNS.GetNetclasses() )                  // clears caches
        projectNS.SetNetclassPatternAssignments( move( boardNS patterns ) ) // replaces, clears caches
    bds.m_NetSettings = projectNS
```

Notes: the project's label assignments, chain maps and colours are kept in the legacy case;
its pattern list is replaced (also by an empty list). Values that legacy `(setup ...)` tokens
wrote into the private Default class (`trace_clearance`, `via_size`, `via_drill`, `uvia_size`,
`uvia_drill`) reach the project only when the board also has at least one `(net_class ...)`
section and none of them is named `Default` (a `(net_class Default ...)` replaces the object and
its values); without `m_LegacyNetclassesLoaded` they are dropped together with the private
`NET_SETTINGS`. In the QA harness without a project the private settings stay in place, so these
values do apply there.
`ClearProject` (B:420-434): release the nested registration and set `bds.m_NetSettings = nullptr`.

### 4.10 Nets

- `NETINFO_LIST::AppendNet( net )` (netinfo_list.cpp:154-184): a net with the same name already
  exists -> the new one is discarded (returns false; same object -> true). Otherwise, if
  `net.m_netCode != m_netCodes.size()` or negative: `net.m_netCode = getFreeNetCode()` (L308-316:
  `do { if( m_newNetCode < 0 ) m_newNetCode = 0; } while( m_netCodes.count( ++m_newNetCode ) )`).
  So codes are assigned 1, 2, 3, ... in order of first appearance; a file-provided code is kept
  only when it equals the current count. (Which nets are created when, and the legacy net-code
  remapping table `m_netCodes` of the parser, belong to `pcb-io.md`.)
- `NETINFO_ITEM` ctor (netinfo_item.cpp:43-63): short name = `name.AfterLast( '/' )` (whole name
  when there is no `/`), display name = `UnescapeString( short )`, class = the board's current
  Default class object. `SetNetname` (L227-237) recomputes both.
  `HasAutoGeneratedNetname()` (L220-224): short name starts with `Net-(` or `unconnected-(`.
- `BOARD::FindNet( code )` (B:2805-2815): `m_netCodes` lookup (a static orphaned item when the
  list is empty and code is 0); `FindNet( name )`; `FindOrAddNet( name, code, ... )` (B:2824).
- `BOARD::BuildListOfNets()` -> `NETINFO_LIST::buildListOfNets()` (netinfo_list.cpp:187-228): for
  every net (name order): remember chain name and the two terminal-pad UUIDs; `net.Clear()`
  (class := current Default object, chain cleared, terminals cleared); restore chain name and
  terminal UUIDs (terminal pad pointers stay null until resolved again); then
  `board.SynchronizeNetsAndNetClasses( false )` and `board.SetAreasNetCodesFromNetNames()`.
- `BOARD::SynchronizeNetsAndNetClasses( reset )` (B:3240-3281): **no project -> return**.
  `ApplyNetChainNetclasses()`; `netSettings.ClearAllCaches()`; for every net in name order:
  `net.SetNetClass( netSettings.GetEffectiveNetClass( net.GetNetname() ) )`. If `reset`:
  `UseCustomTrackViaSize( false )`, custom track width / via size / via drill := Default class
  values; custom diff pair width := Default diff pair width if set else track width; gap :=
  Default diff pair gap if set else clearance; via gap := Default via gap if set else the custom
  gap just stored.
- `BOARD::ApplyNetChainNetclasses()` (B:3200-3237): no project / no settings -> return; if
  `net_chain_netclasses` is empty and no `BOARD`-source chain patterns exist -> return; clear the
  `BOARD`-source chain patterns (clears caches); for every net (name order) with a chain name
  that maps to an **existing user net class** (`HasNetclass`): `SetChainPatternAssignment( BOARD,
  net name, class )` - note the net name is used as a *pattern* (so it goes through bus
  expansion and regex/wildcard matching like any other pattern).
- `BOARD::SetAreasNetCodesFromNetNames()` (B:3294-3325): zones not on copper -> net 0; zones with
  a non-zero code: re-read the code from the zone's net (or -1 if it has none).
- `BOARD::MatchDpSuffix( name, &complement )` (B:2835-2880) and `DpCoupledNet` (B:2883-2895):
  scan the name from the end, skipping digits and `_`; the first other character decides: `+` ->
  complement `-`, polarity 1; `-` -> `+`, -1; `N` -> `P`, -1; `P` -> `N`, 1; anything else ->
  0. The complement name = prefix + complement char + the skipped tail. (Used by the diff-pair
  DRC providers and the router.)

### 4.11 What the board file writes into the settings (legacy and current)

The grammar is in `pcb-io.md`; this is the settings-side mapping (`parseSetup`, P:3033-3447).
**L** = sets `m_LegacyDesignSettingsLoaded`.

| Token in `(setup ...)` | Target | L |
|---|---|---|
| `stackup` | `bds.m_stackup`, `m_HasStackup = true` (4.12) | |
| `drill_symbol_profile` | `bds.m_drillSymbolProfile` | |
| `last_trace_width`, `zone_45_only`, `uvias_allowed`, `blind_buried_vias_allowed`, `pad_size`, `pad_drill`, `filled_areas_thickness` | parsed and ignored | L (all but the first and last) |
| `user_trace_width` | append to `m_TrackWidthList` (a leading 0 placeholder is inserted first) | L |
| `trace_clearance`, `via_size`, `via_drill`, `uvia_size`, `uvia_drill` | the board-private Default net class | L |
| `zone_clearance` | default zone clearance | L |
| `clearance_min`, `trace_min`, `via_min_annulus`, `via_min_size`, `through_hole_min` / `via_min_drill`, `hole_to_hole_min`, `uvia_min_size`, `uvia_min_drill`, `max_error` | the matching BDS minimum | L |
| `user_via`, `user_diff_pair` | size lists (via list gets a leading `(0,0)` placeholder; the diff pair list does **not**) | L |
| `segment_width`, `edge_width`, `mod_edge_width`, `pcb_text_width`, `mod_text_width`, `pcb_text_size`, `mod_text_size` | pre-V6 line/text defaults | L |
| `defaults ( ... )` (P:3504-3581) | `edge_clearance` -> `m_CopperEdgeClearance` (+ `m_LegacyCopperEdgeClearanceLoaded`), line widths, text dims, `dimension_units`, `dimension_precision` | L |
| `pad_to_mask_clearance`, `solder_mask_min_width`, `pad_to_paste_clearance`, `pad_to_paste_clearance_ratio`, `allow_soldermask_bridges_in_footprints`, `tenting`, `covering`, `plugging`, `capping`, `filling`, `aux_axis_origin`, `grid_origin`, `zone_defaults`, `pcbplotparams` | current-format board settings | |
| `visible_elements` | `m_LegacyVisibleItems` (GUI migration only) | |

After the loop: if no stackup was read, `stackup.BuildDefaultStackupList( &bds,
board.GetCopperLayerCount() )` (P:3441-3446).

`(net_class NAME "description" (clearance ..) (trace_width ..) (via_dia ..) (via_drill ..)
(uvia_dia ..) (uvia_drill ..) (diff_pair_width ..) (diff_pair_gap ..) (add_net NET)*)`
(`parseNETCLASS`, P:3788-3878): creates `NETCLASS( "", aInitWithDefaults = true )` (so **all**
fields are set, priority -1), sets name/description/values; each `add_net` calls
`SetNetclassPatternAssignment( netName, className )` on the board-private settings (bus
expansion + matcher, 4.6/4.7; overbar conversion for files older than 20210606); a duplicate
class name is an `IO_ERROR`; a class named like the current Default replaces the Default object;
the caller sets `m_LegacyNetclassesLoaded` (P:1396-1399).

Because legacy net names become regex/wildcard patterns, a legacy net called `+5V` matches
literally, but one called `Net-(R1-Pad1)` additionally matches `Net-R1-Pad1` as a regex.

### 4.12 Stackup

- **Default stackup** `BuildDefaultStackupList( settings, activeCu )` (board_stackup.cpp:699-818),
  with `settings` non-null: `enabled = settings.GetEnabledLayers()`, `n =
  settings.GetCopperLayerCount()`; `diel = boardThickness - 35000 * n - 10000 * count( F_Mask,
  B_Mask enabled )`, then `diel /= max( 1, n - 1 )` (integer division). Items top to bottom:
  `F_SilkS` (type name `"Top Silk Screen"`), `F_Paste` (`"Top Solder Paste"`), `F_Mask` (`"Top
  Solder Mask"`) - each only if enabled; then for each layer of `enabled.CuStack()`: a COPPER
  item (`"copper"`), and unless it is `B_Cu` a DIELECTRIC item with thickness `diel`,
  `m_DielectricLayerId = k + 1`, type name `"core"` for even `k` / `"prepreg"` for odd `k`,
  material `FR4`; then `B_Mask` (`"Bottom Solder Mask"`), `B_Paste` (`"Bottom Solder Paste"`),
  `B_SilkS` (`"Bottom Silk Screen"`); finally copy finish, dielectric-constraint flag, edge
  connector and edge plating from the settings' current stackup descriptor. With `settings ==
  nullptr` all allowed layers and 32 copper layers are used (not reachable from the flows in
  scope).
- `BOARD::GetStackupOrDefault()` (B:3478-3486): the stored stackup when `m_HasStackup`, else a
  fresh default stackup. Note the parser already stores a default stackup in `m_stackup` when
  the file has none, with `m_HasStackup` left false.
- `SynchronizeWithBoard( settings )` (L641-696): build the default list; `change` = some current
  item has no counterpart, or some default item has no counterpart; counterpart = same board
  layer id and, for dielectrics, same dielectric id; matched default items are overwritten by
  the stored ones; then replace self and copy the four board-level flags from the settings'
  descriptor (not `m_HasThicknessConstrains`). Returns `change`. Used by gbrjob
  (`gerber_jobfile_writer.cpp:563-568`: "Board stackup settings not up to date." error when it
  changed and `m_HasStackup`) and the stackup export.
- `BuildBoardThicknessFromStackup()` (L617-638): sum of all sub-layer thicknesses of enabled
  items whose thickness is editable (copper, dielectric, mask).
- `GetLayerDistance( a, b )` (L918-976): both copper; equal -> 0; order so that the first comes
  first in the stack (`B_Cu` is last although its id is small); walk the list skipping
  silk/mask/paste items; start at the first layer; add all sub-layer thicknesses of every item
  from the start layer to the end layer inclusive, counting only **half** (integer division by
  2 per sub-layer) of the start and end copper layers when they are inner layers. Consumers:
  `PADSTACK` post-machining knockout (`padstack.cpp:1402`), drill chart, length calculation.
- `GetDrillDepth( a, b )` (L979-1035): walk the whole list from the board surface - from the
  top if `b` is below `a` (`IsCopperLayerLowerThan( b, a )`), else from the bottom; `a` only
  selects the direction; add copper and dielectric thicknesses; after the item for layer `b`
  has been added, the **next** counted item is added at half thickness (integer division per
  sub-layer) and the walk stops. Consumer: backdrill stub length
  (`drill/drill_enumerator.cpp:94-96`).
- What consumers read: gbrjob - `m_FinishType`, `m_HasDielectricConstrains`, `m_EdgePlating`,
  `m_EdgeConnectorConstraints`, per item type / sub-layer thickness / colour / material /
  epsilon_r / loss tangent / type name / dielectric id, plus `bds.GetBoardThickness()`
  (`gerber_jobfile_writer.cpp:252-290, 556-760`); DRC via-stack provider - `GetStackupOrDefault()`
  (tier 2); statistics - `BuildBoardThicknessFromStackup`.

### 4.13 Component classes (tier 2)

- Static classes come from the board file (footprint `component_classes`), via
  `GetEffectiveStaticComponentClass( set of names )` (component_class_manager.cpp:40-66): empty
  -> the none class; one name -> constituent class; several -> effective class named by the
  **sorted** names joined with `,` (`sortClassNames`: `wxString::Cmp`; L418-430).
- `BOARD::SynchronizeComponentClasses( newSheetPaths )` (B:3284-3291) ->
  `SyncDynamicComponentClassAssignments( assignments, enableSheetClasses, newSheetPaths )`
  (L182-289): bump ticker; compile each assignment (`CompileAssignmentRule`, L292-322: text from
  `GetAssignmentInDRCLanguage()` parsed by `DRC_RULES_PARSER::ParseComponentClassAssignmentRules`;
  an empty text or any parse error -> rule dropped and overall result `false`); if sheet classes
  are enabled add one rule per distinct footprint sheet name (skip empty and `/`; quotes removed)
  - the C++ iterates an `unordered_set` here: **sort the sheet names** in the port (rule order
  only affects evaluation order, the resulting class set is sorted anyway); the new rule list is
  installed only if everything compiled; stale dynamic classes are removed.
- `GetAssignmentInDRCLanguage()` (component_class_settings.cpp:227-393) builds
  `(version 2) (assign_component_class "<name>" (condition "<expr>" ) )` (or without condition)
  from the conditions: REFERENCE -> `( A.Reference == 'R1' || ... )` (comma list, trimmed);
  FOOTPRINT -> `( A.Library_Link == '<id>' )`; SIDE -> `( A.Layer == 'F.Cu' | 'B.Cu' )` (`Any` ->
  none); ROTATION -> `( A.Orientation == <int> deg )` (`Any`/non-integer -> none);
  FOOTPRINT_FIELD -> `( A.getField('<name>') == '<value>' )`; CUSTOM -> `( <expr> )`;
  SHEET_NAME -> `( A.memberOfSheet('<name>') )`; joined with ` && ` (ALL) or ` || ` (ANY).
- JSON (`component_class_settings`): `sheet_component_classes: { "enabled": bool }`,
  `assignments: [ { "component_class": str, "conditions_operator": "ALL"|"ANY", "conditions":
  { "<TYPE>[-n]": { "primary": str, "secondary": str } } } ]`. The conditions object is read with
  `items()`, i.e. **in key-sorted order**, the type is the key text before the first `-`.
- Per footprint: dynamic class = classes of all matching rules (`rule.Matches( fp )` evaluates
  the DRC condition with the footprint as item A on its side layer); combined class = union of
  static and dynamic constituent names, sorted (`GetDynamicComponentClassesForFootprint` L389-415,
  `GetCombinedComponentClass` L325-371).
- This needs the rule parser and expression evaluator of `kicad-drc`/`kicad-libeval`, which sit
  above `kicad-pcb`: put the manager behind a trait or implement it in `kicad-drc` (10.1).

### 4.14 Text variables

Resolver chain for a board text item (`PCB_TEXT::GetShownText`, `pcbnew/pcb_text.cpp:180-`): token
`LAYER` -> the item's layer name; then `FOOTPRINT::ResolveTextVar` of the parent footprint
(`footprint.cpp:1548-1680`: `REFERENCE`, `VALUE`, `LAYER`, `FOOTPRINT_LIBRARY`, `FOOTPRINT_NAME`,
`SHORT_NET_NAME(pad)`, `NET_NAME(pad)`, `NET_CLASS(pad)`, `PIN_NAME(pad)`, `EXCLUDE_FROM_*`,
`DNP`, field names, `PROPERTY.x`, then the board); then `BOARD::ResolveTextVar( token, variant,
depth )` (B:686-795):

1. token contains `:` -> cross reference `<uuid or refdes>:<var>`: resolve the footprint by UUID,
   else by case-insensitive reference; delegate `<var>` to it; unresolved -> the literal text
   `<Unresolved: REF:VAR>` (returns true).
2. `FILENAME` (board file name with extension), `FILEPATH` (full path), `VARIANT`,
   `VARIANT_DESC`, `PROJECTNAME` (only with a project), `DRILL_OPERATIONS` / `DRILL_SITES` /
   `DRILL_GROUPS` (drill chart totals).
3. `TITLE_BLOCK::TextVarResolver` (`common/title_block.cpp:123-200`): `ISSUE_DATE`,
   `CURRENT_DATE`, `CURRENT_TIME_HH_MM_SS`, `CURRENT_TIME_LOCALE`, `REVISION`, `TITLE`, `COMPANY`,
   `COMMENT1`..`COMMENT9`; the value is itself expanded against the project
   (`ExpandTextVars( value, project )`); if the result equals `${TOKEN}` it counts as unresolved.
4. `PROJECT::TextVarResolver` (`common/project.cpp:178-220`): `PROJECTNAME`, `CURRENT_DATE`,
   `CURRENT_TIME_HH_MM_SS`, `CURRENT_TIME_LOCALE`, `VCSHASH`, `VCSSHORTHASH`, then the project
   text variables (`text_variables` of the `.kicad_pro` plus `-D` overrides).
5. The board's own `m_properties` (`(property "k" "v")` in the board file).

So: title block beats project variables, which beat board properties
(`qa/tests/pcbnew/test_text_variable_resolution.cpp`). `BOARD::SynchronizeProperties()`
(B:2968-2972) copies the project variables into the board properties when the project is not the
null project - called before saving and by `resolveJobOutputPath`.

String expansion (`common/common.cpp`): `ResolveTextVars( src, resolver, depth )` (L333-360)
loops `ExpandTextVars( text, resolver, INTERNAL )` while the text still contains `${` or `@{`,
at most `m_ResolveTextRecursionDepth` (6) passes, evaluating `@{...}` math after each pass;
`ExpandTextVars` (L103-330) handles `\${` / `\@{` escapes through the markers `<<<ESC_DOLLAR:` /
`<<<ESC_AT:`, nested braces, nested references (inner first), suppression of
`DRC_WARNING`/`DRC_ERROR`/`ERC_*` tokens outside the `FOR_ERC_DRC` context, and leaves unresolved
references as `${token}`; `FinalizeTextVarExpansion` (L88-100) turns the markers back into `${` /
`@{` (canvas contexts) or `\${` / `\@{` (ERC/DRC, internal). Tier 1 needs `${...}` (nearly every
footprint has `${REFERENCE}` on a fab layer); `@{...}` evaluation (`common/text_eval`, 4300
lines) is tier 3 - until then leave `@{...}` text unevaluated and record it.

### 4.15 Where the custom rule file is looked up

- CLI load (`board_loader.cpp:137-141`): the **board file path** with the extension replaced by
  `kicad_dru`.
- `BOARD::GetDesignRulesPath()` (B:437-445; used by `JobExportDrc` and the GUI): empty when the
  board has no file name or no project; else `project.AbsolutePath( <board base name>.kicad_dru )`
  = project directory + that file name (`AbsolutePath`, `common/project.cpp:501-517`: a name
  starting with `${` is returned unchanged, a relative name is resolved against the project
  directory).
- In the CLI the project is `<board dir>/<board name>.kicad_pro`, so both are the same file.
  A missing file is not an error (the engine then has implicit rules only; see `drc.md` 4.1).
- QA: `<relPath>.kicad_dru` if it exists.

### 4.16 Derived getters of `BOARD_DESIGN_SETTINGS`

- `GetSeverity( code )`, `Ignore( code )` (= severity is `IGNORE`).
- `GetBiggestClearanceValue()` (BDS:1779-1810): max of `m_MinClearance`, `m_HoleClearance`,
  `m_HoleToHoleMin`, `m_CopperEdgeClearance` and - when an engine is attached - the `Min()` of
  the worst constraint (`QueryWorstConstraint`) for `CLEARANCE`, `PHYSICAL_CLEARANCE`,
  `PHYSICAL_HOLE_CLEARANCE`, `HOLE_CLEARANCE`, `EDGE_CLEARANCE`, `HOLE_TO_HOLE`; clipped to
  `MAXIMUM_CLEARANCE`. The engine lives in a higher crate: take it as a callback/trait.
  `BOARD::GetMaxClearanceValue()` (B:1325-1354) = max of that, every zone's local clearance and
  every pad's clearance override (cached).
- `GetSmallestClearanceValue()` (BDS:1813-1824): min over Default and user classes with a
  clearance.
- `GetCurrentTrackWidth/ViaSize/ViaDrill/DiffPairWidth/Gap/ViaGap`, `Set*Index`,
  `GetNext*Index` (BDS:1830-1987): router state, tier 3; note `GetCurrentViaDrill()` returns -1
  for a non-positive drill.
- `GetLineThickness( layer )` etc. (BDS:2066-2093): defaults by layer class.
- `GetDRCEpsilon()`, `GetHolePlatingThickness()`: constants (1.5).

### 4.17 Places where iteration order can change results

| Place | C++ container | Port |
|---|---|---|
| JSON objects (`items()`), e.g. `netclass_assignments`, component class `conditions` | `std::map<std::string, json>` (UTF-8 byte order) | `serde_json::Map` as `BTreeMap` (default features) |
| `classes[]`, `netclass_patterns[]`, `track_widths[]`, ... | JSON arrays, file order | `Vec` |
| `m_netClasses`, `m_compositeNetClasses`, `m_impicitNetClasses`, `m_netClassLabelAssignments` (+ inner `std::set`), `m_TextVars`, board `m_properties` | `std::map<wxString, ...>` / `std::set<wxString>`: `wxString` order = code point order on Linux | `BTreeMap<String, _>` / `BTreeSet<String>` (UTF-8 byte order = code point order) |
| nets (`NETINFO_LIST` iteration, `SynchronizeNetsAndNetClasses`, `ApplyNetChainNetclasses`) | `std::map<wxString, NETINFO_ITEM*>` | iterate by name; keep a `BTreeMap<String, NetId>` beside the code-indexed storage |
| resolved classes of one net | `std::unordered_set<shared_ptr>` then **sorted** by (priority, name) | `Vec` + sort with the same comparator; ties are impossible within one `NET_SETTINGS` except an implicit and an explicit class of the same name created at different times (cannot coexist in one resolution) |
| chain pattern sources | `std::map<NET_CHAIN_SOURCE, ...>` | fixed order `SCHEMATIC`, `BOARD` |
| `m_DRCSeverities` | `std::map<int, SEVERITY>` | array indexed by code or `BTreeMap` |
| `m_DrcExclusions` | `std::set` ordered by protobuf bytes | see `drc.md` 6.4 (equality only) |
| component class manager maps | `std::unordered_map` (lookups only), sheet-name `unordered_set` (iterated) | maps: any; sheet names: sort |
| stackup items | `std::vector`, top to bottom | `Vec` |
| `preservedNetChains` etc. in `buildListOfNets` | `unordered_map` (lookups only) | any |

No thread or R-tree ordering exists in this subsystem.

---------------------------------------------------------------------------------------------------

## 5. What this subsystem needs from other subsystems

### 5.1 `kicad-core`

- Units: `mm_to_iu( f64 ) -> i32` with the exact semantics of `EDA_IU_SCALE::mmToIU`
  (`include/base_units.h:92-95`: `(int)( mm < 0 ? mm * IU_PER_MM - 0.5 : mm * IU_PER_MM + 0.5 )`),
  `iu_to_mm( i32 ) -> f64` (`iu / 1e6`), `mils_to_iu( i32 ) -> i32` (pcb: 25400 per mil), the
  schematic scale's `MilsToIU` (254 per mil, argument truncated to `int` first) for
  `wire_width` / `bus_width`; `ki_round::<f64, i32>` and `ki_round::<f64, i64>` (`KiROUND`,
  `libs/kimath/include/math/util.h:99-`); constants `ARC_HIGH_DEF` (5000), `PCB_IU_PER_MM`.
- Layers (`lset.rs`, `layer_ids.rs`): `LSET::all_layers_mask()`, `all_cu_mask( n )`,
  `user_defined_layers_mask( n )` and the no-argument form, `clear_copper_layers()`,
  `clear_user_defined_layers()`, `clear_non_copper_layers()`, `count()`, `set`/`test`,
  `cu_stack()` (copper layers top to bottom), `external_cu_mask()`, `internal_cu_mask()`,
  `LSET::name( id )`, `LSET::name_to_layer( name )`, `fmt_hex()` / `parse_hex()` (`.kicad_prl`),
  `is_copper_layer( id )`, `is_copper_layer_lower_than( a, b )`, `to_layer_id( i32 )`,
  `PCB_LAYER_ID_COUNT`; ids `F_Cu`, `In1_Cu`, `B_Cu`, `F_SilkS`, `B_SilkS`, `F_Mask`, `B_Mask`,
  `F_Paste`, `B_Paste`, `Edge_Cuts`, `Margin`, `F_CrtYd`, `B_CrtYd`, `F_Fab`, `B_Fab`.
- GAL layer ids for visibility (tier 2): `LAYER_TRACKS`, `LAYER_VIAS`, `LAYER_PADS`,
  `LAYER_ZONES`, `LAYER_FILLED_SHAPES`, `LAYER_DRAW_BITMAPS`, `LAYER_FOOTPRINTS_FR`,
  `LAYER_FOOTPRINTS_BK`, `LAYER_FP_VALUES`, `LAYER_FP_REFERENCES`, `LAYER_FP_TEXT`, `LAYER_ANCHOR`,
  `LAYER_POINTS`, `LAYER_VIA_STITCHING`, `LAYER_RATSNEST`, `LAYER_DRC_WARNING`, `LAYER_DRC_ERROR`,
  `LAYER_DRC_EXCLUSION`, `LAYER_LOCKED_ITEM_SHADOW`, `LAYER_CONFLICTS_SHADOW`,
  `LAYER_CONSTRAINT_SHADOW`, `LAYER_BOARD_OUTLINE_AREA`, `LAYER_DRAWINGSHEET`, `LAYER_GRID`,
  `LAYER_SUBGRIDS` and a `GalSet` bitset.
- `Kiid` (`niluuid`, parse from string, `as_string`), `Severity` + `severity_from_string` /
  `severity_to_string` (`report_severity.rs`; `common/widgets/ui_common.cpp:58-77`).
- String helpers (`common/string_utils.cpp`): `unescape_string` (L399), `escape_string( s,
  CTX_NETNAME )` (L265), `convert_to_new_overbar_notation` (L87); wx-style helpers used here:
  `after_last( ch )` (whole string when absent), `after_first`, `before_first`, `before_last`,
  `cmp_no_case`, `wx_matches( text, mask )` (`wxString::Matches` glob: `*`, `?`), `wx_split( s,
  sep )`, `trim`, `starts_with`.
- `EdaAngle` (hatch orientation), `FormatDouble2Str`, `UIDouble2Str`,
  `EDA_UNIT_UTILS::FormatInternalUnits`, `EDA_UNIT_UTILS::UI::StringFromValue` (stackup writer and
  reports).
- **DRC error-code table**: BDS needs, for every code, the settings key and whether it has a
  user-editable severity (3.7). `kicad-drc` sits above `kicad-pcb`, so the enum `PcbDrcCode`
  (78 values, numeric values as in `drc_item.h:45-145`) plus `settings_key()` and
  `has_user_severity()` must live in `kicad-pcb` (or `kicad-core`) and be re-exported by
  `kicad-drc`. Titles and everything else stay in `kicad-drc`.

### 5.2 `kicad-geom`

`Vector2I` only (aux/grid origin, text sizes).

### 5.3 Board model and parser (`kicad-pcb`)

- Board: file name; the three legacy flags; `properties: BTreeMap<String, String>`; title block
  (title, date, revision, company, comments 1-9); variants (current variant, description lookup);
  `copper_layer_count()`, `enabled_layers()`, `layer_name( id )`; `resolve_item( Kiid )`;
  footprints with `reference()`, `uuid`, `sheet_name()`, static component class names,
  `resolve_text_var`, pads (`number`, net, `pin_function`, `clearance_overrides`); zones
  (`is_on_copper_layer`, net / net code, `local_clearance`); drill chart totals for `DRILL_*`
  variables (tier 3).
- Plain data types owned by the board model but stored in BDS: `ZoneSettings` (with its
  constructor defaults), `ZoneLayerProperties`, `TeardropParametersList`, `ViaType`,
  `DrillSymbolProfile`, `DimUnitsMode`, `DimPrecision`, `DimUnitsFormat`, `DimTextPosition`,
  `ViaDimension`, `DiffPairDimension`, `ViaStackPreset`, a `MeanderSettings` data struct (the
  router is not ported; define the struct with the settings).
- Parser contract (see 4.11): set `m_LegacyDesignSettingsLoaded`,
  `m_LegacyCopperEdgeClearanceLoaded`, `m_LegacyNetclassesLoaded` at exactly the listed tokens;
  write legacy net class values into the board-private `NetSettings`; call
  `set_netclass_pattern_assignment` for `add_net`; reset `m_SolderMaskMinWidth` to 0 at the start
  of `(setup`; build the default stackup at the end of `(setup` when none was read; create nets
  through `append_net` semantics (4.10).
- Connected items read `net.net_class` (effective class id), `net.net_name`, `net.short_name`.

### 5.4 Higher crates (called from the load glue, which therefore cannot live inside the settings modules)

- `kicad-connectivity`: `build( board )` (step 6.5 of 1.3).
- `kicad-drc`: engine creation and `init_engine( rule file text )`; `query_worst_constraint(
  kind ) -> min value` for `biggest_clearance_value`; `DrcExclusion` (JSON object <-> struct,
  sort key, legacy string form) and `resolve_drc_exclusions`; for component classes:
  `parse_component_class_assignment_rules( text )` and `DrcRuleCondition::evaluate_for( footprint,
  none, 0, layer )`.
- `kicad-cli` / `kicad-wasm`: read `<board>.kicad_pro`, `<board>.kicad_prl`, `<board>.kicad_dru`
  (each optional) and pass their text to the library together with the board path (for
  `FILENAME`, `FILEPATH`, `PROJECTNAME`), `-D` overrides, and date/time values.

### 5.5 Third-party crates

- `serde_json` (MIT/Apache-2.0, pure Rust, wasm OK), **default features only** (no
  `preserve_order`, no `arbitrary_precision`), plus a ~40-line comment/BOM stripping pre-pass.
- `regex` (MIT/Apache-2.0, pure Rust, wasm OK) for the anchored-regex matcher, behind the
  PCRE2-compatibility guard of 4.6. (`fancy-regex` would add lookaround/backreferences but has
  the same `^+` and brace differences; not needed.)

---------------------------------------------------------------------------------------------------

## 6. Output formats

This subsystem writes almost nothing itself; it parameterises other writers.

### 6.1 `.kicad_pro` (tier 3 - not needed by any flow in scope)

`JSON_SETTINGS::formatFileContents` (JS:623-642): start from `m_original` (the document as
parsed, before migration); for every parameter with `ClearUnknownKeys` (maps such as
`text_variables`, `net_colors`, `netclass_assignments`, `net_chain_*`) replace its subtree by
`{}`; merge the current document over it (`update( ..., merge_objects = true )`, so unknown keys
of the original file survive); `dump( 2 ) + "\n"`: two-space indentation, **keys sorted** (byte
order), nlohmann number formatting (integers as integers; doubles in shortest round-trip form
with a trailing `.0` for integral values), non-ASCII characters written as UTF-8. Nested settings
store into the parent under their path first (`NESTED_SETTINGS::SaveToFile`, NST:129-183).
`meta.filename` is set to `<project name>.kicad_pro`. Value conversions when storing:
`PARAM_SCALED` writes `iu / 1e6`; net class sizes write `IUTomm`; `wire_width`/`bus_width` write
mils; severities write `SeverityToString`; `classes[]` = Default first, then user classes in name
order, each with `name`, `priority`, `schematic_color`, `pcb_color`, `tuning_profile` and only
the optional fields that are set (NS:71-126).

### 6.2 `(stackup ...)` in the board file (`BOARD_STACKUP::FormatBoardStackup`, board_stackup.cpp:821-915)

Owned by the board writer package (`pcb-io.md` 4.4); called only when `bds.m_HasStackup`.
Nothing is printed for an empty list. Per item:
`(layer "<name>" (type "<type name>") ... )` with name = `dielectric <id>` for dielectrics, else
`LSET::Name( layer )`; then per sub-layer (sub-layers after the first are introduced by the bare
word `addsublayer`): `(color "<c>")` if the colour is editable and specified; `(thickness <IU
formatted> [locked])` if the thickness is editable (`locked` only for dielectrics);
`(material "<m>")` if specified; `(epsilon_r <FormatDouble2Str>)` and `(loss_tangent ...)` if
the item type has them and the material is specified; for dielectrics with a specified material
`(spec_frequency ...)(dielectric_model constant|djordjevic_sarkar)`. After the items:
`(copper_finish "<f>")` if specified, `(dielectric_constraints yes|no)`,
`(edge_connector yes|bevelled)` if not NONE, `(edge_plating yes)` if set.

### 6.3 Stackup report (tier 3; `board_stackup_reporter.cpp`)

`BuildStackupCsv` (L193-297): header `Layer,Type,Sublayer` + `Thickness,Locked` + `Material` +
`Color` + `EpsilonR` + `LossTg` (each group optional); one row per enabled item and sub-layer
(sub-layer column `i/n` only for dielectrics with more than one sub-layer; thickness through
`StringFromValue( pcbIUScale, units, value, true )`); then an empty row, `Property,Value` and the
finish / board options rows; written with `CSV_WRITER`. Default output file name:
`<board base name>_stackup.csv` (`.json` for the JSON format, which is the protobuf JSON of the
API message and out of scope), relative to the working directory
(`JOB_EXPORT_PCB_STACKUP::SetDefaultOutputPath`). The text variant (`BuildStackupReport`) is GUI
only.

### 6.4 Values that surface in other outputs

- DRC reports: the severity of every violation (`GetSeverity`), the names of net classes in
  rule names and messages (`GetHumanReadableName()` of parameter parents, e.g.
  `netclass 'HV'`), net names in item descriptions (`UnescapeString( net name )`).
- Gerber/drill: `m_auxOrigin` (plot/drill origin), `m_MaxError`, mask/paste margins, tenting;
  gbrjob: stackup and `GetBoardThickness()`; text items: resolved text variables.

---------------------------------------------------------------------------------------------------

## 7. Backward compatibility to keep

### 7.1 Project documents of older schema versions

Survey of the 386 `.kicad_pro` files under `qa/data` and `demos` at this commit (all parse as
strict JSON, none uses comments):

| Object | Versions found |
|---|---|
| root `meta.version` | 1: 166, 2: 3, 3: 212, 4: 3, missing: 2 |
| `board.design_settings.meta.version` | 3: 11, 2: 257, 1: 3, object present without `meta`: 67, object absent: 48 |
| `net_settings.meta.version` | 5: 158, 4: 67, 3: 125, 2: 16, 1: 1, 0: 12, no `meta`: 2, absent: 5 |

So all `net_settings` migrations and BDS migrations 1->2 and 2->3 are exercised by existing
data; "no `meta`" (no migration, keys read as they are) is common.

### 7.2 Legacy keys

- `rule_severities.hole_near_hole` (V8) read before `hole_to_hole` (4.4).
- `rule_severities.legacy_no_courtyard_defined` / `legacy_courtyards_overlap` (booleans, 4.4
  post-step). The legacy `.pro` migration writes them under a wrong path
  (`board.design_settings.rule_severitieslegacy_...`, 23 files in the test data), where nothing
  reads them - keep ignoring them.
- `rules.solder_mask_clearance`, `rules.solder_mask_min_width`, `rules.solder_paste_clearance`,
  `rules.solder_paste_margin_ratio`: honoured only by migration 1->2.
- `net_settings.classes[].nets` (versions 0-2): turned into patterns by migration 2->3, with the
  overbar conversion of 0->1 (`~NAME~` -> `~{NAME}`).
- `netclass_assignments` as `name -> string` (version <= 3).
- DRC exclusions as strings (BDS version <= 2).

### 7.3 Legacy boards (pre-V6 `(setup ...)` and `(net_class ...)`)

Section 4.11 and 4.9. Additional points:

- With `m_LegacyDesignSettingsLoaded` the severities stay at the constructor defaults, the
  exclusions are empty, and nothing from `board.design_settings` is applied - even when a
  `.kicad_pro` with that object exists.
- `m_CopperEdgeClearance` stays 0.5 mm unless `(defaults (edge_clearance ...))` is present; the
  GUI-only `inferLegacyEdgeClearance` is not run by the CLI.
- `m_ZoneKeepExternalFillets` stays false in the CLI path (the code that would set it for legacy
  projects looks for a key that the migration never writes, 4.4 post-step 2).
- Net names in files older than 20210606 go through `ConvertToNewOverbarNotation` (parser).
- In the QA data 2 boards have legacy setup tokens and 6 have `(net_class ...)` sections.

### 7.4 Legacy `.pro` project files (tier 3)

When `<name>.kicad_pro` is missing and `<name>.pro` exists, `PROJECT_FILE::MigrateFromLegacy`
(PF:379-819) reads the INI file (`wxFileConfig`) and fills the JSON document: pinned libraries,
`text_variables` (group `/text_variables`, entries `N=key:value`), schematic settings, and from
group `/pcbnew`: `DRCExclusionN`, `AllowMicroVias`, `MinClearance`, `MinTrackWidth`,
`MinViaAnnulus` (-> `rules.min_via_annulus`, a key nothing reads), `MinViaDiameter`,
`MinThroughDrill`/`MinViaDrill`, `MinMicroViaDiameter`, `MinMicroViaDrill`, `MinHoleToHole`,
`CopperEdgeClearance`, `SolderMask*`, `SolderPaste*`, the `defaults.*` line/text values,
`DimensionUnits`, `DimensionPrecision`, `TrackWidthN`, `ViaDiameterN`/`ViaDrillN`,
`dPairWidthN`/`dPairGapN`/`dPairViaGapN`; everything else is copied to `legacy.<group>.<key>`.
The written document has no `meta` inside `board.design_settings`, so no nested migration runs.
A board that still has a `.pro` is practically always a legacy board, for which
`board.design_settings` is not read anyway; the only visible effect is `text_variables`.
Recommendation: do not port; treat a lone `.pro` as "no project" and record the gap
(2 boards in `qa/data/pcbnew`: `issue4257`, `issue7004`).

### 7.5 `.kicad_prl`

Old `visible_items` encodings (integers) need migrations 1->2 ... 6->7; `visible_layers` strings
written before the V9 layer renumbering are applied unmigrated (3.11).

---------------------------------------------------------------------------------------------------

## 8. Multi-threading

There is no thread pool in this subsystem. Threading-related code and how to port it:

| C++ | Port |
|---|---|
| `NET_SETTINGS::GetEffectiveNetClass` mutates its caches and is not thread safe; DRC worker threads only read `NETINFO_ITEM::GetNetClass()` (a raw pointer) after `SynchronizeNetsAndNetClasses` ran on the main thread | resolution takes `&mut NetSettings` and runs once in `synchronize_nets_and_net_classes`; afterwards nets hold class ids and everything is read-only |
| `BOARD::GetMaxClearanceValue` caches under `m_CachesMutex` (B:1325-1354); `SetDesignSettings` invalidates | compute on demand or cache in a plain `Option`, invalidate at the same places |
| `COMPONENT_CLASS_CACHE_PROXY` lazy recomputation keyed by `m_ticker` (mutable members read from DRC threads) | compute the class of every footprint eagerly at the end of `synchronize_component_classes` |
| `ExpandTextVars` / `ResolveTextVars` create a fresh `EXPRESSION_EVALUATOR` per call because they run on parallel workers | no shared state; nothing to do |
| `LOCALE_IO` around JSON read/write | the port never depends on the process locale |
| `SETTINGS_MANAGER` project locks (`LOCKFILE`) | not ported |

Results cannot depend on scheduling here; the sequential port is identical by construction.

---------------------------------------------------------------------------------------------------

## 9. Validation

### 9.1 C++ tests to port

| Test file | Cases relevant to the port | What they assert |
|---|---|---|
| `qa/tests/common/test_netclass.cpp` (69) | `NameEqualsMatchesGetName` | `NameEquals` vs `GetName` for root classes, an alias (`SetConstituentNetclasses( { &first } )` keeps its own name), composites `"Power,Ground"`, `"A,B,Signal"`, `",Power"` |
| `qa/tests/common/test_net_settings.cpp` (830) | `SharedParserRejectsMalformedBoundsWithoutExpansion`, `SharedParserExpandsLargestVectorIndex`, `SharedParserPreservesPadding`, `SharedParserGroupPrefixBoundaryUsesSourceSpelling` | bus parser: `D[..2]`, `D[1..]`, `D[1..2`, 30-digit bound rejected; `D[LONG_MAX-1..LONG_MAX]` gives 2 members; `D[01..03]` -> `D01`, `D03`; group prefix end offsets |
| | `CompositeOwnsConstituents` | class `Signal` created without defaults + label assignment `N1 -> {Signal}`: effective class has 2 constituents and name `"Signal,Default"` |
| | `ChainPatternAssignmentResolvesAndClears`, `ClearChainPatternAssignmentsLeavesUserPatterns`, `ChainPatternAssignmentSourcesAreIndependent` | chain patterns take part in resolution; clearing one source keeps the other and the user patterns |
| | equality cases (`*AffectsEquality`, `ChainPatternAssignmentExcludedFromEquality`), `ClearNetChainClassesRemovesAllEntries` | `operator==` semantics (persisted fields only) |
| | `NetChainClassesJson*`, `ChainNetclassesJsonRoundTrip`, `RemovedChainNetclassIsDroppedFromProjectFile`, `CopyFrom*`, `RenameNetPathPrefixRetargetsAssignments` | need `Store`/writer or GUI operations: port the *load* halves (feed the JSON the test expects) or skip with a note |
| `qa/tests/pcbnew/test_board_design_settings.cpp` (370) | `SeededBoardValueSurvivesRoundTrip` | parent has `board.design_settings = { "meta": { "version": 2 } }`; `m_SilkClearance` preset to `mmToIU( -0.1 )` survives `LoadFromFile` (fact 1 of section 0) |
| | `NegativeSilkClearanceRoundTrip` | `rules.min_silk_clearance = -0.1` loads as -100000 (range lower bound -10 mm) |
| | `OneMalformedPresetDoesNotDropTheRest`, `AnAbsurdPresetPitchIsClampedOnLoad` | `via_stack_presets`: 3 entries, the middle with `via_size` as a string -> size 0, others 250000; pitch 100000 mm -> 25 mm (note the test stores **integer** layer ids in `start_layer`, which are not strings and therefore fall back to `F_Cu` / `In1_Cu`) |
| | `Predefined*CyclingMonotonic`, `TrackWidthSwitchAdvancesFromConnectedWidth` | router index helpers (tier 3) |
| | `BiggestClearanceIncludesPhysicalHoleClearance` | needs the DRC engine: port in `kicad-drc` |
| `qa/tests/common/test_nested_settings.cpp` (514) | `AbsentDefaultIsModifiedWhenNotResetIfMissing`, `MigrationFlushesToParent` | load-side semantics; the rest is dirty-tracking/saving (out of scope) |
| `qa/tests/pcbnew/test_text_variable_resolution.cpp` (164) | all 5 | title block > project variables > board properties; `PCB_TEXT::GetShownText( FOR_CANVAS )` of `"Version: ${VERSION}"` |
| `qa/tests/common/test_expand_text_vars.cpp` (649) | suite `ExpandTextVarsTests` (13 cases, L72-233) | escapes `\${..}`, nested references, DRC/ERC token suppression; the cases with `@{...}` need the evaluator (tier 3) |
| `qa/tests/common/test_title_block.cpp` (84) | `SimpleAccess`, `Copy` | title block accessors |
| `qa/tests/pcbnew/test_component_classes.cpp` (233) | `ComponentClasses` | board `component_classes.kicad_pcb` + `.kicad_pro`: expected constituent classes per reference (C1: CAP_1, CLASS_3, CLASS_4; C2: CAP_2, CLASS_3; C3: none; R8: RES_1, RES_2; R88: RES_2; R2: CLASS_1, RES_1, RES_2, RES_3; R1: CLASS_1, CLASS_2, RES_1, RES_2, RES_4; U1, U2: ANY; U3, U4: MULTI_REF; U55: REF_WILDCARD, REF_WILDCARD2; U555: REF_WILDCARD2; R3: `/SHEET1/`, RES_1, RES_2) |
| `qa/tests/common/test_component_class_settings.cpp` (99) | 4 equality cases | settings equality |
| `qa/tests/common/test_project_local_settings.cpp` (199) | `V8PrlFootprintTextStaysVisible`, `MinimalVisibleItemsAppliesMigration` | `visible_items` migrations: `[5, 17, 18]` -> FP text, values, references visible; tracks, zones, pads not |
| `qa/tests/pcbnew/test_fab_stackup.cpp` (64) | `FabStackupSnapshotsRealBoard` | `SynchronizeWithBoard` on `ipc2581/dielectric-sublayer.kicad_pcb`; `BuildBoardThicknessFromStackup` |
| `qa/tests/pcbnew/test_board_loader.cpp` (118) | `BoardLoaderWithInit` | `reference_images_load_save` loads with project, engine and connectivity set |
| `qa/tests/pcbnew/drc/test_drc_multi_netclasses.cpp` | `DRCMultiNetclasses` | board `multinetclasses_drc` (+ `.kicad_pro`, `.kicad_dru`): exactly 2 error-severity violations; depends on implicit classes and the composite names `"CLASS1,Default"` / `"CLASS1,CLASS2,Default"` (worked example in 4.8). Needs custom rules (tier 2) to run end to end; the resolution part can be asserted directly in tier 1 |
| `qa/tests/pcbnew/drc/test_drc_netclass_literal.cpp`, `test_custom_rule_severities.cpp` | all | net class names in rule conditions; severities from rules vs settings |
| `qa/tests/pcbnew/drc/*` (via `KI_TEST::LoadBoard`) | everything | indirect: every DRC regression depends on severities, minima and net classes loaded from the sibling `.kicad_pro` (see `drc.md` 9) |

There are no C++ unit tests for `EDA_COMBINED_MATCHER` in `CTX_NETCLASS` and none for the JSON
parameter classes in isolation; write new table tests from 4.2 and 4.6.

### 9.2 Data

- `qa/data/pcbnew`: 355 boards; 193 with a sibling `.kicad_pro`, 2 with only a legacy `.pro`, 160
  with no project; 66 `.kicad_dru`; 1 `.kicad_prl`. `demos`: 37 `.kicad_pro`, 2 `.kicad_dru`.
- Net class resolution: `qa/data/pcbnew/connect/connect.kicad_pro` (10 classes, regex patterns
  with alternation, partial classes), `issue15280` (83 patterns incl. trailing `+`), `issue22010`
  (13 classes, 1535 patterns incl. `+2V5` ...), `issue18142` (72 wildcard patterns + 17 label
  assignments), `issue24211` (parenthesised net names as patterns), `issue11814` (braces),
  `issue17967` (`.*_[PN]`), `creepage/creepage.kicad_pro` and `issue18878` (label assignments
  to partial classes -> composites), `demos/vme-wren` (72 patterns), `demos/tiny_tapeout`
  (regex + wildcard mix), `demos/jetson-agx-thor-baseboard` (391 label assignments);
  `qa/data/pcbnew/multinetclasses_drc` (implicit classes, worked example of 4.8),
  `qa/data/eeschema/netlists/multinetclasses` (nets with two classes),
  `qa/data/eeschema/netlists/issue16915` (bus patterns `/IN[0..7]`, `/OUT[7..0]`).
- Design settings: every `.kicad_pro`; BDS version 1 files (3) for migration 1->2;
  `api_kitchen_sink.kicad_pro` has both `hole_near_hole` and `hole_to_hole`.
- Stackup: `ipc2581/dielectric-sublayer.kicad_pcb`, any multi-layer demo.
- Component classes: `component_classes.kicad_pcb/.kicad_pro`.

### 9.3 New self-contained tests (no oracle)

1. Default tables: assert every value of 3.3, 3.6, 3.7, 3.8 against the constructors.
2. JSON rules: one test per row of 4.2 (missing key with/without reset, out-of-range, wrong
   type, `true` as int, comment and BOM handling, duplicate keys).
3. `board.design_settings` loader: synthetic documents for every key of 4.4 including the
   migrations (0->1 precision arithmetic, 1->2 truncation with 0.0638, 2->3) and the post-step.
4. `net_settings` loader: one synthetic document per schema version 0..5.
5. Pattern matcher table of 4.6 and bus expansion.
6. Effective net class: single full class -> itself; partial class -> `"X,Default"`; two classes
   with priorities; implicit class; `Default` assigned explicitly; chain patterns; bus net name.
7. Corpus smoke test: load all project files under `qa/data` and `demos`; no errors; the version
   histogram of 7.1.
8. Stackup: default stackup for 2, 4 and 6 layers at 1.6 mm (dielectric thickness
   `( 1600000 - 35000 n - 20000 ) / ( n - 1 )`: 1510000, 480000, 274000), `GetLayerDistance`,
   `GetDrillDepth` hand-computed cases.

### 9.4 Differential tests against the oracle

Always work on a private copy of the data directory (the oracle may write `.kicad_prl`).

1. **Net -> effective net class** (the central test): `kicad-cli pcb export specctra BOARD -o
   out.dsn`. The DSN `(network ...)` section contains one `(class <name> <nets...> (circuit
   (use_via <padstack>)) (rule (width W) (clearance C)))` for the Default class (written as
   `kicad_default`) and for every effective class in use, with `<name>` = `NETCLASS::GetName()`
   (composites appear as `"A,B,Default"`), its member nets, effective track width and clearance,
   and via diameter/drill inside the via padstack name
   (`pcbnew/specctra_import_export/specctra_export.cpp:1579-1812`). Scrape those forms and
   compare **as sets** (the class order comes from an `unordered_map`) with the port's
   resolution. No DSN exporter has to be ported for this. Run over all boards with a project
   (the DSN export refuses some boards, e.g. without a usable outline; skip those).
2. **Rule minima and severities**: `kicad-cli pcb drc --format json --severity-all
   --all-track-errors` compared with the port's DRC (see `drc.md` 9.4); settings errors show up
   as different violation sets / severities. For a focused check generate boards + projects
   where one minimum at a time is violated.
3. **Missing-key semantics**: project files with `board.design_settings` reduced to `{}` /
   `{ "meta": { "version": 3 } }` / single keys removed, next to a board with a via whose
   annular ring is 0.2 mm: no violation while the constructor value (0.1 mm) is in effect, an
   `annular_width` violation if the parameter default (0.5 mm) were applied. Also one file with
   `"min_via_annular_width": 99` (out of range -> 0.5 mm).
4. **Regex flavour**: a project with patterns `+3V3`, `Net-(D1-A)`, `/A|/B`, `~{RESET}`,
   `D[0..3]`, `*CLK`, `in\d+` and a board with nets chosen on both sides of each pattern; compare
   through the DSN scrape of (1).
5. **Legacy boards**: the boards with `(net_class ...)`; compare (1) with and without a sibling
   `.kicad_pro`.
6. **Stackup**: `kicad-cli pcb export stackup --format csv` (once 6.3 is ported) and the
   `MaterialStackup` section of `.gbrjob` files from `pcb export gerbers`.
7. **Text variables**: plot a layer with text containing `${PROJECTNAME}`, `${TITLE}`,
   `${REFERENCE}`, `${MYVAR}` (with and without `-D MYVAR=x`) and compare the plot output
   (through the plot differential tests).
8. **`.kicad_prl`**: a board with mirrored text on a layer hidden in `.kicad_prl` -> the text
   mirroring violation disappears from the DRC report.

---------------------------------------------------------------------------------------------------

## 10. Rust module layout and work packages

### 10.1 Modules

All in `crates/kicad-pcb/src/` (the crate description already says "board data model,
.kicad_pcb reader/writer and project settings"); one module per C++ file. Modules marked (core)
port `common/` code without board dependencies and may be moved to `kicad-core` if the
orchestrator prefers (they need no JSON).

```
settings/json_settings.rs        JS + NST + PRM: JsonDoc (comment/BOM stripping parse, dotted-path
                                 get<T> with nlohmann coercions, set, contains, erase), migrate
                                 loop, the load helpers of 4.2
project/project.rs               common/project.cpp: Project { full_name, name(), path(),
                                 text_vars, ... }, text_var_resolver, absolute_path, apply_text_vars
project/project_file.rs          PF: ProjectFile { text_vars, boards, net_settings,
                                 component_class_settings, tuning_profiles }, load_from_str
project/net_settings.rs          NS: NetSettings (class arena, assignments, patterns, caches),
                                 JSON load + migrations, effective_net_class, bus parsing
project/component_class_settings.rs   (tier 2)
project/tuning_profiles.rs            (tier 3; names only before that)
project/project_local_settings.rs     (tier 2; visibility only) + settings/layer_settings_utils.rs
netclass.rs               (core) NetClass, NetClassId, names, defaults
eda_pattern_match.rs      (core) anchored regex + anchored wildcard, CombinedMatcher (CTX_NETCLASS)
common/text_vars.rs       (core) ExpandTextVars / ResolveTextVars / FinalizeTextVarExpansion
title_block.rs            (core) TitleBlock + text_var_resolver (if not already in the board model)
board_design_settings.rs         BDS: struct, defaults, JSON load, migrations, getters
board_design_settings_defaults.rs  constants
drc_codes.rs                     PcbDrcCode enum + settings keys (5.1)
netinfo_item.rs, netinfo_list.rs NetInfoItem, NetInfoList (name-ordered + code map)
board_stackup_manager/board_stackup.rs, stackup_predefined_prms.rs, board_stackup_reporter.rs
component_classes/{component_class.rs, component_class_manager.rs, ...}   (tier 2)
board_project.rs                 the parts of board.cpp listed in 2.1: set_project, clear_project,
                                 design_rules_path, synchronize_nets_and_net_classes,
                                 apply_net_chain_netclasses, synchronize_properties,
                                 set_areas_net_codes_from_net_names, resolve_text_var,
                                 visibility, max_clearance_value, stackup_or_default
```

Entry points to provide (names indicative):

- `ProjectFile::load_from_str( json: Option<&str> ) -> ( ProjectFile, Vec<Warning> )` - `None`
  or a parse error = the all-default project of 1.3.
- `Board::set_project( &mut self, project: &mut Project )` = 4.9, including the BDS load.
- `load_glue::initialize_loaded_board_cli( ... )` and `..._qa_harness( ... )` reproducing 1.3 and
  1.4. Because they call connectivity and the DRC engine they must live in a crate above
  `kicad-drc` (the same open question as `pcb-io.md` 11.2 Q1); the settings modules only provide
  the individual steps.

Ownership: the project owns `NetSettings`; the board stores the BDS and, in the port, either a
copy-free handle to the project's `NetSettings` or the `Project` itself (design decision of the
board model; C++ shares by `shared_ptr`). The board-private `NetSettings` used during parsing
must exist until `set_project` moved its legacy content.

### 10.2 Work packages

Line estimates are C++ lines to read/port. "Board model" = whichever package implements `BOARD`.

| Id | Tier | Title | C++ files (ranges) | Est. lines | Depends on | Validating tests |
|---|:-:|---|---|---:|---|---|
| SET-1 | 1 | JSON settings core | `common/settings/json_settings.cpp` L48-65, L127-160, L348-430, L690-768, L804-846; `nested_settings.cpp` L45-126; `include/settings/parameters.h` L94-172, L230-330, L408-490, L492-584, L750-822; `parameters.cpp` L58-78; `json_settings_internals.h` | ~800 | - | 9.3 (2); `test_nested_settings.cpp` load cases |
| SET-2 | 1 | NETCLASS, pattern matcher, bus parser | `include/netclass.h`; `common/netclass.cpp` L30-158, L313-435; `common/eda_pattern_match.cpp` L77-151, L203-241, L386-473, L519-530; `net_settings.cpp` L1423-1906 | ~1100 | core string utils | `test_netclass.cpp`; `test_net_settings.cpp` `SharedParser*`; 9.3 (5) |
| SET-3 | 1 | NET_SETTINGS | `include/project/net_settings.h`; `net_settings.cpp` L36-1420 | ~1400 | SET-1, SET-2 | `test_net_settings.cpp` (resolution, chain, equality, `CompositeOwnsConstituents`); 9.3 (4), (6) |
| SET-4 | 1 | BOARD_DESIGN_SETTINGS | `include/board_design_settings.h`; `board_design_settings_defaults.h`; `board_design_settings.cpp` L44-1182, L1431-1613, L1779-2093; DRC code/key table from `pcbnew/drc/drc_item.h` L45-145, L172-188 and `drc_item.cpp` L86-492 (codes, keys, `allItemTypes` order only); `ui_common.cpp` L58-77; zone/teardrop/meander default structs | ~2500 | SET-1, SET-3, core layers | `test_board_design_settings.cpp`; 9.3 (1), (3) |
| SET-5 | 1 | Project file, board binding, net sync | `project_file.cpp` L43-66, L230-236, L822-826; `common/project.cpp` L178-300, L501-517; `board.cpp` L99-178 (settings part of the ctor), L379-445, L2954-2972, L3200-3325; `board_loader.cpp` L99-170 (as the sequence spec) | ~700 | SET-3, SET-4, SET-6, board model | 9.4 (1), (3), (5); `test_board_loader.cpp`; 9.3 (7) |
| SET-6 | 1 | Net info | `pcbnew/netinfo.h`; `netinfo_item.cpp` L43-110, L220-237; `netinfo_list.cpp` L39-228, L308-316 (`RenameNets` L320-389 is tier 3); `board.cpp` L2805-2895 | ~600 | SET-2 (class ids), board model | new unit tests for `append_net` code assignment, short names, `MatchDpSuffix`; DRC regressions. Merge into the board-model package if that already covers nets (`qa/tests/pcbnew/test_import_net_names.cpp` covers only `RenameNets`) |
| SET-7 | 1 | Stackup model | `board_stackup.h`; `board_stackup.cpp` L39-470, L599-818, L918-1050 (and L821-915 for the board writer); `stackup_predefined_prms.h` | ~750 | SET-4 (BDS fields), core layers | `test_fab_stackup.cpp`; 9.3 (8); gbrjob differential |
| SET-8 | 1 | Text variable expansion (`${...}`) | `common/common.cpp` L60-360; `common/title_block.cpp` L72-200; `board.cpp` L650-795; `common/project.cpp` L178-241 | ~700 | SET-5, board model (footprint resolver, `GetShownText`) | `test_expand_text_vars.cpp` (non-math cases), `test_text_variable_resolution.cpp`, `test_title_block.cpp`. Coordinate with the board-model/text package to avoid double porting |
| SET-9 | 2 | `.kicad_prl` visibility | `project_local_settings.cpp` L57-121, L367-568; `layer_settings_utils.cpp`; `board.cpp` L1174-1245 | ~500 | SET-1, core GAL ids | `test_project_local_settings.cpp`; 9.4 (8) |
| SET-10 | 2 | Component classes | `pcbnew/component_classes/*` (all, ~1100); `component_class_settings.cpp/.h` (564); `board.cpp` L3284-3291 | ~1700 | SET-1, SET-5, `kicad-drc` rule parser + conditions (tier 2) | `test_component_classes.cpp`, `test_component_class_settings.cpp` |
| SET-11 | 3 | Tuning profiles | `tuning_profiles.h/.cpp` | ~500 | SET-1 | `qa/tests/pcbnew/drc/test_drc_tuning_profiles.cpp`, `drc_missing_tuning_profile` data |
| SET-12 | 3 | Stackup report + `pcb export stackup` | `board_stackup_reporter.cpp/.h`; `pcbnew_jobs_handler.cpp` L2106-2211; `kicad/cli/command_pcb_export_stackup.cpp` | ~500 | SET-7 | 9.4 (6) |
| SET-13 | 3 | Project writer, legacy `.pro`, router helpers | `json_settings.cpp` L433-656; `nested_settings.cpp` L129-183; `project_file.cpp` L379-819, L887-989; BDS store lambdas and L1830-1987; `net_settings.cpp` `CopyFrom`, renames; `netinfo_list.cpp` L232-305 | ~2500 | all | the round-trip cases of `test_net_settings.cpp`, `test_project_file.cpp`, `test_nested_settings.cpp` |

Thin-slice order: SET-1 -> SET-2 -> SET-3 -> SET-4 -> (board model + parser) -> SET-6 -> SET-5.
After SET-5 a board with its project can be loaded with correct minima, severities and
effective net classes, which is what DRC and plotting need first; SET-7 follows for gbrjob and
padstack knockouts, SET-8 for plotted text. SET-1..4 have no dependency on the board model and
can start immediately (BDS needs only plain data structs from it).

---------------------------------------------------------------------------------------------------

## 11. Risks, traps and open questions

### 11.1 Corrections to sibling documents

1. `drc.md` section 7 ("Rule minima defaults") says that when the project file has no value the
   `PARAM_SCALED` defaults apply, e.g. `rules.min_via_annular_width` -> 0.5 mm. That is **not**
   what happens: `m_resetParamsIfMissing` is false for `BOARD_DESIGN_SETTINGS`, so a missing key
   keeps the constructor value (0.1 mm for the annular width). The 0.5 mm parameter default is
   reached only through an out-of-range value in the file. Likewise "when there is no project at
   all the constructor values apply" should read "no `.kicad_pro` file" - in the CLI there is
   always a project object. Differential test 9.4 (3) decides it against the oracle.
2. `drc.md` 5.3 lists `NetSettings::composite_netclasses()` as a `BTreeMap<String, _>` of all
   composites: note that this map keeps only the **latest** composite per name, while nets may
   hold older objects with equal content; and that after `ClearAllCaches()` implicit classes
   survive.

### 11.2 Risks and traps

- **R1 Regex flavour** (4.6). The C++ uses PCRE2 through `wxRegEx`; `regex` differs on leading
  quantifiers (`+3V3`-style names - very common), literal braces (overbar notation `~{X}` and
  names like `SIG_{1}`), lookaround/backreferences and `\<`/`\>`. The guard must be validated
  against the oracle with differential test 9.4 (4) before the DRC campaign, because a wrong
  class changes clearances for a whole net.
- **R2 Composite classes depend on schematic-only fields.** Dropping `wire_width` / `bus_width`
  from the model changes which nets get composite classes and therefore class names in reports
  and in rule conditions (`A.NetClass == 'X'` is evaluated with `NameEquals` /
  `ContainsNetclassWithName` in the expression code).
- **R3 Legacy flags.** `m_LegacyDesignSettingsLoaded` is set by any of ~35 tokens, including
  harmless ones (`zone_45_only`, `pad_size`, `defaults`, `max_error`). Missing one of them makes
  the port read `board.design_settings` where C++ ignores it.
- **R4 Partial application on malformed input.** Setters that throw half-way (non-string
  severity, non-string class name, wrong element type in a list) leave earlier assignments in
  place; migrators that throw leave the document half migrated without bumping the version.
  Port with early returns at the same statements, not with "validate first, then apply".
- **R5 `PARAM_LAMBDA` vs `reset`.** `drc_exclusions` is cleared and the six `defaults.zones.*`
  lambda values (hatch orientation, pad connection, corner smoothing, corner radius, island
  removal, min island area) are reset to their defaults on every load of a document that lacks
  them, even though `reset` is false for this object.
- **R6 Truncation vs rounding.** Migration 1->2 truncates (`static_cast<int>`), `PARAM_SCALED`
  rounds with `KiROUND`, setters use `mmToIU` (round half away from zero through a C cast).
  Keep the three apart.
- **R7 nlohmann `get<int>` accepts floats and booleans**; `serde_json::Value::as_i64` does not.
  Implement the coercions of 4.2 explicitly.
- **R8 Two load modes** (1.4). Tests ported from C++ and oracle comparisons need different
  sequences; a board without project file behaves differently in each (net class sync,
  visibility, `PROJECTNAME`).
- **R9 Net class arena lifetime.** In C++ items keep composites alive after the settings were
  changed (`m_constituentOwners`). With ids, `clear_all_caches` must not invalidate ids that nets
  still hold; either re-synchronise nets immediately (the C++ callers always do) or never reuse
  arena slots within one settings generation.
- **R10 Default class identity.** The Default class object is replaced (not modified) by
  `classes[]` loading, by a legacy `(net_class Default ...)` and by `SetDefaultNetclass` in
  `SetProject`; nets created earlier point at the old object until `BuildListOfNets` /
  `SynchronizeNetsAndNetClasses` runs. With ids: make "Default" a fixed id whose content is
  replaced, and verify no code path reads a net's class between creation and synchronisation
  (the board parser's `via->GetDrillValue()` fallback for vias without drill is the candidate
  mentioned in `pcb-io.md` 4.3).
- **R11 Priority of the Default class** is `INT_MAX` in project settings but -1 for the
  board-constructor Default and for legacy boards; it matters only when `Default` is assigned
  explicitly next to other classes.
- **R12 `SetEnabledLayers` recounts from its argument**, not from the stored mask with the
  forced mandatory layers; `GetCopperLayerCount()` after loading a file whose layer list lacks
  `B_Cu` would be off by one compared with the mask. Port literally.
- **R13 Advanced config.** `m_DRCEpsilon`, `m_HoleWallThickness`, `m_ResolveTextRecursionDepth`
  are user-overridable in C++ (`kicad_advanced` file). The oracle wrapper uses a private empty
  config, so the defaults apply; keep them as overridable constants.
- **R14 Translated strings.** `GetHumanReadableName`, the Default class description,
  `"Not specified"`, stackup type names (`_HKI`, i.e. not translated at build time) - the port
  uses the English strings; the oracle runs with `LANGUAGE=C`.
- **R15 `.kicad_prl` side effects.** The oracle reads it and may rewrite it; the QA harness
  reads it only when a project is loaded. Only one exists in the test data, but user projects
  always have one. Until SET-9 exists the CLI should warn when a `.kicad_prl` hides layers.

### 11.3 Open questions for the orchestrator

- **Q1** Where does the DRC code/key table live (5.1)? Proposed: `kicad-pcb::drc_codes`
  re-exported by `kicad-drc`.
- **Q2** Where does the load glue of 1.3 / 1.4 live (needs connectivity + DRC)? Same question as
  `pcb-io.md` Q1; proposed: a small `load` module in `kicad-drc` or a dedicated top crate used by
  `kicad-cli`, `kicad-wasm` and the test helpers.
- **Q3** Component class manager needs the rule parser (`kicad-drc`) but is owned by the board
  in C++. Proposed: data (`ComponentClass`, per-footprint results) in `kicad-pcb`, the
  rule-compiling `sync_dynamic_component_class_assignments` in `kicad-drc`.
- **Q4** Does wx's PCRE2 setup enable `PCRE2_UCP` (Unicode `\d`, `\w`)? Decide with one oracle
  run (pattern `\d+` against a net named with a non-ASCII digit); default assumption: no effect
  on real designs.
- **Q5** Should the port honour `-D` text-variable overrides and `.kicad_prl` in the wasm API?
  They are plain optional inputs in the proposed entry points; the question is only API surface.
- **Q6** Text-variable expansion (SET-8) overlaps with the board-model/text work; assign it to
  exactly one package.
