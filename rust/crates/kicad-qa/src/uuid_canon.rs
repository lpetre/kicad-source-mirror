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

//! UUID canonicaliser for differential tests of the board writer (new code, test support;
//! `docs/analysis/pcb-io.md` 9.3, D3).
//!
//! When KiCad loads a file in which some item has no UUID it generates a random one, so the
//! re-save of a legacy file by the oracle and by the port can only be compared after masking
//! those UUIDs. A UUID is kept when it is *deterministic*:
//!
//! * it occurs in the original file (compared case-insensitively: `KIID` parses either case
//!   and writes lower case), or
//! * it has the form `00000000-0000-0000-0000-0000xxxxxxxx`, which is what `KIID` makes of a
//!   legacy 8-digit time stamp (`(tstamp 5A0B1C2D)`) - a function of the file, not random.
//!
//! Every other UUID-shaped token (anywhere in the text, also inside `(path "/../..")` strings
//! and group member lists) is replaced by [`UUID_PLACEHOLDER`]. After masking, sibling blocks
//! whose only difference was a random UUID are identical, so their order (the oracle sorts some
//! items by UUID as the last tie-breaker) no longer shows; [`canonicalise_uuids_sorted`]
//! additionally sorts runs of masked sibling blocks for the cases where a comparator falls
//! through to the UUID although the blocks differ in fields the comparator ignores.

use std::collections::BTreeSet;

use crate::sexpr_text::{SexprNode, parse_lists};

/// What a random UUID is replaced by.
pub const UUID_PLACEHOLDER: &str = "<uuid>";

/// Prefix of the UUIDs `KIID` derives from legacy 8-digit time stamps.
const LEGACY_TIMESTAMP_PREFIX: &str = "00000000-0000-0000-0000-0000";

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// True when `bytes[i..i + 36]` has the `8-4-4-4-12` hex shape and is not part of a longer
/// hex/dash run.
fn uuid_at(bytes: &[u8], i: usize) -> bool {
    if i + 36 > bytes.len() {
        return false;
    }

    if i > 0 && (is_hex(bytes[i - 1]) || bytes[i - 1] == b'-') {
        return false;
    }

    if i + 36 < bytes.len() && (is_hex(bytes[i + 36]) || bytes[i + 36] == b'-') {
        return false;
    }

    (0..36).all(|k| match k {
        8 | 13 | 18 | 23 => bytes[i + k] == b'-',
        _ => is_hex(bytes[i + k]),
    })
}

/// All UUID-shaped tokens of `text`: (byte offset, the UUID).
pub fn find_uuids(text: &str) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i + 36 <= bytes.len() {
        if is_hex(bytes[i]) && uuid_at(bytes, i) {
            out.push((i, &text[i..i + 36]));
            i += 36;
        } else {
            i += 1;
        }
    }

    out
}

/// The UUIDs that count as deterministic for `original` (lower case).
fn deterministic_set(original: &str) -> BTreeSet<String> {
    find_uuids(original)
        .into_iter()
        .map(|(_, u)| u.to_ascii_lowercase())
        .collect()
}

/// Replace every UUID of `text` that is not deterministic for `original` (see the module
/// comment) by [`UUID_PLACEHOLDER`].
pub fn canonicalise_uuids(text: &str, original: &str) -> String {
    let keep = deterministic_set(original);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;

    for (at, uuid) in find_uuids(text) {
        let lower = uuid.to_ascii_lowercase();

        if keep.contains(&lower) || lower.starts_with(LEGACY_TIMESTAMP_PREFIX) {
            continue;
        }

        out.push_str(&text[pos..at]);
        out.push_str(UUID_PLACEHOLDER);
        pos = at + 36;
    }

    out.push_str(&text[pos..]);
    out
}

/// The number of UUIDs of `text` that [`canonicalise_uuids`] would mask.
pub fn count_random_uuids(text: &str, original: &str) -> usize {
    let keep = deterministic_set(original);

    find_uuids(text)
        .into_iter()
        .filter(|(_, u)| {
            let lower = u.to_ascii_lowercase();
            !keep.contains(&lower) && !lower.starts_with(LEGACY_TIMESTAMP_PREFIX)
        })
        .count()
}

/// Render `node` (a list of `text`) with the runs of its children sorted (bottom-up).
fn render_sorted(text: &str, node: &SexprNode) -> String {
    let mut out = String::with_capacity(node.end + 1 - node.start);
    out.push_str(&render_children_sorted(
        text,
        node.start,
        node.end + 1,
        &node.children,
    ));
    out
}

/// Render `text[from..to]` with `children` (lists inside that range) rendered by
/// [`render_sorted`], and every maximal run of consecutive children that have the same keyword,
/// contain the placeholder and are separated by blanks only, sorted by their rendered text.
fn render_children_sorted(text: &str, from: usize, to: usize, children: &[SexprNode]) -> String {
    let rendered: Vec<String> = children.iter().map(|c| render_sorted(text, c)).collect();
    let mut order: Vec<usize> = (0..children.len()).collect();
    let mut i = 0;

    while i < children.len() {
        let masked = |k: usize| rendered[k].contains(UUID_PLACEHOLDER);

        if !masked(i) {
            i += 1;
            continue;
        }

        let mut j = i + 1;

        while j < children.len()
            && masked(j)
            && children[j].keyword == children[i].keyword
            && text[children[j - 1].end + 1..children[j].start]
                .bytes()
                .all(|b| b.is_ascii_whitespace())
        {
            j += 1;
        }

        order[i..j].sort_by(|&a, &b| rendered[a].cmp(&rendered[b]));
        i = j;
    }

    let mut out = String::with_capacity(to - from);
    let mut pos = from;

    for (slot, child) in children.iter().enumerate() {
        out.push_str(&text[pos..child.start]);
        out.push_str(&rendered[order[slot]]);
        pos = child.end + 1;
    }

    out.push_str(&text[pos..to]);
    out
}

/// [`canonicalise_uuids`], then sort every run of consecutive sibling blocks (same keyword,
/// blanks between them) that contain a masked UUID by their text, innermost lists first.
///
/// This removes the order the oracle derives from random UUIDs. It can also hide a real
/// ordering difference between such blocks, so use it only for files with masked UUIDs (D3).
pub fn canonicalise_uuids_sorted(text: &str, original: &str) -> String {
    let masked = canonicalise_uuids(text, original);
    let roots = parse_lists(&masked);
    render_children_sorted(&masked, 0, masked.len(), &roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0a1b2c3d-0000-4000-8000-00000000000a";
    const B: &str = "0a1b2c3d-0000-4000-8000-00000000000b";

    #[test]
    fn finds_uuid_shapes_only() {
        let text = format!(
            "(uuid \"{A}\") (path \"/{B}/{A}\") x{A}0 12345678-1234-1234-1234-1234567890123"
        );
        let found: Vec<&str> = find_uuids(&text).into_iter().map(|(_, u)| u).collect();
        assert_eq!(found, [A, B, A]);
    }

    #[test]
    fn masks_random_keeps_original_and_legacy() {
        let original = format!("(uuid \"{}\")", A.to_ascii_uppercase());
        let text = format!(
            "(uuid \"{A}\")\n(uuid \"{B}\")\n(uuid \"00000000-0000-0000-0000-00005a0b1c2d\")\n"
        );
        assert_eq!(
            canonicalise_uuids(&text, &original),
            format!(
                "(uuid \"{A}\")\n(uuid \"<uuid>\")\n(uuid \"00000000-0000-0000-0000-00005a0b1c2d\")\n"
            )
        );
        assert_eq!(count_random_uuids(&text, &original), 1);
    }

    #[test]
    fn sorts_masked_sibling_runs() {
        let x = "11111111-2222-3333-4444-555555555555";
        let y = "99999999-2222-3333-4444-555555555555";
        let text = format!(
            "(kicad_pcb\n\t(net 0)\n\t(segment (w 2) (uuid \"{x}\"))\n\t(segment (w 1) (uuid \"{y}\"))\n\t(net 1)\n)\n"
        );
        let canon = canonicalise_uuids_sorted(&text, "");
        assert_eq!(
            canon,
            "(kicad_pcb\n\t(net 0)\n\t(segment (w 1) (uuid \"<uuid>\"))\n\t(segment (w 2) (uuid \"<uuid>\"))\n\t(net 1)\n)\n"
        );

        // Unmasked blocks keep their order.
        assert_eq!(canonicalise_uuids_sorted(&text, &text), text);
    }
}
