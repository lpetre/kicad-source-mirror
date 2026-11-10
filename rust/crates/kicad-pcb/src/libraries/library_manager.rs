// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of the table half of `common/libraries/library_manager.cpp` (`LIBRARY_MANAGER`:
//! `loadTables`, `loadNestedTables`, `Rows`, `GetRow`, `GetFullURI`, `ExpandURI`) for the
//! footprint table, and of `KIwxExpandEnvVars` / `ExpandEnvVarSubstitutions`
//! (`common/common.cpp`) and `ENV_VAR::IsVersionedEnvVar` (`common/env_vars.cpp`), which the
//! table code uses for its URIs (package DRC-LP).
//!
//! What C++ reads from the process (environment variables, the working directory, the home
//! directory, the project's text variables) is a [`LibraryEnv`] given by the host; the table
//! files are read through a [`LibrarySource`].

use std::collections::{BTreeMap, BTreeSet};

use super::LibrarySource;
use super::library_table::{LibraryTable, LibraryTableRow, LibraryTableScope, LibraryTableType};

/// `FILEEXT::FootprintLibraryTableFileName`.
pub const FOOTPRINT_LIBRARY_TABLE_FILE_NAME: &str = "fp-lib-table";

/// The major version of the reference build (`GetMajorMinorPatchTuple()`), used by
/// `ENV_VAR::GetVersionedEnvVarName`.
pub const KICAD_MAJOR_VERSION: i32 = 10;

/// `ENV_VAR::GetVersionedEnvVarName( aBaseName )`: `KICAD<major>_<base>`.
pub fn versioned_env_var_name(base_name: &str) -> String {
    format!("KICAD{KICAD_MAJOR_VERSION}_{base_name}")
}

/// Port of `ENV_VAR::IsVersionedEnvVar`: `KICAD<digits>_<base>`.
pub fn is_versioned_env_var(name: &str, base_name: &str) -> bool {
    let prefix = "KICAD";
    let suffix = format!("_{base_name}");

    if !name.starts_with(prefix) || !name.ends_with(&suffix) {
        return false;
    }

    if name.len() < prefix.len() + suffix.len() {
        return false;
    }

    let version = &name[prefix.len()..name.len() - suffix.len()];

    !version.is_empty() && version.chars().all(|c| c.is_ascii_digit())
}

/// `ENV_VAR::GetPredefinedEnvVars()` (`predefinedEnvVars`), in their C++ order.
pub fn predefined_env_vars() -> Vec<String> {
    vec![
        "KIPRJMOD".to_string(),
        versioned_env_var_name("SYMBOL_DIR"),
        versioned_env_var_name("3DMODEL_DIR"),
        versioned_env_var_name("FOOTPRINT_DIR"),
        versioned_env_var_name("TEMPLATE_DIR"),
        "KICAD_USER_TEMPLATE_DIR".to_string(),
        "KICAD_PTEMPLATES".to_string(),
        versioned_env_var_name("3RD_PARTY"),
    ]
}

/// What `ExpandEnvVarSubstitutions` and `wxFileName::MakeAbsolute` read from the process and
/// the project, supplied by the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryEnv {
    /// The process environment as `wxGetEnv` sees it in kicad-cli, i.e. including what
    /// kicad-cli itself defines: `KIPRJMOD` (the project directory, set by
    /// `SETTINGS_MANAGER::LoadProject`) and the `KICAD<major>_*` path variables
    /// (`PGM_BASE::SetLocalEnvVariables`).
    pub env_vars: BTreeMap<String, String>,
    /// `PROJECT::TextVarResolver` tokens (`PROJECTNAME` and the project's text variables);
    /// `None` without a project file (the resolver then answers false).
    pub project_vars: Option<BTreeMap<String, String>>,
    /// The working directory, for relative URIs (`wxFileName::MakeAbsolute()`).
    pub cwd: String,
    /// The home directory (`~` in `wxFileName::Normalize`).
    pub home: String,
}

/// `Bracket` of `common/common.cpp`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bracket {
    None,
    Normal,
    Curly,
}

impl Bracket {
    /// The enumerator value (`Bracket_Normal = ')'`, `Bracket_Curly = '}'`).
    fn value(self) -> u32 {
        match self {
            Bracket::None => 0,
            Bracket::Normal => ')' as u32,
            Bracket::Curly => '}' as u32,
        }
    }
}

impl LibraryEnv {
    /// The `readEnvironment` lambda: `wxGetEnv`.
    fn read_environment(&self, name: &str) -> Option<&str> {
        self.env_vars.get(name).map(String::as_str)
    }

    /// `PROJECT::TextVarResolver( &token )`.
    fn text_var_resolver(&self, token: &str) -> Option<&str> {
        self.project_vars.as_ref()?.get(token).map(String::as_str)
    }

    /// Port of `ExpandEnvVarSubstitutions( aString, aProject, aAllowEscapes = true )`.
    pub fn expand_env_var_substitutions(&self, s: &str) -> String {
        self.ki_wx_expand_env_vars(s, None, true)
    }

    /// Port of `KIwxExpandEnvVars` (Unix branch: `$NAME`, `$(NAME)`, `${NAME}`).
    fn ki_wx_expand_env_vars(
        &self,
        s: &str,
        set: Option<&mut BTreeSet<String>>,
        allow_escapes: bool,
    ) -> String {
        // If the same string is inserted twice, we have a loop
        let mut set = set;

        if let Some(seen) = set.as_deref_mut()
            && !seen.insert(s.to_string())
        {
            return s.to_string();
        }

        let chars: Vec<char> = s.chars().collect();
        let strlen = chars.len();
        let mut result = String::with_capacity(s.len());

        // getVersionedEnvVar: the first predefined variable matching KICAD*_<base> that is
        // defined in the environment
        let versioned = |base: &str, result: &mut String| -> bool {
            for var in predefined_env_vars() {
                if wildcard_matches_kicad_star(&var, base)
                    && let Some(value) = self.read_environment(&var)
                {
                    result.push_str(value);
                    return true;
                }
            }

            false
        };

        let is_alnum = |c: char| c.is_alphanumeric();
        let mut n = 0usize;

        while n < strlen {
            let mut str_n = chars[n];

            match str_n {
                '$' => {
                    let control_char = '$';
                    let bracket;

                    if n == strlen - 1 {
                        bracket = Bracket::None;
                    } else {
                        match chars[n + 1] {
                            '(' => {
                                bracket = Bracket::Normal;
                                n += 1;
                                str_n = chars[n]; // skip the bracket
                            }
                            '{' => {
                                bracket = Bracket::Curly;
                                n += 1;
                                str_n = chars[n]; // skip the bracket
                            }
                            _ => bracket = Bracket::None,
                        }
                    }

                    let mut m = n + 1;

                    if m >= strlen {
                        // break out of the switch: nothing appended
                        n += 1;
                        continue;
                    }

                    let mut str_m = chars[m];

                    while is_alnum(str_m) || str_m == '_' || str_m == ':' {
                        m += 1;

                        if m == strlen {
                            str_m = '\0';
                            break;
                        }

                        str_m = chars[m];
                    }

                    let var_name: String = chars[n + 1..m].iter().collect();

                    if bracket == Bracket::Curly
                        && matches!(
                            var_name.as_str(),
                            "DRC_WARNING" | "DRC_ERROR" | "ERC_WARNING" | "ERC_ERROR"
                        )
                    {
                        // These aren't environment variables; pass them through unchanged.
                        // (`strResult << controlChar << bracket << strVarName << str_m`:
                        // the unscoped enum goes through operator<<( int ).)
                        result.push(control_char);
                        result.push_str(&bracket.value().to_string());
                        result.push_str(&var_name);

                        if str_m != '\0' {
                            result.push(str_m);
                        }

                        n = m + 1;
                        continue;
                    }

                    let mut expanded = false;

                    if let Some(value) = self.text_var_resolver(&var_name) {
                        result.push_str(value);
                        expanded = true;
                    } else if let Some(value) = self.read_environment(&var_name) {
                        result.push_str(value);
                        expanded = true;
                    }
                    // Replace unmatched older variables with current locations
                    else if var_name.contains("KISYS3DMOD")
                        || is_versioned_env_var(&var_name, "3DMODEL_DIR")
                    {
                        expanded = versioned("3DMODEL_DIR", &mut result);
                    } else if var_name == "KICAD_SYMBOL_DIR"
                        || is_versioned_env_var(&var_name, "SYMBOL_DIR")
                    {
                        expanded = versioned("SYMBOL_DIR", &mut result);
                    } else if is_versioned_env_var(&var_name, "FOOTPRINT_DIR") {
                        expanded = versioned("FOOTPRINT_DIR", &mut result);
                    } else if is_versioned_env_var(&var_name, "3RD_PARTY") {
                        expanded = versioned("3RD_PARTY", &mut result);
                    } else {
                        // variable doesn't exist => don't change anything
                        if bracket != Bracket::None {
                            result.push(chars[n - 1]);
                        }

                        result.push(str_n);
                        result.push_str(&var_name);
                    }

                    // When a versioned-wildcard branch matched but no env var was found, emit
                    // the original ${VARNAME} text so the closing-bracket handler can append
                    // the closing bracket.
                    if !expanded && bracket != Bracket::None {
                        let is_versioned_wildcard = var_name.contains("KISYS3DMOD")
                            || var_name == "KICAD_SYMBOL_DIR"
                            || is_versioned_env_var(&var_name, "3DMODEL_DIR")
                            || is_versioned_env_var(&var_name, "SYMBOL_DIR")
                            || is_versioned_env_var(&var_name, "FOOTPRINT_DIR")
                            || is_versioned_env_var(&var_name, "3RD_PARTY");

                        if is_versioned_wildcard {
                            result.push(chars[n - 1]);
                            result.push(str_n);
                            result.push_str(&var_name);
                        }
                    }

                    // check the closing bracket
                    if bracket != Bracket::None {
                        let closing = char::from_u32(bracket.value()).unwrap_or('\0');

                        if m == strlen || str_m != closing {
                            // wxLogWarning( "Environment variables expansion failed: missing
                            // '%c' at position %u in '%s'." ): not reproduced (log only)
                        } else {
                            // skip closing bracket unless the variables wasn't expanded
                            if !expanded {
                                result.push(closing);
                            }

                            m += 1;
                        }
                    }

                    n = m; // skip variable name (n = m - 1, then the loop's n++)
                    continue;
                }
                '\\' => {
                    // backslash can be used to suppress special meaning of % and $
                    if allow_escapes
                        && n < strlen - 1
                        && (chars[n + 1] == '%' || chars[n + 1] == '$')
                    {
                        result.push(str_n);
                        n += 1;
                        result.push(chars[n]);
                        n += 1;
                        continue;
                    }

                    result.push(str_n);
                }
                _ => result.push(str_n),
            }

            n += 1;
        }

        let mut loop_check = BTreeSet::new();
        let first_pos = result.find(['{', '(', '%']);
        let last_pos = result.rfind(['}', ')', '%']);

        if let (Some(first), Some(last)) = (first_pos, last_pos)
            && first != last
        {
            let set = match set {
                Some(set) => set,
                None => &mut loop_check,
            };

            result = self.ki_wx_expand_env_vars(&result, Some(set), allow_escapes);
        }

        result
    }

    /// Port of `wxFileName( aPath ).MakeAbsolute( aCwd )` + `GetFullPath()` on Unix
    /// (`Normalize( wxPATH_NORM_DOTS | wxPATH_NORM_ABSOLUTE | wxPATH_NORM_TILDE )`): `~`
    /// expanded, a relative path put under `cwd` (the working directory when `None`), `.`
    /// and `..` resolved, empty components dropped; a trailing separator is kept.
    pub fn make_absolute(&self, path: &str, cwd: Option<&str>) -> String {
        let mut path = path.to_string();

        if path == "~" || path.starts_with("~/") {
            path = format!("{}{}", self.home, &path[1..]);
        }

        let joined = if path.starts_with('/') {
            path
        } else {
            format!("{}/{}", cwd.unwrap_or(&self.cwd), path)
        };

        let trailing = joined.ends_with('/');
        let mut parts: Vec<&str> = Vec::new();

        for comp in joined.split('/') {
            match comp {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                c => parts.push(c),
            }
        }

        let mut out = format!("/{}", parts.join("/"));

        if trailing && !parts.is_empty() {
            out.push('/');
        }

        out
    }
}

/// `var.Matches( "KICAD*_<base>" )`: the predefined variable name against the wildcard.
fn wildcard_matches_kicad_star(var: &str, base: &str) -> bool {
    let suffix = format!("_{base}");
    var.starts_with("KICAD") && var.len() >= 5 + suffix.len() && var.ends_with(&suffix)
}

/// `wxFileName( aDir, aName ).GetFullPath()`.
pub fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// `wxFileName( aPath ).GetPath()`: the directory part (no trailing separator).
pub fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(p) => &path[..p],
        None => "",
    }
}

/// The footprint half of `LIBRARY_MANAGER`: the global and project footprint tables and the
/// nested tables they reference.
#[derive(Clone, Debug, Default)]
pub struct LibraryManager {
    /// What the URI expansion reads.
    pub env: LibraryEnv,
    /// `m_tables[ FOOTPRINT ]`.
    pub global_table: Option<LibraryTable>,
    /// `m_projectTables[ FOOTPRINT ]`.
    pub project_table: Option<LibraryTable>,
    /// `m_childTables`, keyed by the referencing row's raw URI (an `unordered_map`, only
    /// looked up).
    pub child_tables: BTreeMap<String, LibraryTable>,
}

/// Recursion guard of [`LibraryManager::rows`]: the C++ recursion has none and overflows the
/// stack for a nested-table cycle that `loadNestedTables` did not cut (two tables referenced
/// by the same raw URI string); the port stops descending there.
const MAX_TABLE_NESTING: usize = 64;

impl LibraryManager {
    /// An empty manager (no tables loaded).
    pub fn new(env: LibraryEnv) -> Self {
        LibraryManager {
            env,
            ..Default::default()
        }
    }

    /// Port of `LIBRARY_MANAGER::loadTables( aTablePath, aScope, { FOOTPRINT } )`.
    fn load_tables(
        &mut self,
        source: &dyn LibrarySource,
        table_path: &str,
        scope: LibraryTableScope,
    ) {
        match scope {
            LibraryTableScope::Global => self.global_table = None,
            _ => self.project_table = None,
        }

        let file = join_path(table_path, FOOTPRINT_LIBRARY_TABLE_FILE_NAME);

        if !source.is_file_readable(&file) {
            // wxLogTrace( "No library table found at %s" )
            return;
        }

        let table = LibraryTable::from_file(source, &file, scope, LibraryTableType::Uninitialized);

        if table.type_ != LibraryTableType::Footprint {
            // wxLogWarning( "Library table '%s' has type %s but expected %s; skipping" )
            return;
        }

        let mut table = table;
        self.load_nested_tables(source, &mut table);

        match scope {
            LibraryTableScope::Global => self.global_table = Some(table),
            _ => self.project_table = Some(table),
        }
    }

    /// Port of `LIBRARY_MANAGER::LoadGlobalTables( { FOOTPRINT } )` as kicad-cli runs it:
    /// `loadTables( PATHS::GetUserSettingsPath(), GLOBAL )`. The PCM auto-add / auto-remove
    /// passes only change an existing global table under a `KICAD<major>_3RD_PARTY`
    /// directory; they are not ported (see `PORT_STATUS.md`).
    pub fn load_global_tables(&mut self, source: &dyn LibrarySource, user_settings_path: &str) {
        self.load_tables(source, user_settings_path, LibraryTableScope::Global);
    }

    /// Port of `LIBRARY_MANAGER::LoadProjectTables( aProjectPath, { FOOTPRINT } )`.
    pub fn load_project_tables(&mut self, source: &dyn LibrarySource, project_path: &str) {
        if source.is_dir_readable(project_path) {
            self.load_tables(source, project_path, LibraryTableScope::Project);
        } else {
            self.project_table = None;
        }
    }

    /// Port of `LIBRARY_MANAGER::loadNestedTables`.
    fn load_nested_tables(&mut self, source: &dyn LibrarySource, root: &mut LibraryTable) {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let root_scope = root.scope;
        self.process_one_table(source, root, root_scope, &mut seen);
    }

    /// The `processOneTable` lambda of `loadNestedTables`.
    fn process_one_table(
        &mut self,
        source: &dyn LibrarySource,
        table: &mut LibraryTable,
        root_scope: LibraryTableScope,
        seen: &mut BTreeSet<String>,
    ) {
        seen.insert(table.path.clone());

        if !table.is_ok() {
            return;
        }

        let parent_dir = dir_of(&table.path).to_string();

        for row in &mut table.rows {
            if row.type_ != LibraryTableRow::TABLE_TYPE_NAME {
                continue;
            }

            // URI may be relative to parent
            let expanded = self.env.expand_env_var_substitutions(&row.uri);
            let file = self.env.make_absolute(&expanded, Some(&parent_dir));
            let src = source.resolve_symlinks(&file);

            if seen.contains(&src) {
                // "Library table %s has already been loaded!"
                row.ok = false;
                row.error_description =
                    "A reference to this library table already exists".to_string();
                continue;
            }

            let mut child =
                LibraryTable::from_file(source, &file, root_scope, LibraryTableType::Uninitialized);

            self.process_one_table(source, &mut child, root_scope, seen);

            if !child.is_ok() {
                row.ok = false;
                row.error_description = child.error_description.clone();
            }

            // applyLibOverrides( *child ): only for read-only tables with overrides in the
            // kicad settings (m_LibOverrides); not ported (PORT_STATUS.md)

            self.child_tables.insert(row.uri.clone(), child);
        }
    }

    /// Port of `LIBRARY_MANAGER::Rows( FOOTPRINT, aScope, aIncludeInvalid )`: the rows of the
    /// tables of `scope` (global before project) with nested tables flattened, a later row
    /// replacing an earlier one of the same nickname in place of first appearance. The
    /// `SetHidden( true )` that C++ applies to the rows of hidden nested tables is not
    /// reproduced (the flag is not read by the ported code).
    pub fn rows(&self, scope: LibraryTableScope, include_invalid: bool) -> Vec<&LibraryTableRow> {
        let mut rows: BTreeMap<&str, &LibraryTableRow> = BTreeMap::new();
        let mut row_order: Vec<&str> = Vec::new();

        let tables: Vec<&LibraryTable> = match scope {
            LibraryTableScope::Global => self.global_table.iter().collect(),
            LibraryTableScope::Project => self.project_table.iter().collect(),
            LibraryTableScope::Both => self
                .global_table
                .iter()
                .chain(self.project_table.iter())
                .collect(),
            LibraryTableScope::Uninitialized => Vec::new(),
        };

        for table in tables {
            self.process_table(table, include_invalid, 0, &mut rows, &mut row_order);
        }

        row_order.iter().map(|name| rows[name]).collect()
    }

    /// The `processTable` lambda of `Rows`.
    fn process_table<'s>(
        &'s self,
        table: &'s LibraryTable,
        include_invalid: bool,
        depth: usize,
        rows: &mut BTreeMap<&'s str, &'s LibraryTableRow>,
        row_order: &mut Vec<&'s str>,
    ) {
        if table.type_ != LibraryTableType::Footprint || depth > MAX_TABLE_NESTING {
            return;
        }

        if !(table.is_ok() || include_invalid) {
            return;
        }

        for row in &table.rows {
            if !(row.ok || include_invalid) {
                continue;
            }

            if row.type_ == LibraryTableRow::TABLE_TYPE_NAME {
                let Some(child) = self.child_tables.get(&row.uri) else {
                    continue;
                };

                // Don't include libraries from disabled nested tables
                if row.disabled {
                    continue;
                }

                self.process_table(child, include_invalid, depth + 1, rows, row_order);
            } else {
                if !rows.contains_key(row.nickname.as_str()) {
                    row_order.push(row.nickname.as_str());
                }

                rows.insert(row.nickname.as_str(), row);
            }
        }
    }

    /// Port of `LIBRARY_MANAGER::GetRow( FOOTPRINT, aNickname, aScope )` (the row cache is
    /// transparent and not ported).
    pub fn get_row(&self, nickname: &str, scope: LibraryTableScope) -> Option<&LibraryTableRow> {
        self.rows(scope, true)
            .into_iter()
            .find(|row| row.nickname == nickname)
    }

    /// Port of `LIBRARY_MANAGER::GetFullURI( aRow, aSubstituted )`.
    pub fn get_full_uri(&self, row: &LibraryTableRow, substituted: bool) -> String {
        if substituted {
            self.env.expand_env_var_substitutions(&row.uri)
        } else {
            row.uri.clone()
        }
    }

    /// Port of `LIBRARY_MANAGER::ExpandURI`: substituted and made absolute against the
    /// working directory.
    pub fn expand_uri(&self, short_uri: &str) -> String {
        let expanded = self.env.expand_env_var_substitutions(short_uri);
        self.env.make_absolute(&expanded, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> LibraryEnv {
        let mut env = LibraryEnv {
            cwd: "/work".to_string(),
            home: "/home/u".to_string(),
            ..Default::default()
        };
        env.env_vars.insert("KIPRJMOD".into(), "/proj".into());
        env.env_vars.insert(
            "KICAD10_FOOTPRINT_DIR".into(),
            "/usr/share/kicad/footprints/".into(),
        );
        env
    }

    #[test]
    fn expansion() {
        let env = env();
        assert_eq!(
            env.expand_env_var_substitutions("${KIPRJMOD}/a.pretty"),
            "/proj/a.pretty"
        );
        assert_eq!(
            env.expand_env_var_substitutions("$(KIPRJMOD)/a.pretty"),
            "/proj/a.pretty"
        );
        assert_eq!(env.expand_env_var_substitutions("$KIPRJMOD/a"), "/proj/a");
        // an older versioned name falls back to the current one
        assert_eq!(
            env.expand_env_var_substitutions("${KICAD6_FOOTPRINT_DIR}/x.pretty"),
            "/usr/share/kicad/footprints//x.pretty"
        );
        assert_eq!(env.expand_env_var_substitutions("${NOPE}/x"), "${NOPE}/x");
        assert_eq!(
            env.expand_env_var_substitutions("\\${KIPRJMOD}"),
            "\\${KIPRJMOD}"
        );

        let mut bare = LibraryEnv::default();
        bare.cwd = "/w".into();
        assert_eq!(
            bare.expand_env_var_substitutions("${KICAD10_FOOTPRINT_DIR}/x.pretty"),
            "${KICAD10_FOOTPRINT_DIR}/x.pretty"
        );
        assert_eq!(
            bare.expand_uri_for_test("${KICAD10_FOOTPRINT_DIR}/x.pretty"),
            "/w/${KICAD10_FOOTPRINT_DIR}/x.pretty"
        );
    }

    impl LibraryEnv {
        fn expand_uri_for_test(&self, uri: &str) -> String {
            self.make_absolute(&self.expand_env_var_substitutions(uri), None)
        }
    }

    #[test]
    fn absolute() {
        let env = env();
        assert_eq!(
            env.make_absolute("a/../b/./c.pretty", None),
            "/work/b/c.pretty"
        );
        assert_eq!(env.make_absolute("~/x", None), "/home/u/x");
        assert_eq!(env.make_absolute("/a//b/", None), "/a/b/");
        assert_eq!(env.make_absolute("sub/t", Some("/p")), "/p/sub/t");
    }

    #[test]
    fn versioned_names() {
        assert!(is_versioned_env_var(
            "KICAD6_FOOTPRINT_DIR",
            "FOOTPRINT_DIR"
        ));
        assert!(!is_versioned_env_var(
            "KICAD_FOOTPRINT_DIR",
            "FOOTPRINT_DIR"
        ));
        assert!(!is_versioned_env_var(
            "KICADx_FOOTPRINT_DIR",
            "FOOTPRINT_DIR"
        ));
    }
}
