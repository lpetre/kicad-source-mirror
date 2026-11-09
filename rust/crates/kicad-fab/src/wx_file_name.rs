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

//! The part of `wxFileName` (wxWidgets 3.2, Unix path format, the reference platform) that
//! the fabrication writers use to build output file names: split a path into directory, name
//! and extension (`wxFileName::SplitPath`), `SetName` / `SetExt` / `SetPath`, `GetName`,
//! `GetFullName`, `GetFullPath`.
//!
//! Only string manipulation; nothing touches the file system. Not reproduced: the
//! normalisation of the directory part (`wxFileName` stores the directory as components, so
//! `a//b/` comes back as `a/b/`) and volumes (Windows only); the writers pass the caller's
//! output directory through unchanged.

/// A file name split like `wxFileName( fullpath )` does on Unix.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WxFileName {
    /// The directory part, without a trailing separator (`""` for none, `"/"` for the root).
    pub path: String,
    /// `GetName()`: the file name without directory and extension.
    pub name: String,
    /// `GetExt()`.
    pub ext: String,
    /// `m_hasExt`: true when the name had a dot (even with an empty extension, `"name."`).
    pub has_ext: bool,
}

impl WxFileName {
    /// `wxFileName( aFullPath )` (`wxFileName::Assign` -> `SplitPath`, `wxPATH_UNIX`).
    ///
    /// A dot that starts the name (`.hidden`) does not start an extension; a dot in the
    /// directory part is ignored.
    pub fn new(full_path: &str) -> Self {
        let last_slash = full_path.rfind('/');
        let mut last_dot = full_path.rfind('.');

        // check whether this dot occurs at the very beginning of a path component
        if let Some(dot) = last_dot
            && (dot == 0 || full_path.as_bytes()[dot - 1] == b'/')
        {
            last_dot = None;
        }

        // if we do have a dot and a slash, check that the dot is in the name part
        if let (Some(dot), Some(slash)) = (last_dot, last_slash)
            && dot < slash
        {
            last_dot = None;
        }

        let (path, name_start) = match last_slash {
            // The root directory keeps its separator.
            Some(0) => ("/".to_string(), 1),
            Some(slash) => (full_path[..slash].to_string(), slash + 1),
            None => (String::new(), 0),
        };

        let (name, ext, has_ext) = match last_dot {
            Some(dot) => (full_path[name_start..dot].to_string(), full_path[dot + 1..].to_string(), true),
            None => (full_path[name_start..].to_string(), String::new(), false),
        };

        WxFileName { path, name, ext, has_ext }
    }

    /// `wxFileName::GetName()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `wxFileName::SetName()`.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    /// `wxFileName::SetExt()`: an empty extension also clears `m_hasExt`.
    pub fn set_ext(&mut self, ext: &str) {
        self.ext = ext.to_string();
        self.has_ext = !self.ext.is_empty();
    }

    /// `wxFileName::SetPath()` (directory only; see the module comment about normalisation).
    pub fn set_path(&mut self, path: &str) {
        let trimmed = path.trim_end_matches('/');
        self.path = if trimmed.is_empty() && path.starts_with('/') {
            "/".to_string()
        } else {
            trimmed.to_string()
        };
    }

    /// `wxFileName::GetFullName()`: name and, when there is one, `.` and the extension.
    pub fn full_name(&self) -> String {
        if self.has_ext {
            format!("{}.{}", self.name, self.ext)
        } else {
            self.name.clone()
        }
    }

    /// `wxFileName::GetFullPath()`: directory with a separator, then the full name.
    pub fn full_path(&self) -> String {
        if self.path.is_empty() {
            self.full_name()
        } else if self.path.ends_with('/') {
            format!("{}{}", self.path, self.full_name())
        } else {
            format!("{}/{}", self.path, self.full_name())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_like_wx_unix() {
        let f = WxFileName::new("/home/u/board.kicad_pcb");
        assert_eq!((f.path.as_str(), f.name(), f.ext.as_str()), ("/home/u", "board", "kicad_pcb"));
        assert_eq!(f.full_name(), "board.kicad_pcb");
        assert_eq!(f.full_path(), "/home/u/board.kicad_pcb");

        let f = WxFileName::new("a.b/c");
        assert_eq!((f.path.as_str(), f.name(), f.has_ext), ("a.b", "c", false));

        let f = WxFileName::new("dir/.hidden");
        assert_eq!((f.name(), f.has_ext), (".hidden", false));

        let f = WxFileName::new("x.tar.gz");
        assert_eq!((f.name(), f.ext.as_str()), ("x.tar", "gz"));

        let f = WxFileName::new("");
        assert_eq!(f.full_name(), "");

        let f = WxFileName::new("/b.kicad_pcb");
        assert_eq!(f.full_path(), "/b.kicad_pcb");
    }

    #[test]
    fn set_name_ext_path() {
        let mut f = WxFileName::new("/p/board.kicad_pcb");
        f.set_name(&format!("{}-PTH", f.name()));
        f.set_ext("drl");
        assert_eq!(f.full_name(), "board-PTH.drl");
        f.set_path("out/");
        f.set_ext("");
        assert_eq!(f.full_path(), "out/board-PTH");
        f.set_path("");
        assert_eq!(f.full_path(), "board-PTH");
    }
}
