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

//! Port of `include/embedded_files.h` and `common/embedded_files.cpp` (`EMBEDDED_FILES`):
//! data, constructor defaults and accessors (package M0d). Compression, hashing and the
//! writer are package M24.

use std::collections::BTreeMap;
use std::sync::Arc;

/// `EMBEDDED_FILES::EMBEDDED_FILE::FILE_TYPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum EmbeddedFileType {
    Font = 0,
    Model,
    Worksheet,
    Datasheet,
    #[default]
    Other,
}

/// `EMBEDDED_FILES::RETURN_CODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum EmbeddedFilesReturnCode {
    Ok = 0,
    FileNotFound,
    PermissionsError,
    FileAlreadyExists,
    OutOfMemory,
    ChecksumError,
}

/// `EMBEDDED_FILES::EMBEDDED_FILE`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbeddedFile {
    pub name: String,            // name
    pub type_: EmbeddedFileType, // type (constructor: OTHER)
    pub is_valid: bool,          // is_valid (constructor: false)
    /// compressedEncodedData: base64( zstd( data ) ), verbatim from the file.
    pub compressed_encoded_data: String,
    /// decompressedData: filled only if a consumer asks (fonts).
    pub decompressed_data: Vec<u8>,
    pub data_hash: String, // data_hash
}

impl EmbeddedFile {
    /// `EMBEDDED_FILE::Validate` (MMH3 of the decompressed data against `data_hash`).
    pub fn validate(&mut self) -> bool {
        crate::port_pending!("EMBEDDED_FILES::EMBEDDED_FILE::Validate")
    }

    /// `EMBEDDED_FILE::GetLink`: `kicad-embed://` + name.
    pub fn get_link(&self) -> String {
        crate::port_pending!("EMBEDDED_FILES::EMBEDDED_FILE::GetLink")
    }
}

/// `EMBEDDED_FILES` (the `m_embedFonts` member of `BOARD` included). Dropped:
/// `m_fileAddedCallback` (editor hook).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbeddedFiles {
    pub files: BTreeMap<String, Arc<EmbeddedFile>>, // m_files
    pub font_files: Vec<String>,                    // m_fontFiles
    pub embed_fonts: bool,                          // m_embedFonts (initialiser: false)
}

impl EmbeddedFiles {
    /// `EMBEDDED_FILES::EmbeddedFileMap`.
    pub fn embedded_file_map(&self) -> &BTreeMap<String, Arc<EmbeddedFile>> {
        &self.files
    }

    /// `EMBEDDED_FILES::GetEmbeddedFile`.
    pub fn get_embedded_file(&self, name: &str) -> Option<&Arc<EmbeddedFile>> {
        self.files.get(name)
    }

    /// `EMBEDDED_FILES::IsEmpty`.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// `EMBEDDED_FILES::GetAreFontsEmbedded`.
    pub fn are_fonts_embedded(&self) -> bool {
        self.embed_fonts
    }

    /// `EMBEDDED_FILES::SetAreFontsEmbedded`.
    pub fn set_are_fonts_embedded(&mut self, embed_fonts: bool) {
        self.embed_fonts = embed_fonts;
    }

    /// `EMBEDDED_FILES::ClearEmbeddedFiles`.
    pub fn clear_embedded_files(&mut self) {
        self.files.clear();
    }

    /// `EMBEDDED_FILES::HasFile` (by the file-name part of `name`).
    pub fn has_file(&self, name: &str) -> bool {
        crate::port_pending!("EMBEDDED_FILES::HasFile", name)
    }

    /// `EMBEDDED_FILES::AddFile( std::shared_ptr<EMBEDDED_FILE> )`: replaces a file of the same
    /// name.
    pub fn add_file(&mut self, file: Arc<EmbeddedFile>) {
        crate::port_pending!("EMBEDDED_FILES::AddFile", file)
    }

    /// `EMBEDDED_FILES::RemoveFile`.
    pub fn remove_file(&mut self, name: &str) {
        crate::port_pending!("EMBEDDED_FILES::RemoveFile", name)
    }

    /// `EMBEDDED_FILES::UpdateFontFiles`.
    pub fn update_font_files(&mut self) -> &[String] {
        crate::port_pending!("EMBEDDED_FILES::UpdateFontFiles")
    }

    /// `EMBEDDED_FILES::CompressAndEncode`.
    pub fn compress_and_encode(file: &mut EmbeddedFile) -> EmbeddedFilesReturnCode {
        crate::port_pending!("EMBEDDED_FILES::CompressAndEncode", file)
    }

    /// `EMBEDDED_FILES::DecompressAndDecode`.
    pub fn decompress_and_decode(
        file: &mut EmbeddedFile,
        allow_empty_hash: bool,
    ) -> EmbeddedFilesReturnCode {
        crate::port_pending!(
            "EMBEDDED_FILES::DecompressAndDecode",
            file,
            allow_empty_hash
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `EMBEDDED_FILE::EMBEDDED_FILE()` and `EMBEDDED_FILES() = default`
    /// (`include/embedded_files.h`).
    #[test]
    fn constructor_defaults() {
        let f = EmbeddedFile::default();
        assert_eq!(f.type_, EmbeddedFileType::Other);
        assert!(!f.is_valid);
        let files = EmbeddedFiles::default();
        assert!(files.is_empty() && !files.embed_fonts && files.font_files.is_empty());
    }
}
