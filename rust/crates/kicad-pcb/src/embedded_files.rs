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

//! Port of `include/embedded_files.h`, `common/embedded_files.cpp` (`EMBEDDED_FILES`,
//! `EMBEDDED_FILES_PARSER::ParseEmbedded`) and `include/embedded_files_parser.h`.
//!
//! The stored payload is `base64( zstd( data ) )`. Loading decodes it with `base64` and the
//! pure-Rust `ruzstd` decoder and checks the stored checksum (current MMH3, the legacy MMH3
//! `addDataV1` variant, or the SHA-256 of very old files) exactly like the C++; the stored text
//! is kept verbatim and written back unchanged. Only a newly added file is compressed here, by
//! the `ruzstd` encoder (its frames differ from libzstd level 15, they decode identically).
//!
//! Deviations (no filesystem in the library): `AddFile( wxFileName, .. )` and
//! `ComputeFileHash` take the file bytes instead of a path; `UpdateFontFiles` lists the
//! embedded font *names* instead of paths of temporary files (`GetTemporaryFileName`), the font
//! bytes are read with [`EmbeddedFiles::get_embedded_file`]. Dropped: `m_fileAddedCallback`
//! (editor hook).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use base64::Engine as _;
use kicad_geom::mmh3_hash::Mmh3Hash;
use kicad_sexpr::OutputFormatter;
use kicad_sexpr::{IoError, keywords::embedded_files_t::EmbeddedFilesLexer};

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

/// `FILEEXT::KiCadUriPrefix` (`common/wildcards_and_files_ext.cpp`).
pub const KICAD_URI_PREFIX: &str = "kicad-embed";

impl EmbeddedFile {
    /// `EMBEDDED_FILE::Validate` (MMH3 of the decompressed data against `data_hash`).
    pub fn validate(&mut self) -> bool {
        self.is_valid = mmh3_hex(&self.decompressed_data) == self.data_hash;
        self.is_valid
    }

    /// `EMBEDDED_FILE::Validate_SHA256`: the old way of validating the file, retained only to
    /// validate files that were previously embedded.
    pub fn validate_sha256(&mut self) -> bool {
        self.is_valid = sha256_hex(&self.decompressed_data) == self.data_hash;
        self.is_valid
    }

    /// `EMBEDDED_FILE::GetLink`: `kicad-embed://` + name.
    pub fn get_link(&self) -> String {
        format!("{}://{}", KICAD_URI_PREFIX, self.name)
    }
}

/// `EMBEDDED_FILES` (the `m_embedFonts` member of `BOARD` included). Dropped:
/// `m_fileAddedCallback` (editor hook).
///
/// `Clone` is the C++ copy constructor: the payloads are shared (`Arc`), see
/// [`EmbeddedFiles::deep_copy`] for `EMBEDDED_FILES( other, true )` / `operator=`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbeddedFiles {
    pub files: BTreeMap<String, Arc<EmbeddedFile>>, // m_files
    pub font_files: Vec<String>,                    // m_fontFiles
    pub embed_fonts: bool,                          // m_embedFonts (initialiser: false)
}

impl EmbeddedFiles {
    /// `EMBEDDED_FILES::Seed()`.
    pub const fn seed() -> u32 {
        0xABBA2345
    }

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

    /// `EMBEDDED_FILES::ClearEmbeddedFiles` (`aDeleteFiles` is retained in C++ only for API
    /// compatibility: with shared ownership the entries are released either way).
    pub fn clear_embedded_files(&mut self) {
        self.files.clear();
    }

    /// `EMBEDDED_FILES::HasFile`: looks up the file-name part (`wxFileName::GetFullName`) of
    /// `name`.
    pub fn has_file(&self, name: &str) -> bool {
        self.files.contains_key(wx_full_name(name))
    }

    /// `EMBEDDED_FILES::AddFile( std::shared_ptr<EMBEDDED_FILE> )`: `std::map::emplace`, so a
    /// file whose name is already present is dropped and the stored entry is kept. Returns the
    /// entry the collection holds (issue 25362).
    pub fn add_file(&mut self, file: Arc<EmbeddedFile>) {
        self.add_file_stored(file);
    }

    /// [`add_file`](Self::add_file) returning the stored entry (the C++ return value).
    pub fn add_file_stored(&mut self, file: Arc<EmbeddedFile>) -> Arc<EmbeddedFile> {
        let name = file.name.clone();
        self.files.entry(name).or_insert(file).clone()
    }

    /// `EMBEDDED_FILES::AddFile( const wxFileName& aName, bool aOverwrite )` with the file's
    /// bytes passed in (no filesystem): `file_name` is the path or name of the file, `data` its
    /// content. The type follows the extension, the data is compressed and hashed. Returns the
    /// stored file, `None` if the compression failed.
    pub fn add_file_from_data(
        &mut self,
        file_name: &str,
        data: Vec<u8>,
        overwrite: bool,
    ) -> Option<Arc<EmbeddedFile>> {
        let full_name = wx_full_name(file_name).to_string();

        if self.has_file(&full_name) {
            if !overwrite {
                return self.files.get(&full_name).cloned();
            }

            self.files.remove(&full_name);
        }

        let mut efile = EmbeddedFile {
            name: full_name.clone(),
            decompressed_data: data,
            ..EmbeddedFile::default()
        };

        let ext = wx_ext(&full_name).to_uppercase();

        // Handle some common file extensions
        if ext == "STP" || ext == "STPZ" || ext == "STEP" || ext == "WRL" || ext == "WRZ" {
            efile.type_ = EmbeddedFileType::Model;
        } else if ext == "WOFF" || ext == "WOFF2" || ext == "TTF" || ext == "OTF" {
            efile.type_ = EmbeddedFileType::Font;
        } else if ext == "PDF" {
            efile.type_ = EmbeddedFileType::Datasheet;
        } else if ext == "KICAD_WKS" {
            efile.type_ = EmbeddedFileType::Worksheet;
        }

        if Self::compress_and_encode(&mut efile) != EmbeddedFilesReturnCode::Ok {
            return None;
        }

        efile.is_valid = true;

        let result = Arc::new(efile);
        self.files.insert(full_name, result.clone());
        Some(result)
    }

    /// `EMBEDDED_FILES::RemoveFile` (`aErase` is retained in C++ only for API compatibility).
    pub fn remove_file(&mut self, name: &str) {
        self.files.remove(name);
    }

    /// `EMBEDDED_FILES::AssignSharedFrom`: share the payloads of `source` except the files
    /// whose name is in `excluded_names` (the filtered set is built first, so assigning from a
    /// copy of itself is safe).
    pub fn assign_shared_from(
        &mut self,
        source: &EmbeddedFiles,
        excluded_names: &BTreeSet<String>,
    ) {
        let mut filtered = BTreeMap::new();

        for (name, file) in &source.files {
            if excluded_names.contains(&file.name) {
                continue;
            }

            filtered.insert(name.clone(), file.clone());
        }

        self.files = filtered;
    }

    /// `EMBEDDED_FILES::EMBEDDED_FILES( const EMBEDDED_FILES& other, bool aDeepCopy = true )`
    /// (and the copy assignment operator, which always deep-copies): a fresh `EMBEDDED_FILE`
    /// for each entry.
    pub fn deep_copy(&self) -> EmbeddedFiles {
        EmbeddedFiles {
            files: self
                .files
                .iter()
                .map(|(name, file)| (name.clone(), Arc::new(EmbeddedFile::clone(file))))
                .collect(),
            font_files: self.font_files.clone(),
            embed_fonts: self.embed_fonts,
        }
    }

    /// `EMBEDDED_FILES::ClearEmbeddedFonts`: remove every file of type `FONT`.
    pub fn clear_embedded_fonts(&mut self) {
        self.files
            .retain(|_, file| file.type_ != EmbeddedFileType::Font);
    }

    /// `EMBEDDED_FILES::GetFontFiles`.
    pub fn font_files(&self) -> &[String] {
        &self.font_files
    }

    /// `EMBEDDED_FILES::UpdateFontFiles`: the list of embedded fonts, in map order.
    ///
    /// Deviation: C++ lists `GetTemporaryFileName( name ).GetFullPath()`, a cache file written
    /// to disk; the port lists the file names (the keys of [`embedded_file_map`]), whose bytes
    /// the font loader gets from [`get_embedded_file`] (no filesystem in the library).
    ///
    /// [`embedded_file_map`]: Self::embedded_file_map
    /// [`get_embedded_file`]: Self::get_embedded_file
    pub fn update_font_files(&mut self) -> &[String] {
        self.font_files.clear();

        for (name, entry) in &self.files {
            if entry.type_ == EmbeddedFileType::Font {
                self.font_files.push(name.clone());
            }
        }

        &self.font_files
    }

    /// `EMBEDDED_FILES::WriteEmbeddedFiles`: the `(embedded_files ...)` section as the C++
    /// prints it (raw, before prettification). Files without payload are skipped.
    pub fn write_embedded_files<F: OutputFormatter + ?Sized>(
        &self,
        out: &mut F,
        write_data: bool,
    ) -> Result<(), IoError> {
        const MIME_BASE64_LENGTH: usize = 76;
        out.print(format_args!("(embedded_files "))?;

        for entry in self.files.values() {
            let file: &EmbeddedFile = entry;

            // Skip empty files
            if file.compressed_encoded_data.is_empty() {
                continue;
            }

            out.print(format_args!("(file "))?;
            let name = out.quotew(&file.name);
            out.print(format_args!("(name {name})"))?;

            let type_ = match file.type_ {
                EmbeddedFileType::Datasheet => "datasheet",
                EmbeddedFileType::Font => "font",
                EmbeddedFileType::Model => "model",
                EmbeddedFileType::Worksheet => "worksheet",
                EmbeddedFileType::Other => "other",
            };

            out.print(format_args!("(type {type_})"))?;

            if write_data {
                out.print(format_args!("(data"))?;

                let data = &file.compressed_encoded_data;
                let mut first = 0usize;

                while first < data.len() {
                    let remaining = data.len() - first;
                    let length = remaining.min(MIME_BASE64_LENGTH);

                    // "\n%1s%.*s%s\n": "%1s" pads the empty string to one blank.
                    let lead = if first != 0 { " " } else { "|" };
                    let tail = if remaining == length { "|" } else { "" };
                    let view = &data[first..first + length];
                    out.print(format_args!("\n{lead}{view}{tail}\n"))?;
                    first += MIME_BASE64_LENGTH;
                }

                out.print(format_args!(")"))?; // Close data
            }

            let hash = out.quotew(&file.data_hash);
            out.print(format_args!("(checksum {hash})"))?;
            out.print(format_args!(")"))?; // Close file
        }

        out.print(format_args!(")"))?; // Close embedded_files
        Ok(())
    }

    /// `EMBEDDED_FILES::CompressAndEncode`: compress `decompressed_data` into
    /// `compressed_encoded_data` and set `data_hash` to its current MMH3 hash.
    ///
    /// The zstd frame comes from the `ruzstd` encoder (libzstd level 15 is not reproduced, so
    /// the bytes of a *newly* embedded file differ from KiCad's, the decoded data does not).
    /// libzstd's one-shot `ZSTD_compress` declares the content size in the frame header and
    /// `DecompressAndDecode` (C++ and port) rejects frames without it, so the header written
    /// by `ruzstd` is replaced by one that carries the size.
    pub fn compress_and_encode(file: &mut EmbeddedFile) -> EmbeddedFilesReturnCode {
        let compressed = ruzstd::encoding::compress_to_vec(
            file.decompressed_data.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );

        let Some(compressed) =
            zstd_with_content_size(&compressed, file.decompressed_data.len() as u64)
        else {
            return EmbeddedFilesReturnCode::OutOfMemory;
        };

        file.compressed_encoded_data = wx_base64_encode(&compressed);
        file.data_hash = mmh3_hex(&file.decompressed_data);

        EmbeddedFilesReturnCode::Ok
    }

    /// `EMBEDDED_FILES::DecompressAndDecode( aFile, aAllowEmptyHash = false )`: decode
    /// `compressed_encoded_data` into `decompressed_data` and check the stored checksum.
    ///
    /// The checksum may be the current MMH3 hash, the hash of the pre-fix `addDataV1` (files
    /// saved before the tail-byte alignment fix) or, for a 64 character checksum, the SHA-256
    /// of older file formats. On success `data_hash` becomes the current MMH3 hash; on a
    /// checksum error the decompressed data is cleared.
    pub fn decompress_and_decode(
        file: &mut EmbeddedFile,
        allow_empty_hash: bool,
    ) -> EmbeddedFilesReturnCode {
        let compressed_size = wx_base64_decoded_size(file.compressed_encoded_data.len());

        if compressed_size == 0 {
            // wxLogTrace: "Base64DecodedSize failed for file '%s' with size %zu"
            return EmbeddedFilesReturnCode::OutOfMemory;
        }

        // The return value from wxBase64Decode is the actual size of the decoded data avoiding
        // the modulo 4 padding of the base64 encoding
        let Some(compressed) = wx_base64_decode(&file.compressed_encoded_data) else {
            // wxCONV_FAILED: the C++ then reads the frame header of garbage; no frame content
            // size can be found there (CONTENTSIZE_ERROR -> OUT_OF_MEMORY).
            return EmbeddedFilesReturnCode::OutOfMemory;
        };

        let est_decompressed_size = zstd_get_frame_content_size(&compressed);

        if est_decompressed_size as f64 > 1e9 {
            // Limit to 1GB (also catches ZSTD_CONTENTSIZE_ERROR / _UNKNOWN, both > 1e9)
            return EmbeddedFilesReturnCode::OutOfMemory;
        }

        if est_decompressed_size == ZSTD_CONTENTSIZE_ERROR
            || est_decompressed_size == ZSTD_CONTENTSIZE_UNKNOWN
        {
            return EmbeddedFilesReturnCode::OutOfMemory;
        }

        let mut decompressed = vec![0u8; est_decompressed_size as usize];
        let mut decoder = ruzstd::decoding::FrameDecoder::new();

        let decompressed_size = match decoder.decode_all(&compressed, &mut decompressed) {
            Ok(size) => size,
            Err(_) => {
                // wxLogTrace: "ZSTD_decompress failed with error '%s'"
                file.decompressed_data.clear();
                return EmbeddedFilesReturnCode::OutOfMemory;
            }
        };

        decompressed.truncate(decompressed_size);
        file.decompressed_data = decompressed;

        let new_hash = mmh3_hex(&file.decompressed_data);

        if allow_empty_hash && file.data_hash.is_empty() {
            // wxLogTrace: "Generated new hash for '%s'"
        } else if file.data_hash.len() == 64 {
            // SHA-256 hash from older file formats
            let sha_hash = sha256_hex(&file.decompressed_data);

            if sha_hash != file.data_hash {
                // wxLogTrace: "Checksum error in embedded file '%s'"
                file.decompressed_data.clear();
                return EmbeddedFilesReturnCode::ChecksumError;
            }
        } else if new_hash != file.data_hash {
            // Current MMH3 hash didn't match. Try the V1 hash algorithm for files
            // saved before the tail-byte alignment fix.
            let mut v1hash = Mmh3Hash::new(EmbeddedFiles::seed());
            v1hash.add_data_v1(&file.decompressed_data);
            let v1_hash = v1hash.digest().to_string_hex();

            if v1_hash != file.data_hash {
                // wxLogTrace: "Checksum error in embedded file '%s'"
                file.decompressed_data.clear();
                return EmbeddedFilesReturnCode::ChecksumError;
            }
        }

        file.data_hash = new_hash;

        EmbeddedFilesReturnCode::Ok
    }

    /// `EMBEDDED_FILES::ComputeFileHash` on the file's bytes (no filesystem: the C++
    /// `FILE_NOT_FOUND` case belongs to the caller that reads the file): the current MMH3 hash
    /// that `AddFile` would store.
    pub fn compute_file_hash(data: &[u8]) -> String {
        mmh3_hex(data)
    }
}

// ---- the (embedded_files ...) reader ------------------------------------------------------

/// `EMBEDDED_FILES_PARSER::ParseEmbedded( EMBEDDED_FILES* aFiles )`: parse the body of an
/// `(embedded_files ...)` section into `files`. Call it with the lexer positioned on the
/// `embedded_files` keyword (the board / footprint parser hands its line reader over with
/// `DsnLexer::with_sub_lexer`, the C++ `SyncLineReaderWith` pair). Every file is decoded and
/// its checksum verified as it is completed; a failure is the C++ `PARSE_ERROR`
/// "Checksum error in embedded file <name>", which the board parser turns into a warning.
pub fn parse_embedded(
    lexer: &mut EmbeddedFilesLexer,
    files: &mut EmbeddedFiles,
) -> Result<(), IoError> {
    use kicad_sexpr::keywords::embedded_files_t::T;

    // embedded files are version 20240706 and uses also Bars as separator
    lexer.set_knows_bar(true);

    let mut file: Option<EmbeddedFile> = None;

    let mut token = lexer.next_tok()?;

    while token != T::T_RIGHT {
        if token != T::T_LEFT {
            return Err(lexer.expecting(T::T_LEFT));
        }

        token = lexer.next_tok()?;

        if token != T::T_file {
            return Err(lexer.expecting_str("file"));
        }

        if let Some(mut f) = file.take() {
            finish_parsed_file(lexer, &mut f)?;
            files.add_file(Arc::new(f));
        }

        token = lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(lexer.expecting(T::T_LEFT));
            }

            token = lexer.next_tok()?;

            match token {
                T::T_checksum => {
                    let Some(f) = file.as_mut() else {
                        return Err(lexer.expecting(T::T_name));
                    };

                    let tok = lexer.need_symbol_or_number()?;

                    if !EmbeddedFilesLexer::is_symbol(tok) {
                        return Err(lexer.expecting_str("checksum data"));
                    }

                    f.data_hash = lexer.cur_str().to_string();
                    lexer.need_right()?;
                }

                T::T_data => {
                    if file.is_none() {
                        return Err(lexer.expecting(T::T_name));
                    }

                    if let Err(e) = lexer.need_bar() {
                        // No data in the file -- due to bug in writer for 9.0.0
                        if lexer.cur_tok() == T::T_RIGHT {
                            token = lexer.next_tok()?;
                            continue;
                        }

                        return Err(e);
                    }

                    let mut tok = lexer.next_tok()?;
                    let mut data = String::with_capacity(1 << 17);

                    while tok != T::T_BAR {
                        if !EmbeddedFilesLexer::is_symbol(tok) {
                            return Err(lexer.expecting_str("base64 file data"));
                        }

                        data.push_str(lexer.cur_str());
                        tok = lexer.next_tok()?;
                    }

                    data.shrink_to_fit();

                    if let Some(f) = file.as_mut() {
                        f.compressed_encoded_data.push_str(&data);
                    }

                    lexer.need_right()?;
                }

                T::T_name => {
                    // wxLogTrace on a duplicate 'name' tag; the new name starts a new file.
                    lexer.need_symbol_or_number()?;

                    file = Some(EmbeddedFile {
                        name: lexer.cur_str().to_string(),
                        ..EmbeddedFile::default()
                    });

                    lexer.need_right()?;
                }

                T::T_type => {
                    if file.is_none() {
                        return Err(lexer.expecting(T::T_name));
                    }

                    let tok = lexer.next_tok()?;

                    let type_ = match tok {
                        T::T_datasheet => EmbeddedFileType::Datasheet,
                        T::T_font => EmbeddedFileType::Font,
                        T::T_model => EmbeddedFileType::Model,
                        T::T_worksheet => EmbeddedFileType::Worksheet,
                        T::T_other => EmbeddedFileType::Other,
                        _ => {
                            return Err(
                                lexer.expecting_str("datasheet, font, model, worksheet or other")
                            );
                        }
                    };

                    if let Some(f) = file.as_mut() {
                        f.type_ = type_;
                    }

                    lexer.need_right()?;
                }

                _ => return Err(lexer.expecting_str("checksum, data or name")),
            }

            token = lexer.next_tok()?;
        }

        token = lexer.next_tok()?;
    }

    // Add the last file in the collection
    if let Some(mut f) = file.take() {
        finish_parsed_file(lexer, &mut f)?;
        files.add_file(Arc::new(f));
    }

    Ok(())
}

/// The block of `ParseEmbedded` that completes a parsed file: decode it if it has data.
fn finish_parsed_file(lexer: &EmbeddedFilesLexer, file: &mut EmbeddedFile) -> Result<(), IoError> {
    if !file.compressed_encoded_data.is_empty()
        && EmbeddedFiles::decompress_and_decode(file, false) != EmbeddedFilesReturnCode::Ok
    {
        return Err(lexer.parse_error(format!("Checksum error in embedded file {}", file.name)));
    }

    Ok(())
}

// ---- helpers: hashes, base64, zstd frame header -------------------------------------------

/// `MMH3_HASH hash( EMBEDDED_FILES::Seed() ); hash.add( data ); hash.digest().ToString()`.
fn mmh3_hex(data: &[u8]) -> String {
    let mut hash = Mmh3Hash::new(EmbeddedFiles::seed());
    hash.add_bytes(data);
    hash.digest().to_string_hex()
}

/// `wxFileName( aName ).GetFullName()` on Unix: the part after the last `/`.
fn wx_full_name(name: &str) -> &str {
    match name.rfind('/') {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

/// `wxFileName::GetExt()` of a full name: the text after the last dot, empty if there is none
/// or the only dot starts the name (a hidden file such as `.config`).
fn wx_ext(full_name: &str) -> &str {
    match full_name.rfind('.') {
        Some(0) | None => "",
        Some(pos) => &full_name[pos + 1..],
    }
}

/// `wxBase64Decode( .., wxBase64DecodeMode_Strict )` semantics: standard alphabet, no white
/// space, the length must be a multiple of four with `=` padding only at the end; the unused
/// low bits of the last character are ignored as in wx.
const WX_BASE64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_encode_padding(true)
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireCanonical),
);

/// `wxBase64DecodedSize( aSrcLen )`: `3 * aSrcLen / 4`.
pub fn wx_base64_decoded_size(src_len: usize) -> usize {
    3 * src_len / 4
}

/// `wxBase64Decode( const wxString& aSrc )` (strict mode): `None` for `wxCONV_FAILED`.
/// Shared with the parsers of reference images (`wxBase64Decode` of the `(data ..)` text).
pub fn wx_base64_decode(src: &str) -> Option<Vec<u8>> {
    WX_BASE64.decode(src.as_bytes()).ok()
}

/// `wxBase64Encode( aSrc, aSrcLen )`: standard alphabet with `=` padding, no line breaks.
pub fn wx_base64_encode(data: &[u8]) -> String {
    WX_BASE64.encode(data)
}

/// `ZSTD_CONTENTSIZE_UNKNOWN` (`(0ULL - 1)`).
const ZSTD_CONTENTSIZE_UNKNOWN: u64 = u64::MAX;
/// `ZSTD_CONTENTSIZE_ERROR` (`(0ULL - 2)`).
const ZSTD_CONTENTSIZE_ERROR: u64 = u64::MAX - 1;
const ZSTD_MAGICNUMBER: u32 = 0xFD2F_B528;
const ZSTD_MAGIC_SKIPPABLE_START: u32 = 0x184D_2A50;
const ZSTD_MAGIC_SKIPPABLE_MASK: u32 = 0xFFFF_FFF0;
/// `ZSTD_WINDOWLOG_MAX` on a 64-bit target.
const ZSTD_WINDOWLOG_MAX: u32 = 31;

/// The fields of `ZSTD_frameHeader` the port needs.
struct ZstdFrameHeader {
    frame_content_size: u64,
    skippable: bool,
    header_size: usize,
    window_descriptor: Option<u8>,
    checksum_flag: bool,
    dict_id_bytes: usize,
}

/// Port of libzstd's `ZSTD_getFrameHeader` (`lib/decompress/zstd_decompress.c`, format
/// `ZSTD_f_zstd1`). `Err(())` is any non-zero return (an error or "need more input").
fn zstd_get_frame_header(src: &[u8]) -> Result<ZstdFrameHeader, ()> {
    const MIN_INPUT_SIZE: usize = 5;
    let read_le32 = |p: &[u8]| u32::from_le_bytes([p[0], p[1], p[2], p[3]]);

    if src.len() < MIN_INPUT_SIZE {
        return Err(());
    }

    if read_le32(src) != ZSTD_MAGICNUMBER {
        if (read_le32(src) & ZSTD_MAGIC_SKIPPABLE_MASK) == ZSTD_MAGIC_SKIPPABLE_START {
            if src.len() < 8 {
                return Err(());
            }

            return Ok(ZstdFrameHeader {
                frame_content_size: u64::from(read_le32(&src[4..])),
                skippable: true,
                header_size: 8,
                window_descriptor: None,
                checksum_flag: false,
                dict_id_bytes: 0,
            });
        }

        return Err(()); // prefix_unknown
    }

    const DID_FIELD_SIZE: [usize; 4] = [0, 1, 2, 4];
    const FCS_FIELD_SIZE: [usize; 4] = [0, 2, 4, 8];

    let fhd_byte = src[MIN_INPUT_SIZE - 1];
    let dict_id_size_code = (fhd_byte & 3) as usize;
    let checksum_flag = (fhd_byte >> 2) & 1 != 0;
    let single_segment = (fhd_byte >> 5) & 1 != 0;
    let fcs_id = (fhd_byte >> 6) as usize;

    // ZSTD_frameHeaderSize_internal
    let fhsize = MIN_INPUT_SIZE
        + usize::from(!single_segment)
        + DID_FIELD_SIZE[dict_id_size_code]
        + FCS_FIELD_SIZE[fcs_id]
        + usize::from(single_segment && fcs_id == 0);

    if src.len() < fhsize {
        return Err(());
    }

    if fhd_byte & 0x08 != 0 {
        return Err(()); // frameParameter_unsupported: reserved bits, must be zero
    }

    let mut pos = MIN_INPUT_SIZE;
    let mut window_descriptor = None;

    if !single_segment {
        let wl_byte = src[pos];
        pos += 1;
        let window_log = u32::from(wl_byte >> 3) + 10;

        if window_log > ZSTD_WINDOWLOG_MAX {
            return Err(()); // frameParameter_windowTooLarge
        }

        window_descriptor = Some(wl_byte);
    }

    pos += DID_FIELD_SIZE[dict_id_size_code];

    let frame_content_size = match fcs_id {
        0 => {
            if single_segment {
                u64::from(src[pos])
            } else {
                ZSTD_CONTENTSIZE_UNKNOWN
            }
        }
        1 => u64::from(u16::from_le_bytes([src[pos], src[pos + 1]])) + 256,
        2 => u64::from(read_le32(&src[pos..])),
        _ => u64::from_le_bytes(src[pos..pos + 8].try_into().map_err(|_| ())?),
    };

    Ok(ZstdFrameHeader {
        frame_content_size,
        skippable: false,
        header_size: fhsize,
        window_descriptor,
        checksum_flag,
        dict_id_bytes: DID_FIELD_SIZE[dict_id_size_code],
    })
}

/// Port of libzstd's `ZSTD_getFrameContentSize`.
fn zstd_get_frame_content_size(src: &[u8]) -> u64 {
    match zstd_get_frame_header(src) {
        Err(()) => ZSTD_CONTENTSIZE_ERROR,
        Ok(header) if header.skippable => 0,
        Ok(header) => header.frame_content_size,
    }
}

/// Rewrite the header of the single frame `frame` (as written by the `ruzstd` encoder, which
/// declares no content size) so that it declares `content_size`, as libzstd's one-shot
/// compression does. The window descriptor, checksum flag and blocks are kept. `None` if the
/// frame header cannot be read.
fn zstd_with_content_size(frame: &[u8], content_size: u64) -> Option<Vec<u8>> {
    let header = zstd_get_frame_header(frame).ok()?;

    if header.skippable || header.dict_id_bytes != 0 {
        return None;
    }

    let window_descriptor = header.window_descriptor?;

    if header.frame_content_size != ZSTD_CONTENTSIZE_UNKNOWN {
        return Some(frame.to_vec());
    }

    // Frame_Content_Size_flag 2 (4 bytes) or 3 (8 bytes); the window descriptor stays, so the
    // single segment flag stays clear.
    let (fcs_flag, fcs_bytes): (u8, Vec<u8>) = match u32::try_from(content_size) {
        Ok(size) => (2, size.to_le_bytes().to_vec()),
        Err(_) => (3, content_size.to_le_bytes().to_vec()),
    };

    let mut out = Vec::with_capacity(frame.len() + fcs_bytes.len());
    out.extend_from_slice(&ZSTD_MAGICNUMBER.to_le_bytes());
    out.push((fcs_flag << 6) | (u8::from(header.checksum_flag) << 2));
    out.push(window_descriptor);
    out.extend_from_slice(&fcs_bytes);
    out.extend_from_slice(&frame[header.header_size..]);
    Some(out)
}

/// Local shim for `picosha2::hash256_hex_string` (thirdparty/picosha2, MIT): the lower case
/// hex SHA-256 digest (FIPS 180-4), used only to verify checksums of embedded files saved by
/// old KiCad versions. Verified against the FIPS 180-2 test vectors in the tests below.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);

    while message.len() % 64 != 56 {
        message.push(0);
    }

    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        let mut w = [0u32; 64];

        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }

        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_sexpr::StringFormatter;

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

    fn file_with(name: &str, data: &[u8]) -> EmbeddedFile {
        EmbeddedFile {
            name: name.to_string(),
            decompressed_data: data.to_vec(),
            ..EmbeddedFile::default()
        }
    }

    /// `std::mt19937` (32-bit Mersenne Twister) as the C++ test seeds it.
    struct Mt19937 {
        mt: [u32; 624],
        index: usize,
    }

    impl Mt19937 {
        fn new(seed: u32) -> Self {
            let mut mt = [0u32; 624];
            mt[0] = seed;

            for i in 1..624 {
                mt[i] = 1812433253u32
                    .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                    .wrapping_add(i as u32);
            }

            Mt19937 { mt, index: 624 }
        }

        fn next(&mut self) -> u32 {
            if self.index >= 624 {
                for i in 0..624 {
                    let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % 624] & 0x7fff_ffff);
                    let mut v = self.mt[(i + 397) % 624] ^ (y >> 1);

                    if y & 1 != 0 {
                        v ^= 0x9908_b0df;
                    }

                    self.mt[i] = v;
                }

                self.index = 0;
            }

            let mut y = self.mt[self.index];
            self.index += 1;
            y ^= y >> 11;
            y ^= (y << 7) & 0x9d2c_5680;
            y ^= (y << 15) & 0xefc6_0000;
            y ^ (y >> 18)
        }
    }

    /// qa/tests/common/test_embedded_file_compress.cpp `CompressAndEncode_OK` and
    /// `DecompressAndDecode_OK` (small, large repetitive, sequential and mt19937 data).
    #[test]
    fn compress_decompress_ok() {
        let mut file = file_with("test_file", b"Hello, World!");
        file.data_hash = mmh3_hex(&file.decompressed_data);
        assert_eq!(
            EmbeddedFiles::compress_and_encode(&mut file),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut file, false),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(file.decompressed_data, b"Hello, World!");

        let large = b"Hello, World!".repeat(100000);
        let sequential: Vec<u8> = (0..100000).map(|i| (i % 256) as u8).collect();
        let mut rng = Mt19937::new(0);
        let random: Vec<u8> = (0..100000).map(|_| (rng.next() % 256) as u8).collect();

        for data in [large, sequential, random] {
            file.decompressed_data = data.clone();
            file.data_hash = mmh3_hex(&data);
            assert_eq!(
                EmbeddedFiles::compress_and_encode(&mut file),
                EmbeddedFilesReturnCode::Ok
            );
            file.decompressed_data.clear();
            assert_eq!(
                EmbeddedFiles::decompress_and_decode(&mut file, false),
                EmbeddedFilesReturnCode::Ok
            );
            assert_eq!(file.decompressed_data, data);
        }
    }

    /// The mt19937 reference value (C++ standard, [rand.predef]: the 10000th output of a
    /// default-seeded engine is 4123659995).
    #[test]
    fn mt19937_reference() {
        let mut rng = Mt19937::new(5489);
        let mut v = 0;

        for _ in 0..10000 {
            v = rng.next();
        }

        assert_eq!(v, 4123659995);
    }

    /// qa/tests/common/test_embedded_file_compress.cpp `DecompressAndDecode_ChecksumError`.
    #[test]
    fn decompress_checksum_error() {
        let mut file = file_with("test_file", b"Hello, World!");
        assert_eq!(
            EmbeddedFiles::compress_and_encode(&mut file),
            EmbeddedFilesReturnCode::Ok
        );

        // Modify the checksum
        file.data_hash.replace_range(0..1, "x");
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut file, false),
            EmbeddedFilesReturnCode::ChecksumError
        );
        assert!(file.decompressed_data.is_empty());

        // An empty checksum is an error unless allowed
        let mut file = file_with("test_file", b"Hello, World!");
        EmbeddedFiles::compress_and_encode(&mut file);
        let hash = file.data_hash.clone();
        file.data_hash.clear();
        let mut allowed = file.clone();
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut file, false),
            EmbeddedFilesReturnCode::ChecksumError
        );
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut allowed, true),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(allowed.data_hash, hash);
    }

    /// qa/tests/common/test_embedded_file_compress.cpp `ComputeFileHash_MatchesEmbeddedHash`,
    /// `ComputeFileHash_DifferentContent` (on bytes: the port has no filesystem) and the
    /// extension to type mapping of `AddFile( wxFileName, .. )`.
    #[test]
    fn compute_file_hash() {
        let data = b"Test file content for hash computation".to_vec();
        let computed = EmbeddedFiles::compute_file_hash(&data);
        assert!(!computed.is_empty());

        let mut files = EmbeddedFiles::default();
        let embedded = files
            .add_file_from_data("/tmp/kicad_embed_test/test.txt", data, false)
            .unwrap();
        assert_eq!(embedded.name, "test.txt");
        assert_eq!(computed, embedded.data_hash);
        assert!(embedded.is_valid);
        assert_eq!(embedded.type_, EmbeddedFileType::Other);

        assert_ne!(
            EmbeddedFiles::compute_file_hash(b"Content version 1"),
            EmbeddedFiles::compute_file_hash(b"Content version 2")
        );

        for (name, type_) in [
            ("a.step", EmbeddedFileType::Model),
            ("b.WRZ", EmbeddedFileType::Model),
            ("c.ttf", EmbeddedFileType::Font),
            ("d.woff2", EmbeddedFileType::Font),
            ("e.pdf", EmbeddedFileType::Datasheet),
            ("f.kicad_wks", EmbeddedFileType::Worksheet),
            ("g.txt", EmbeddedFileType::Other),
        ] {
            let f = files
                .add_file_from_data(name, vec![1, 2, 3], false)
                .unwrap();
            assert_eq!(f.type_, type_, "{name}");
        }

        // Not overwritten without aOverwrite
        let kept = files
            .add_file_from_data("dir/c.ttf", vec![9], false)
            .unwrap();
        assert_eq!(kept.decompressed_data, vec![1, 2, 3]);
        let replaced = files
            .add_file_from_data("dir/c.ttf", vec![9], true)
            .unwrap();
        assert_eq!(replaced.decompressed_data, vec![9]);
        assert!(files.has_file("other/dir/c.ttf"));

        assert_eq!(files.update_font_files(), ["c.ttf", "d.woff2"]);
        files.clear_embedded_fonts();
        assert!(!files.has_file("c.ttf") && files.has_file("a.step"));
    }

    /// qa/tests/common/test_embedded_file_compress.cpp `DecompressAndDecode_V1HashFallback`.
    #[test]
    fn v1_hash_fallback() {
        let test_v1_fallback = |data: &[u8]| {
            let mut file = file_with("v1_hash_test", data);

            // Hash with V1 algorithm
            let mut v1hash = Mmh3Hash::new(EmbeddedFiles::seed());
            v1hash.add_data_v1(data);
            file.data_hash = v1hash.digest().to_string_hex();

            // Verify V1 and current hashes differ for non-aligned data
            let current_hash = mmh3_hex(data);

            if data.len() % 16 != 0 {
                assert_ne!(file.data_hash, current_hash);
            }

            // As in C++, CompressAndEncode stores the current hash over the V1 one; the V1
            // fallback itself is checked explicitly below.
            assert_eq!(
                EmbeddedFiles::compress_and_encode(&mut file),
                EmbeddedFilesReturnCode::Ok
            );

            // DecompressAndDecode should succeed via V1 fallback
            assert_eq!(
                EmbeddedFiles::decompress_and_decode(&mut file, false),
                EmbeddedFilesReturnCode::Ok
            );

            // After successful decode, hash should be migrated to current format
            assert_eq!(file.data_hash, current_hash);

            // The fallback itself: a V1 checksum is accepted and migrated
            let mut v1 = file.clone();
            v1.data_hash = {
                let mut h = Mmh3Hash::new(EmbeddedFiles::seed());
                h.add_data_v1(data);
                h.digest().to_string_hex()
            };
            assert_eq!(
                EmbeddedFiles::decompress_and_decode(&mut v1, false),
                EmbeddedFilesReturnCode::Ok
            );
            assert_eq!(v1.data_hash, current_hash);
        };

        // 13 bytes (remaining=13, old padding rounds to 16, hashTail sees 0 tail bytes)
        test_v1_fallback(b"Hello, World!");
        // 17 bytes (remaining=1, old padding rounds to 4)
        test_v1_fallback(b"12345678901234567");
        // 31 bytes (remaining=15, old padding rounds to 16)
        test_v1_fallback(b"1234567890123456789012345678901");

        // 100000 * 16 bytes (no tail, hashes should match)
        let aligned = vec![b'x'; 100000 * 16];
        let mut v1h = Mmh3Hash::new(EmbeddedFiles::seed());
        v1h.add_data_v1(&aligned);
        assert_eq!(v1h.digest().to_string_hex(), mmh3_hex(&aligned));
    }

    /// qa/tests/common/test_embedded_file_compress.cpp `CopySharesEmbeddedFilePayloads`
    /// (issue 24345), `DeepCopyAllocatesIndependentPayloads`, `AssignSharedFromIsIdempotent`
    /// (issue 24998) and `AddFileReturnsTheStoredEntry` (issue 25362).
    #[test]
    fn sharing_and_add_file() {
        let payload = vec![b'x'; 1024 * 1024];
        let mut file = file_with("shared_model.step", &payload);
        file.type_ = EmbeddedFileType::Model;
        assert_eq!(
            EmbeddedFiles::compress_and_encode(&mut file),
            EmbeddedFilesReturnCode::Ok
        );

        let mut original = EmbeddedFiles::default();
        original.add_file(Arc::new(file));
        let original_file = original
            .get_embedded_file("shared_model.step")
            .unwrap()
            .clone();

        // Copy constructor shares
        let copy1 = original.clone();
        let copy1_file = copy1.get_embedded_file("shared_model.step").unwrap();
        assert!(Arc::ptr_eq(copy1_file, &original_file));

        // Assignment deep-copies
        let copy2 = original.deep_copy();
        let copy2_file = copy2.get_embedded_file("shared_model.step").unwrap();
        assert!(!Arc::ptr_eq(copy2_file, &original_file));
        assert_eq!(copy2_file.data_hash, original_file.data_hash);

        {
            let mut transient = original.clone();
            transient.clear_embedded_files();
            assert!(!transient.has_file("shared_model.step"));
        }

        assert!(original.has_file("shared_model.step"));
        assert!(copy1.has_file("shared_model.step"));
        assert!(copy2.has_file("shared_model.step"));

        let mut decoded = EmbeddedFile::clone(copy1_file);
        decoded.decompressed_data.clear();
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut decoded, false),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(decoded.decompressed_data, payload);

        // AssignSharedFrom
        let mut working = EmbeddedFiles::default();
        let mut wks = file_with("ARTIDIS-KiCAD_HEADER.kicad_wks", b"123");
        wks.type_ = EmbeddedFileType::Worksheet;
        EmbeddedFiles::compress_and_encode(&mut wks);
        working.add_file(Arc::new(wks));

        let mut target = EmbeddedFiles::default();
        target.assign_shared_from(&working, &BTreeSet::new());
        target.assign_shared_from(&working, &BTreeSet::new());
        assert!(!target.is_empty());
        assert!(target.has_file("ARTIDIS-KiCAD_HEADER.kicad_wks"));
        assert!(working.has_file("ARTIDIS-KiCAD_HEADER.kicad_wks"));

        let source = working.clone();
        working.assign_shared_from(&source, &BTreeSet::new());
        assert!(working.has_file("ARTIDIS-KiCAD_HEADER.kicad_wks"));
        let exclude = BTreeSet::from(["ARTIDIS-KiCAD_HEADER.kicad_wks".to_string()]);
        let source = working.clone();
        working.assign_shared_from(&source, &exclude);
        assert!(!working.has_file("ARTIDIS-KiCAD_HEADER.kicad_wks"));

        // AddFile keeps the first of two equally named files
        let mut files = EmbeddedFiles::default();
        let first = Arc::new(file_with("duplicate.step", b"kept"));
        let stored = files.add_file_stored(first.clone());
        assert!(Arc::ptr_eq(&stored, &first));
        let stored = files.add_file_stored(Arc::new(file_with("duplicate.step", b"dropped")));
        assert!(Arc::ptr_eq(&stored, &first));
        assert_eq!(files.embedded_file_map().len(), 1);

        let mut stored = EmbeddedFile::clone(&stored);
        assert_eq!(
            EmbeddedFiles::compress_and_encode(&mut stored),
            EmbeddedFilesReturnCode::Ok
        );
        assert!(stored.validate());
        assert_eq!(stored.decompressed_data, b"kept");

        assert_eq!(stored.get_link(), "kicad-embed://duplicate.step");
        files.remove_file("duplicate.step");
        assert!(files.is_empty());
    }

    /// SHA-256 shim against the FIPS 180-2 test vectors, and the legacy SHA-256 checksum path
    /// of `DecompressAndDecode` / `Validate_SHA256`.
    #[test]
    fn sha256_checksums() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );

        let mut file = file_with("old.bin", b"abc");
        EmbeddedFiles::compress_and_encode(&mut file);
        let current = file.data_hash.clone();
        file.data_hash = sha256_hex(b"abc");
        assert!(file.clone().validate_sha256());
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut file, false),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(file.data_hash, current);

        file.data_hash = sha256_hex(b"abd");
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut file, false),
            EmbeddedFilesReturnCode::ChecksumError
        );
    }

    /// The error paths of `DecompressAndDecode`: base64 too short, invalid base64, a frame
    /// without content size (C++ ZSTD_CONTENTSIZE_UNKNOWN), a truncated frame.
    #[test]
    fn decompress_errors() {
        let mut f = file_with("e", b"");
        f.compressed_encoded_data = "A".to_string();
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut f, true),
            EmbeddedFilesReturnCode::OutOfMemory
        );
        f.compressed_encoded_data = "AA AA".to_string();
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut f, true),
            EmbeddedFilesReturnCode::OutOfMemory
        );

        let raw = ruzstd::encoding::compress_to_vec(
            &b"no size"[..],
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        assert_eq!(zstd_get_frame_content_size(&raw), ZSTD_CONTENTSIZE_UNKNOWN);
        f.compressed_encoded_data = wx_base64_encode(&raw);
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut f, true),
            EmbeddedFilesReturnCode::OutOfMemory
        );

        let sized = zstd_with_content_size(&raw, 7).unwrap();
        assert_eq!(zstd_get_frame_content_size(&sized), 7);
        f.compressed_encoded_data = wx_base64_encode(&sized);
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut f, true),
            EmbeddedFilesReturnCode::Ok
        );
        assert_eq!(f.decompressed_data, b"no size");

        f.compressed_encoded_data = wx_base64_encode(&sized[..sized.len() - 3]);
        assert_eq!(
            EmbeddedFiles::decompress_and_decode(&mut f, true),
            EmbeddedFilesReturnCode::OutOfMemory
        );
    }

    /// `WriteEmbeddedFiles` output and its round trip through `ParseEmbedded`.
    #[test]
    fn write_and_parse_round_trip() {
        let mut files = EmbeddedFiles::default();
        files.add_file_from_data(
            "model.step",
            (0..2000u32).map(|i| (i * 7 % 251) as u8).collect(),
            false,
        );
        files.add_file_from_data("font.ttf", b"font bytes".to_vec(), false);
        files.add_file(Arc::new(file_with("empty.txt", b"")));

        let mut out = StringFormatter::new();
        files.write_embedded_files(&mut out, true).unwrap();
        let text = out.string().to_string();
        assert!(text.starts_with("(embedded_files (file (name \"font.ttf\")(type font)(data\n|"));
        assert!(!text.contains("empty.txt"));
        assert!(text.contains("\n\n "));

        let mut lexer = EmbeddedFilesLexer::new(text.clone(), "test");
        assert_eq!(
            lexer.next_tok().unwrap(),
            kicad_sexpr::keywords::embedded_files_t::T::T_LEFT
        );
        lexer.next_tok().unwrap();
        let mut parsed = EmbeddedFiles::default();
        parse_embedded(&mut lexer, &mut parsed).unwrap();
        assert_eq!(parsed.files.len(), 2);

        for (name, file) in &parsed.files {
            let orig = files.get_embedded_file(name).unwrap();
            assert_eq!(file.compressed_encoded_data, orig.compressed_encoded_data);
            assert_eq!(file.decompressed_data, orig.decompressed_data);
            assert_eq!(file.data_hash, orig.data_hash);
            assert_eq!(file.type_, orig.type_);
        }

        let mut out2 = StringFormatter::new();
        parsed.write_embedded_files(&mut out2, false).unwrap();
        assert_eq!(
            out2.string(),
            format!(
                "(embedded_files (file (name \"font.ttf\")(type font)(checksum \"{}\"))(file (name \"model.step\")(type model)(checksum \"{}\")))",
                parsed.files["font.ttf"].data_hash, parsed.files["model.step"].data_hash
            )
        );

        // A corrupted checksum is the C++ parse error
        let bad = text.replacen("(checksum \"", "(checksum \"0", 1);
        let mut lexer = EmbeddedFilesLexer::new(bad, "test");
        lexer.next_tok().unwrap();
        lexer.next_tok().unwrap();
        let err = parse_embedded(&mut lexer, &mut EmbeddedFiles::default()).unwrap_err();
        assert!(
            err.problem()
                .starts_with("Checksum error in embedded file font.ttf")
        );

        // "(data)" without bars (the 9.0.0 writer bug) is accepted
        let mut lexer = EmbeddedFilesLexer::new(
            "(embedded_files (file (name x.txt) (type other) (data) (checksum ABC)))",
            "test",
        );
        lexer.next_tok().unwrap();
        lexer.next_tok().unwrap();
        let mut parsed = EmbeddedFiles::default();
        parse_embedded(&mut lexer, &mut parsed).unwrap();
        assert_eq!(parsed.files["x.txt"].data_hash, "ABC");
        assert!(parsed.files["x.txt"].compressed_encoded_data.is_empty());
    }

    /// Replays tests/golden/m24_embed.txt (tests/oracle/m24_oracle.cpp `embed`): every
    /// `(embedded_files ...)` section of every corpus file that has one is parsed and decoded
    /// with the port; name, type, decoded size, migrated checksum, validity and payload size
    /// must match the reference build line for line. Also classifies the stored checksums.
    #[test]
    fn m24_oracle_embedded_files() {
        use kicad_sexpr::keywords::embedded_files_t::T;

        let golden = include_str!("../tests/golden/m24_embed.txt");
        let root = kicad_qa::kicad_source_root();
        let mut paths: Vec<&str> = Vec::new();

        for line in golden.lines() {
            let path = line.split('|').nth(1).unwrap();

            if paths.last() != Some(&path) {
                paths.push(path);
            }
        }

        let mut out = Vec::new();
        let (mut current, mut legacy, mut other) = (0, 0, 0);

        for path in &paths {
            let text = std::fs::read_to_string(root.join(path)).unwrap();

            // Stored checksums, classified against the decoded data below
            let mut stored: BTreeMap<String, String> = BTreeMap::new();
            {
                let mut lexer = EmbeddedFilesLexer::new(text.clone(), path);
                lexer.set_knows_bar(true);
                let mut name = String::new();

                while let Ok(tok) = lexer.next_tok() {
                    if tok == T::T_EOF {
                        break;
                    }

                    if tok == T::T_name && lexer.prev_tok() == T::T_LEFT {
                        lexer.next_tok().unwrap();
                        name = lexer.cur_str().to_string();
                    } else if tok == T::T_checksum && lexer.prev_tok() == T::T_LEFT {
                        lexer.next_tok().unwrap();
                        stored.insert(name.clone(), lexer.cur_str().to_string());
                    }
                }
            }

            let mut lexer = EmbeddedFilesLexer::new(text, path);
            let mut section = 0;

            loop {
                let tok = match lexer.next_tok() {
                    Ok(T::T_EOF) => break,
                    Ok(tok) => tok,
                    Err(_) => {
                        out.push(format!("embed|{path}|lexer-error"));
                        break;
                    }
                };

                if tok != T::T_embedded_files || lexer.prev_tok() != T::T_LEFT {
                    continue;
                }

                let mut files = EmbeddedFiles::default();

                if parse_embedded(&mut lexer, &mut files).is_err() {
                    out.push(format!("embed|{path}|{section}|error"));
                }

                for (name, file) in &files.files {
                    let mut f = EmbeddedFile::clone(file);
                    let valid = f.validate();
                    out.push(format!(
                        "embed|{path}|{section}|{name}|type={}|size={}|hash={}|valid={}|data={}",
                        file.type_ as i32,
                        file.decompressed_data.len(),
                        file.data_hash,
                        i32::from(valid),
                        file.compressed_encoded_data.len()
                    ));

                    match stored.get(name) {
                        Some(h) if *h == file.data_hash => current += 1,
                        Some(h) => {
                            let mut v1 = Mmh3Hash::new(EmbeddedFiles::seed());
                            v1.add_data_v1(&file.decompressed_data);
                            assert_eq!(*h, v1.digest().to_string_hex(), "{path} {name}");
                            legacy += 1;
                        }
                        None => other += 1,
                    }
                }

                section += 1;
            }
        }

        let expected: Vec<&str> = golden.lines().collect();
        let mismatches: Vec<_> = expected
            .iter()
            .zip(out.iter())
            .filter(|(a, b)| *a != b)
            .take(10)
            .collect();
        assert!(mismatches.is_empty(), "{mismatches:#?}");
        assert_eq!(expected.len(), out.len());
        eprintln!(
            "embedded files: {} decoded, {current} current MMH3, {legacy} legacy MMH3 (addDataV1), {other} unmatched",
            out.len()
        );
        assert_eq!(other, 0);
    }

    /// Files compressed by the port must load in KiCad: the frames (with the content size
    /// added to the `ruzstd` header) are decoded by the reference `EMBEDDED_FILES::
    /// DecompressAndDecode` through `m24_oracle decode` (built by
    /// tests/oracle/m24_build_oracle.sh into target/m24-oracle). Skipped without the oracle.
    #[test]
    fn m24_oracle_cpp_decodes_port_frames() {
        use std::io::Write;

        let oracle = kicad_qa::kicad_source_root().join("rust/target/m24-oracle/m24_oracle");

        if !oracle.exists() {
            return;
        }

        let mut rng = Mt19937::new(7);
        let datasets: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"x".to_vec(),
            b"Hello, World!".to_vec(),
            b"Hello, World!".repeat(100000),
            (0..300000).map(|_| (rng.next() % 256) as u8).collect(),
            (0..70000).map(|i| (i % 7) as u8).collect(),
        ];

        let mut input = String::new();
        let mut expected = Vec::new();

        for (i, data) in datasets.iter().enumerate() {
            let mut f = file_with(&format!("f{i}"), data);
            assert_eq!(
                EmbeddedFiles::compress_and_encode(&mut f),
                EmbeddedFilesReturnCode::Ok
            );
            input.push_str(&format!(
                "{} {} {}\n",
                f.name, f.compressed_encoded_data, f.data_hash
            ));
            expected.push(format!(
                "decode|{}|rc=0|size={}|hash={}",
                f.name,
                data.len(),
                f.data_hash
            ));
        }

        let mut child = std::process::Command::new(oracle)
            .arg("decode")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, expected);
    }
}
