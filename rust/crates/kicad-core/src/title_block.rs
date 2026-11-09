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

//! Port of `include/title_block.h` and `common/title_block.cpp`: the title block texts of a
//! board / schematic / drawing sheet and their file format.
//!
//! The current date and time are not read from the clock (`PORTING.md` section 5): the
//! resolver takes them from a [`TitleBlockClock`] supplied by the caller.  Project text
//! variable expansion (`ExpandTextVars`) is left to the caller as well.

use kicad_sexpr::{IoError, OutputFormatter};

/// Index of the known texts (`TEXTS_IDX`).
const TITLE_IDX: usize = 0;
const DATE_IDX: usize = 1;
const REVISION_IDX: usize = 2;
const COMPANY_IDX: usize = 3;
/// idx of the first comment: one can have more than 1 comment
const COMMENT1_IDX: usize = 4;

/// Port of `class TITLE_BLOCK`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleBlock {
    tb_texts: Vec<String>,
}

/// The date and time texts `TITLE_BLOCK::GetCurrentDate`, `GetCurrentTimeHHMMSS` and
/// `GetCurrentTimeLocale` produce from the clock, supplied by the caller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleBlockClock {
    /// `GetCurrentDate()`: ISO date, e.g. `2024-05-17`
    pub current_date: String,
    /// `GetCurrentTimeHHMMSS()`: e.g. `19h23m42s`
    pub current_time_hhmmss: String,
    /// `GetCurrentTimeLocale()`: e.g. `19:23:42`
    pub current_time_locale: String,
}

impl TitleBlock {
    /// An empty title block.
    pub fn new() -> Self {
        Self::default()
    }

    fn set_tb_text(&mut self, idx: usize, text: &str) {
        if self.tb_texts.len() <= idx {
            self.tb_texts.resize(idx + 1, String::new());
        }

        self.tb_texts[idx] = text.to_string();
    }

    fn get_tb_text(&self, idx: usize) -> &str {
        self.tb_texts.get(idx).map_or("", |s| s.as_str())
    }

    /// `SetTitle`
    pub fn set_title(&mut self, title: &str) {
        self.set_tb_text(TITLE_IDX, title);
    }

    /// `GetTitle`
    pub fn title(&self) -> &str {
        self.get_tb_text(TITLE_IDX)
    }

    /// `SetDate`: the date as an unformatted string (the format is not checked).
    pub fn set_date(&mut self, date: &str) {
        self.set_tb_text(DATE_IDX, date);
    }

    /// `GetDate`
    pub fn date(&self) -> &str {
        self.get_tb_text(DATE_IDX)
    }

    /// `SetRevision`
    pub fn set_revision(&mut self, revision: &str) {
        self.set_tb_text(REVISION_IDX, revision);
    }

    /// `GetRevision`
    pub fn revision(&self) -> &str {
        self.get_tb_text(REVISION_IDX)
    }

    /// `SetCompany`
    pub fn set_company(&mut self, company: &str) {
        self.set_tb_text(COMPANY_IDX, company);
    }

    /// `GetCompany`
    pub fn company(&self) -> &str {
        self.get_tb_text(COMPANY_IDX)
    }

    /// `SetComment( aIdx, ... )`: 0 based comment index.
    pub fn set_comment(&mut self, idx: usize, comment: &str) {
        self.set_tb_text(idx + COMMENT1_IDX, comment);
    }

    /// `GetComment( aIdx )`
    pub fn comment(&self, idx: usize) -> &str {
        self.get_tb_text(idx + COMMENT1_IDX)
    }

    /// `Clear`
    pub fn clear(&mut self) {
        self.tb_texts.clear();
    }

    /// `GetStrings`
    pub fn strings(&self) -> &[String] {
        &self.tb_texts
    }

    /// Port of `TITLE_BLOCK::GetContextualTextVars`: append the text variable names a title
    /// block resolves (unless already present).
    pub fn get_contextual_text_vars(vars: &mut Vec<String>) {
        if !vars.iter().any(|v| v == "ISSUE_DATE") {
            for v in [
                "ISSUE_DATE",
                "CURRENT_DATE",
                "CURRENT_TIME_LOCALE",
                "CURRENT_TIME_HH_MM_SS",
                "REVISION",
                "TITLE",
                "COMPANY",
                "COMMENT1",
                "COMMENT2",
                "COMMENT3",
                "COMMENT4",
                "COMMENT5",
                "COMMENT6",
                "COMMENT7",
                "COMMENT8",
                "COMMENT9",
            ] {
                vars.push(v.to_string());
            }
        }
    }

    /// Port of `TITLE_BLOCK::TextVarResolver` without a project: resolve `token` in place;
    /// true if it was resolved.
    ///
    /// The C++ passes the result through `ExpandTextVars( ..., aProject )` when a project is
    /// given; callers with a project do that themselves.
    pub fn text_var_resolver(&self, token: &mut String, clock: &TitleBlockClock) -> bool {
        let original_token = token.clone();
        let mut token_updated = false;

        match token.as_str() {
            "ISSUE_DATE" => {
                *token = self.date().to_string();
                token_updated = true;
            }
            "CURRENT_DATE" => {
                *token = clock.current_date.clone();
                token_updated = true;
            }
            "CURRENT_TIME_HH_MM_SS" => {
                *token = clock.current_time_hhmmss.clone();
                token_updated = true;
            }
            "CURRENT_TIME_LOCALE" => {
                *token = clock.current_time_locale.clone();
                token_updated = true;
            }
            "REVISION" => {
                *token = self.revision().to_string();
                token_updated = true;
            }
            "TITLE" => {
                *token = self.title().to_string();
                token_updated = true;
            }
            "COMPANY" => {
                *token = self.company().to_string();
                token_updated = true;
            }
            _ => {
                let chars: Vec<char> = token.chars().collect();

                // aToken->Left( aToken->Len() - 1 ).IsSameAs( wxT( "COMMENT" ) )
                if chars.len() == 8 && chars[..7].iter().copied().eq("COMMENT".chars()) {
                    let c = chars[7];

                    if ('1'..='9').contains(&c) {
                        *token = self.comment((c as u8 - b'1') as usize).to_string();
                        token_updated = true;
                    }
                }
            }
        }

        if token_updated {
            if token == "CURRENT_DATE" {
                *token = clock.current_date.clone();
            }

            // This is the default fallback, so don't claim we resolved it
            if *token == format!("${{{original_token}}}") {
                return false;
            }

            return true;
        }

        false
    }

    /// Port of `TITLE_BLOCK::Format`: `(title_block (title ..)(date ..)(rev ..)(company ..)
    /// (comment N ..))`, nothing when every text is empty.
    pub fn format(&self, formatter: &mut dyn OutputFormatter) -> Result<(), IoError> {
        // Don't write the title block information if there is nothing to write.
        if self.tb_texts.iter().all(|t| t.is_empty()) {
            return Ok(());
        }

        formatter.print(format_args!("(title_block"))?;

        for (key, value) in [
            ("title", self.title()),
            ("date", self.date()),
            ("rev", self.revision()),
            ("company", self.company()),
        ] {
            if !value.is_empty() {
                let quoted = formatter.quotew(value);
                formatter.print(format_args!("({key} {quoted})"))?;
            }
        }

        for ii in 0..9 {
            if !self.comment(ii).is_empty() {
                let quoted = formatter.quotew(self.comment(ii));
                formatter.print(format_args!("(comment {} {quoted})", ii + 1))?;
            }
        }

        formatter.print(format_args!(")"))?;
        Ok(())
    }
}
