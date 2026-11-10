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

//! The Rust re-save of a board (parse + format, the D2 `rust_resave` of `pcb-io.md` 9.3 until
//! PCBIO-15 adds the post-load initialisation) and its classification against the oracle's
//! re-save (new code, test support).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use kicad_pcb::io::{
    FormatOptions, ParseOptions, SkippedFormat, SkippedItem, format_board_report,
    parse_board_report,
};
use kicad_qa::sexpr_text::{FirstDifference, SexprNode, first_difference, keyword_path_at};
use kicad_qa::uuid_canon::canonicalise_uuids;

/// The prefix of a `port_pending!` panic of the board model.
pub const PENDING_PANIC: &str = "kicad-pcb: port pending: ";

/// Lines of context in first-difference excerpts.
pub const CONTEXT: usize = 3;

/// Install (once) a panic hook that keeps `port_pending!` panics quiet (they are expected and
/// reported by the harness) and prints every other panic as usual.
pub fn quiet_pending_panics() {
    static ONCE: Once = Once::new();

    ONCE.call_once(|| {
        let default = std::panic::take_hook();

        std::panic::set_hook(Box::new(move |info| {
            let msg = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();

            if !msg.starts_with(PENDING_PANIC) {
                default(info);
            }
        }));
    });
}

/// Where a re-save stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Parse,
    Format,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Parse => "parse",
            Stage::Format => "format",
        }
    }
}

/// The outcome of a Rust re-save.
#[derive(Clone, Debug)]
pub enum Resave {
    /// Parsed and written; with the constructs the staging stubs skipped on each side.
    Written {
        text: String,
        parse_skipped: Vec<SkippedItem>,
        write_skipped: Vec<SkippedFormat>,
        warnings: Vec<String>,
    },
    /// The parser returned an error (with staging, never a "port pending" one).
    ParseError(String),
    /// The writer returned an error.
    FormatError {
        message: String,
        parse_skipped: Vec<SkippedItem>,
    },
    /// A `port_pending!` model stub was reached: the C++ function it names.
    ModelPending {
        stage: Stage,
        function: String,
        parse_skipped: Vec<SkippedItem>,
    },
    /// Any other panic (a bug in the port).
    Panic { stage: Stage, message: String },
}

/// Run `f`, turning a panic into `Err(message)`.
fn guarded<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(non-string panic)".to_string())
    })
}

/// Parse `text` and format the board again (`allow_pending` on both sides when `staging`).
pub fn rust_resave(text: &str, source_name: &str, staging: bool) -> Resave {
    quiet_pending_panics();
    let parse_options = ParseOptions {
        allow_pending: staging,
    };
    let format_options = FormatOptions {
        allow_pending: staging,
        ..FormatOptions::default()
    };

    let (board, report) = match guarded(|| parse_board_report(text, source_name, &parse_options)) {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Resave::ParseError(e.what().to_string()),
        Err(m) => {
            return match m.strip_prefix(PENDING_PANIC) {
                Some(f) => Resave::ModelPending {
                    stage: Stage::Parse,
                    function: f.to_string(),
                    parse_skipped: Vec::new(),
                },
                None => Resave::Panic {
                    stage: Stage::Parse,
                    message: m,
                },
            };
        }
    };

    match guarded(|| format_board_report(&board, &format_options)) {
        Ok(Ok((text, wr))) => Resave::Written {
            text,
            parse_skipped: report.skipped,
            write_skipped: wr.skipped,
            warnings: report.warnings,
        },
        Ok(Err(e)) => Resave::FormatError {
            message: e.what().to_string(),
            parse_skipped: report.skipped,
        },
        Err(m) => match m.strip_prefix(PENDING_PANIC) {
            Some(f) => Resave::ModelPending {
                stage: Stage::Format,
                function: f.to_string(),
                parse_skipped: report.skipped,
            },
            None => Resave::Panic {
                stage: Stage::Format,
                message: m,
            },
        },
    }
}

/// Status of a board, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// `rust_resave( O1 ) == O2` byte for byte.
    Identical,
    /// Identical after masking UUIDs that do not occur in `O1` (the port generated UUIDs).
    IdenticalUuidMasked,
    /// Identical once the top-level blocks of the kinds a staging stub skipped are removed
    /// from both texts: everything the port already writes is right.
    IdenticalModuloPending,
    /// Differs even after removing the pending kinds.
    Differs,
    /// A `port_pending!` model stub stopped the re-save.
    ModelPending,
    /// The parser failed (the oracle loaded the board).
    ParseError,
    /// The writer failed.
    FormatError,
    /// Another panic.
    Panic,
    /// The oracle did not save the board.
    OracleError,
    /// On the skip list.
    Skipped,
}

impl Status {
    pub const ALL: [Status; 10] = [
        Status::Identical,
        Status::IdenticalUuidMasked,
        Status::IdenticalModuloPending,
        Status::Differs,
        Status::ModelPending,
        Status::ParseError,
        Status::FormatError,
        Status::Panic,
        Status::OracleError,
        Status::Skipped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Status::Identical => "identical",
            Status::IdenticalUuidMasked => "identical-uuid-masked",
            Status::IdenticalModuloPending => "identical-modulo-pending",
            Status::Differs => "differs",
            Status::ModelPending => "model-pending",
            Status::ParseError => "parse-error",
            Status::FormatError => "format-error",
            Status::Panic => "panic",
            Status::OracleError => "oracle-error",
            Status::Skipped => "skipped",
        }
    }

    pub fn from_label(label: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.label() == label)
    }

    /// Progress rank for regression checks (higher is better). `Differs` and `ModelPending`
    /// share a rank: a model stub being ported can turn one into the other.
    pub fn rank(self) -> u32 {
        match self {
            Status::Identical => 5,
            Status::IdenticalUuidMasked => 4,
            Status::IdenticalModuloPending => 3,
            Status::Differs | Status::ModelPending => 2,
            Status::ParseError | Status::FormatError | Status::Panic => 1,
            Status::OracleError | Status::Skipped => 0,
        }
    }
}

/// A pending construct and how often it was skipped.
#[derive(Clone, Debug, Default)]
pub struct PendingCount {
    pub package: String,
    pub count: usize,
}

/// The evaluation of one board.
#[derive(Clone, Debug)]
pub struct Evaluation {
    pub status: Status,
    /// First difference of the full texts (`None` when identical or not written).
    pub raw_diff: Option<FirstDifference>,
    /// First difference once the pending kinds are removed.
    pub masked_diff: Option<FirstDifference>,
    /// Parse-side skipped tokens (keyword -> package, count).
    pub parse_pending: BTreeMap<String, PendingCount>,
    /// Write-side skipped items.
    pub write_pending: BTreeMap<String, PendingCount>,
    /// The top-level kinds removed for the modulo-pending comparison.
    pub stripped_kinds: Vec<String>,
    /// The model stub that stopped the re-save, with its stage.
    pub model_pending: Option<(Stage, String)>,
    /// Parse / format error or panic message.
    pub error: Option<String>,
    /// The Rust text, when written.
    pub rust_text: Option<String>,
}

/// The board-file keywords a writer staging stub name stands for (`SkippedFormat::item`).
/// `None` for `shape`, which stands for every `gr_*` graphic except the texts.
fn writer_item_keywords(item: &str) -> Option<Vec<&str>> {
    Some(match item {
        "shape" => return None,
        "text" => vec!["gr_text"],
        "textbox" => vec!["gr_text_box"],
        "generator" => vec!["generated"],
        other => vec![other],
    })
}

/// True when the top-level block `node` belongs to a kind the writer skipped as `item`
/// (directly or nested inside it).
fn node_matches_writer_item(node: &SexprNode, item: &str) -> bool {
    match writer_item_keywords(item) {
        None => {
            let is_graphic =
                |k: &str| (k.starts_with("gr_") || k.starts_with("fp_")) && !k.contains("text");
            is_graphic(&node.keyword) || any_descendant(node, &is_graphic)
        }
        Some(kws) => kws
            .iter()
            .any(|k| node.keyword == *k || node.contains_keyword(k, true)),
    }
}

fn any_descendant(node: &SexprNode, pred: &dyn Fn(&str) -> bool) -> bool {
    node.children
        .iter()
        .any(|c| pred(&c.keyword) || any_descendant(c, pred))
}

/// Byte offset of the start of 1-based `line` in `text`.
fn line_offset(text: &str, line: i32) -> usize {
    if line <= 1 {
        return 0;
    }

    text.match_indices('\n')
        .nth((line - 2) as usize)
        .map_or(text.len(), |(i, _)| i + 1)
}

/// The top-level kinds to remove for the modulo-pending comparison: the top-level ancestor (in
/// `o1`) of every parse-skipped construct, and every top-level kind of `o2` or `rust` that is
/// or contains a writer-skipped item.
fn pending_kinds(
    o1: &str,
    o2: &str,
    rust: &str,
    parse_skipped: &[SkippedItem],
    write_skipped: &[SkippedFormat],
) -> Vec<String> {
    let mut kinds: Vec<String> = Vec::new();
    let mut add = |k: &str| {
        if !k.is_empty() && !kinds.iter().any(|x| x == k) {
            kinds.push(k.to_string());
        }
    };

    for s in parse_skipped {
        let path = keyword_path_at(o1, line_offset(o1, s.line));

        match path.get(1) {
            Some(top) => add(top),
            None => add(&s.token),
        }
    }

    let mut items: Vec<&str> = write_skipped.iter().map(|s| s.item.as_str()).collect();
    items.sort_unstable();
    items.dedup();

    if !items.is_empty() {
        for text in [o2, rust] {
            for root in kicad_qa::sexpr_text::parse_lists(text) {
                for node in &root.children {
                    if items.iter().any(|i| node_matches_writer_item(node, i)) {
                        add(&node.keyword);
                    }
                }
            }
        }
    }

    kinds.sort();
    kinds
}

fn count_pending<'a>(
    it: impl Iterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, PendingCount> {
    let mut map: BTreeMap<String, PendingCount> = BTreeMap::new();

    for (token, package) in it {
        let e = map.entry(token.to_string()).or_default();
        e.package = package.to_string();
        e.count += 1;
    }

    map
}

/// Re-save `o1` with staging and classify the result against the oracle's `o2`.
pub fn evaluate(o1: &str, o2: &str, source_name: &str) -> Evaluation {
    let mut ev = Evaluation {
        status: Status::Differs,
        raw_diff: None,
        masked_diff: None,
        parse_pending: BTreeMap::new(),
        write_pending: BTreeMap::new(),
        stripped_kinds: Vec::new(),
        model_pending: None,
        error: None,
        rust_text: None,
    };

    let set_parse_pending = |ev: &mut Evaluation, skipped: &[SkippedItem]| {
        ev.parse_pending = count_pending(skipped.iter().map(|s| (s.token.as_str(), s.package)));
    };

    match rust_resave(o1, source_name, true) {
        Resave::ParseError(e) => {
            ev.status = Status::ParseError;
            ev.error = Some(e);
        }
        Resave::FormatError {
            message,
            parse_skipped,
        } => {
            set_parse_pending(&mut ev, &parse_skipped);
            ev.status = Status::FormatError;
            ev.error = Some(message);
        }
        Resave::ModelPending {
            stage,
            function,
            parse_skipped,
        } => {
            set_parse_pending(&mut ev, &parse_skipped);
            ev.status = Status::ModelPending;
            ev.model_pending = Some((stage, function));
        }
        Resave::Panic { stage, message } => {
            ev.status = Status::Panic;
            ev.error = Some(format!("{} panicked: {message}", stage.label()));
        }
        Resave::Written {
            text,
            parse_skipped,
            write_skipped,
            warnings: _,
        } => {
            set_parse_pending(&mut ev, &parse_skipped);
            ev.write_pending =
                count_pending(write_skipped.iter().map(|s| (s.item.as_str(), s.package)));
            ev.raw_diff = first_difference(o2, &text, CONTEXT);

            if ev.raw_diff.is_none() {
                ev.status = Status::Identical;
            } else if canonicalise_uuids(&text, o1) == canonicalise_uuids(o2, o1) {
                ev.status = Status::IdenticalUuidMasked;
            } else {
                ev.stripped_kinds = pending_kinds(o1, o2, &text, &parse_skipped, &write_skipped);
                let strip = |t: &str| {
                    kicad_qa::sexpr_text::strip_top_level(t, |n| {
                        ev.stripped_kinds.iter().any(|k| *k == n.keyword)
                    })
                };
                ev.masked_diff = first_difference(&strip(o2), &strip(&text), CONTEXT);
                ev.status = if ev.masked_diff.is_none() {
                    Status::IdenticalModuloPending
                } else {
                    Status::Differs
                };
            }

            ev.rust_text = Some(text);
        }
    }

    ev
}

/// The packages a board still waits for: the packages of every pending construct, or the
/// model stub's name.
pub fn pending_summary(ev: &Evaluation) -> String {
    let mut by_package: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

    for (token, p) in ev.parse_pending.iter().chain(ev.write_pending.iter()) {
        let v = by_package.entry(p.package.as_str()).or_default();

        if !v.contains(&token.as_str()) {
            v.push(token);
        }
    }

    let mut parts: Vec<String> = by_package
        .into_iter()
        .map(|(p, toks)| format!("{p} ({})", toks.join(", ")))
        .collect();

    if let Some((stage, f)) = &ev.model_pending {
        parts.push(format!("model stub {f} ({})", stage.label()));
    }

    parts.join("; ")
}
