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

//! Output sink, value formatters, item walk and keys: the Rust side of
//! `rust/tools/pcb-dump/format.cpp` and `pcb_dump.h` (`docs/PCB_DUMP_FORMAT.md`).
//!
//! Every field is computed inside [`Out::emit`], which catches panics: a `port_pending!` panic
//! becomes a `pending|SECTION|KEY|FIELD=CXX::Name` line, any other panic a
//! `rust-error|SECTION|KEY|FIELD=message` line, and the dump goes on with the next field.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use kicad_core::{Kiid, Lset, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::{
    Box2I, EdaAngle, Shape, ShapeArc, ShapeLineChain, ShapePolySet, ShapeRef, Vector2I,
};

use crate::board::Board;
use crate::ids::ItemId;
use crate::item_ref::AnyItem;

/// The message prefix of `port_pending!` (`src/pending.rs`).
pub const PENDING_PREFIX: &str = "kicad-pcb: port pending: ";

/// Panics like `port_pending!` for a C++ getter the Rust model has no method for yet (the dump
/// reports it as pending, with the `(no Rust API)` suffix so the owning package knows that it
/// has to add the method and wire it up here).
pub(crate) fn no_api(cxx: &str) -> ! {
    panic!("{PENDING_PREFIX}{cxx} (no Rust API)")
}

/// 64-bit FNV-1a (`Fnv1a64`).
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &c in data {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}

/// What a caught panic means for the dump.
pub(crate) enum Failure {
    /// `port_pending!` (or [`no_api`]): the C++ name.
    Pending(String),
    /// Any other panic: its message.
    Error(String),
}

fn classify(payload: Box<dyn std::any::Any + Send>) -> Failure {
    let msg = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    };

    match msg.strip_prefix(PENDING_PREFIX) {
        Some(name) => Failure::Pending(name.to_string()),
        None => Failure::Error(msg),
    }
}

/// Runs `f`, catching a panic (the panic hook installed by [`SilentPanics`] keeps it quiet).
pub(crate) fn attempt<T>(f: impl FnOnce() -> T) -> Result<T, Failure> {
    catch_unwind(AssertUnwindSafe(f)).map_err(classify)
}

type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>;

/// Reference-counted silent panic hook (several dumps may run at once in tests): the first
/// guard saves the current hook and installs a silent one, the last one restores it.
static HOOK: Mutex<(usize, Option<PanicHook>)> = Mutex::new((0, None));

/// RAII guard silencing panic messages for the duration of a dump.
pub struct SilentPanics(());

impl SilentPanics {
    /// Install the silent hook (nested / concurrent guards share it).
    pub fn new() -> Self {
        let mut h = HOOK.lock().unwrap_or_else(|e| e.into_inner());

        if h.0 == 0 {
            h.1 = Some(std::panic::take_hook());
            std::panic::set_hook(Box::new(|_| {}));
        }

        h.0 += 1;
        SilentPanics(())
    }
}

impl Default for SilentPanics {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SilentPanics {
    fn drop(&mut self) {
        let mut h = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        h.0 -= 1;

        if h.0 == 0 {
            if let Some(prev) = h.1.take() {
                std::panic::set_hook(prev);
            }
        }
    }
}

/// Output sink (`DUMP_OUT`): collects lines; in hash mode values are replaced by their FNV-1a
/// hash. Also holds the item keys of the board being dumped (`s_keys`) and the UUIDs found in
/// the board file (`s_fileUuids`).
#[derive(Default)]
pub struct Out {
    pub text: String,
    pub hash: bool,
    /// Current section / key context.
    pub section: String,
    pub key: String,
    /// Prepended to every field name by `emit` (used by the defaults command).
    pub field_prefix: String,
    pub(crate) keys: HashMap<ItemId, String>,
    pub(crate) file_uuids: Option<BTreeSet<String>>,
}

impl Out {
    pub fn new(hash: bool) -> Self {
        Out {
            hash,
            ..Out::default()
        }
    }

    /// `DUMP_OUT::Line`.
    pub fn line(&mut self, section: &str, key: &str, field: &str, value: &str) {
        self.text.push_str(section);
        self.text.push('|');
        self.text.push_str(key);
        self.text.push('|');
        self.text.push_str(field);
        self.text.push('=');

        if self.hash {
            self.text
                .push_str(&format!("#{:016x}", fnv1a64(value.as_bytes())));
        } else {
            self.text.push_str(value);
        }

        self.text.push('\n');
    }

    /// A line of another kind (`error`, `pending`, `rust-error`): never hashed, newlines folded.
    fn tagged(&mut self, tag: &str, field: &str, msg: &str) {
        let msg: String = msg
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        self.text.push_str(&format!(
            "{tag}|{}|{}|{}{field}={msg}\n",
            self.section, self.key, self.field_prefix
        ));
    }

    /// Report a failure for `field`. `field` may list several fields separated by `|` and use
    /// `*` wildcards (`shape.*`, `*.F.Cu`, `*` = every field of the key): a failed condition
    /// stands for all the fields it controls (`compare.py` matches them with `fnmatch`).
    pub(crate) fn fail(&mut self, field: &str, failure: Failure) {
        for f in field.split('|') {
            match &failure {
                Failure::Pending(name) => self.tagged("pending", f, name),
                Failure::Error(msg) => self.tagged("rust-error", f, msg),
            }
        }
    }

    /// `EMIT( field, value )`: compute the value under a panic guard.
    pub fn emit(&mut self, field: &str, f: impl FnOnce() -> String) {
        match attempt(f) {
            Ok(v) => {
                let (s, k, full) = (
                    self.section.clone(),
                    self.key.clone(),
                    format!("{}{field}", self.field_prefix),
                );
                self.line(&s, &k, &full, &v);
            }
            Err(failure) => self.fail(field, failure),
        }
    }

    /// `Guard( what, fn )`: run a block of emits; a panic escaping it (from the control flow
    /// of the block, e.g. a pending `IsOnLayer` deciding which fields exist) is reported on one
    /// line for `what`.
    pub fn guard(&mut self, what: &str, f: impl FnOnce(&mut Out)) {
        let r = attempt(|| f(&mut *self));

        if let Err(failure) = r {
            self.fail(what, failure);
        }
    }

    /// Evaluates a value that steers the control flow of a section (a loop bound, a condition):
    /// on a panic the failure is reported for `what` and `None` returned.
    pub fn cond<T>(&mut self, what: &str, f: impl FnOnce() -> T) -> Option<T> {
        match attempt(f) {
            Ok(v) => Some(v),
            Err(failure) => {
                self.fail(what, failure);
                None
            }
        }
    }

    /// Run `f` with the section / key context set (`SCOPE`).
    pub fn scope(&mut self, section: &str, key: &str, f: impl FnOnce(&mut Out)) {
        let old_s = std::mem::replace(&mut self.section, section.to_string());
        let old_k = std::mem::replace(&mut self.key, key.to_string());
        f(self);
        self.section = old_s;
        self.key = old_k;
    }

    /// `UuidInFile`. Without a scanned file (defaults, in-memory boards) every UUID counts as
    /// not in the file, as for the C++ `defaults` command (empty `s_fileUuids`).
    pub fn uuid_in_file(&self, id: &Kiid) -> bool {
        self.file_uuids
            .as_ref()
            .is_some_and(|s| s.contains(&id.as_std_string()))
    }

    /// `ItemKey`: `null`, the key, or `unkeyed`.
    pub fn item_key(&self, id: Option<ItemId>) -> String {
        match id {
            None => "null".to_string(),
            Some(id) => self
                .keys
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "unkeyed".to_string()),
        }
    }
}

/// `EMIT( "field", value )` with the value expression evaluated under the panic guard.
macro_rules! emit {
    ($o:expr, $field:expr, $val:expr) => {
        $o.emit(&$field, || -> String { $val })
    };
}
pub(crate) use emit;

// ---- value formatters ---------------------------------------------------------------------

/// `V( int )` and the other integer overloads.
pub fn v_int(v: impl Into<i64>) -> String {
    v.into().to_string()
}

pub fn v_bool(v: bool) -> String {
    if v { "true" } else { "false" }.to_string()
}

/// `V( double )`: `%.17g`, `0` for both zeros.
pub fn v_f64(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }

    if x.is_nan() {
        return if x.is_sign_negative() { "-nan" } else { "nan" }.to_string();
    }

    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_string();
    }

    let e = format!("{x:.16e}");
    let (mant, exp) = e.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent value");
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if neg { "-" } else { "" };

    if !(-4..17).contains(&exp) {
        let mut m = format!("{}.{}", &digits[..1], &digits[1..]);
        strip_zeros(&mut m);
        let esign = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{m}e{esign}{:02}", exp.abs());
    }

    let mut s = if exp >= 0 {
        let int_len = (exp + 1) as usize;
        format!("{}.{}", &digits[..int_len], &digits[int_len..])
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    };
    strip_zeros(&mut s);
    format!("{sign}{s}")
}

fn strip_zeros(s: &mut String) {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }

        if s.ends_with('.') {
            s.pop();
        }
    }
}

/// `V( std::string )`: quoted, escaped.
pub fn v_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');

    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }

    out.push('"');
    out
}

/// `V( VECTOR2I )`.
pub fn v_vec(v: Vector2I) -> String {
    format!("{},{}", v.x, v.y)
}

/// `V( VECTOR2D )`.
pub fn v_vec_d(x: f64, y: f64) -> String {
    format!("{},{}", v_f64(x), v_f64(y))
}

/// `V( BOX2I )`: position and size as stored.
pub fn v_box(b: Box2I) -> String {
    let p = b.position();
    let s = b.size();
    format!("{},{},{},{}", p.x, p.y, s.x as i32, s.y as i32)
}

/// `V( EDA_ANGLE )`.
pub fn v_angle(a: EdaAngle) -> String {
    v_f64(a.as_degrees())
}

/// `V( LSET )`: 32 hex digits (128 bits, most significant first).
pub fn v_lset(s: &Lset) -> String {
    let size = s.size();
    let nbits = (std::cmp::max(PCB_LAYER_ID_COUNT as usize, size)).div_ceil(4) * 4;
    let mut r = String::with_capacity(nbits / 4);

    for nib in (0..nbits / 4).rev() {
        let mut v = 0;

        for b in (0..4).rev() {
            let bit = nib * 4 + b;
            v = v * 2 + usize::from(bit < size && s.test(bit));
        }

        r.push(char::from(b"0123456789abcdef"[v]));
    }

    r
}

pub fn v_kiid(k: &Kiid) -> String {
    k.as_std_string()
}

/// `VLayer`: canonical layer name.
pub fn v_layer(layer: PcbLayerId) -> String {
    let n = layer.as_i32();

    if layer == PcbLayerId::UndefinedLayer {
        return "UNDEFINED_LAYER".to_string();
    }

    if layer == PcbLayerId::UnselectedLayer {
        return "UNSELECTED_LAYER".to_string();
    }

    if !(0..PCB_LAYER_ID_COUNT).contains(&n) {
        return format!("?{n}");
    }

    let name = Lset::name(layer);

    if name.is_empty() {
        format!("?{n}")
    } else {
        name
    }
}

/// `VLayers`: `[F.Cu,B.Cu]`.
pub fn v_layers(layers: impl IntoIterator<Item = PcbLayerId>) -> String {
    let v: Vec<String> = layers.into_iter().map(v_layer).collect();
    format!("[{}]", v.join(","))
}

/// `VLayerNames`: the LSET as a layer-name list.
pub fn v_layer_names(s: &Lset) -> String {
    let n = std::cmp::min(s.size(), PCB_LAYER_ID_COUNT as usize);
    v_layers((0..n).filter(|&i| s.test(i)).map(|i| PcbLayerId(i as i32)))
}

/// `VPoints`.
pub fn v_points(pts: &[Vector2I]) -> String {
    let v: Vec<String> = pts.iter().map(|p| v_vec(*p)).collect();
    format!("[{}]", v.join(" "))
}

/// `VArc`.
pub fn v_arc(a: &ShapeArc) -> String {
    format!(
        "A({} {} {} w={})",
        v_vec(a.get_p0()),
        v_vec(a.get_arc_mid()),
        v_vec(a.get_p1()),
        a.get_width()
    )
}

/// `VChain`.
pub fn v_chain(c: &ShapeLineChain) -> String {
    let mut r = format!(
        "C({} w={} n={} {}",
        if c.is_closed() { "closed" } else { "open" },
        c.width(),
        c.point_count(),
        v_points(c.c_points())
    );

    if c.arc_count() > 0 {
        let arcs: Vec<String> = (0..c.arc_count()).map(|i| v_arc(c.arc(i))).collect();
        let shapes: Vec<String> = c
            .c_shapes()
            .iter()
            .map(|&(a, b)| {
                if b >= 0 {
                    format!("{a}/{b}")
                } else {
                    a.to_string()
                }
            })
            .collect();
        r.push_str(&format!(
            " arcs=[{}] shapes=[{}]",
            arcs.join(" "),
            shapes.join(" ")
        ));
    }

    r.push(')');
    r
}

/// `VPolySet`.
pub fn v_polyset(s: &ShapePolySet) -> String {
    let mut r = format!("P({}", s.outline_count());

    for i in 0..s.outline_count() {
        r.push_str(" {");
        r.push_str(&v_chain(s.c_outline(i)));

        for h in 0..s.hole_count(i) {
            r.push_str(" hole ");
            r.push_str(&v_chain(s.c_hole(i, h)));
        }

        r.push('}');
    }

    r.push(')');
    r
}

/// `VShape`; `None` = null shared_ptr.
pub fn v_shape(s: Option<&dyn Shape>) -> String {
    let Some(s) = s else {
        return "null".to_string();
    };

    match s.as_shape_ref() {
        ShapeRef::Rect(r) => format!(
            "SH_RECT(p={} size={} r={} w={})",
            v_vec(r.get_position()),
            v_vec(r.get_size()),
            r.get_radius(),
            r.get_width()
        ),
        ShapeRef::Segment(g) => format!(
            "SH_SEGMENT(a={} b={} w={})",
            v_vec(g.get_seg().a),
            v_vec(g.get_seg().b),
            g.get_width()
        ),
        ShapeRef::LineChain(c) => format!("SH_LINE_CHAIN({})", v_chain(c)),
        ShapeRef::Circle(c) => {
            format!("SH_CIRCLE(c={} r={})", v_vec(c.get_center()), c.get_radius())
        }
        ShapeRef::Simple(c) => format!("SH_SIMPLE({})", v_chain(c.vertices())),
        ShapeRef::PolySet(p) => format!("SH_POLY_SET({})", v_polyset(p)),
        ShapeRef::Compound(c) => {
            let mut r = format!("SH_COMPOUND({}", c.shapes().len());

            for sub in c.shapes() {
                r.push(' ');
                r.push_str(&v_shape(Some(sub.as_ref())));
            }

            r.push(')');
            r
        }
        ShapeRef::Arc(a) => format!("SH_ARC({})", v_arc(a)),
        ShapeRef::Ellipse(e) => format!(
            "SH_ELLIPSE(c={} major={} minor={} rot={} start={} end={} arc={})",
            v_vec(e.get_center()),
            e.get_major_radius(),
            e.get_minor_radius(),
            v_angle(e.get_rotation()),
            v_angle(e.get_start_angle()),
            v_angle(e.get_end_angle()),
            v_bool(e.is_arc())
        ),
        ShapeRef::Null(_) => "SH_NULL()".to_string(),
        ShapeRef::PolySetTriangle(_) => format!("{}(?)", s.type_name()),
    }
}

/// `VOpt` of an integer.
pub fn v_opt_int<T: Into<i64> + Copy>(v: Option<T>) -> String {
    v.map_or_else(|| "none".to_string(), v_int)
}

pub fn v_opt_f64(v: Option<f64>) -> String {
    v.map_or_else(|| "none".to_string(), v_f64)
}

pub fn v_opt_bool(v: Option<bool>) -> String {
    v.map_or_else(|| "none".to_string(), v_bool)
}

pub fn v_opt_enum<E: Debug>(v: Option<E>) -> String {
    v.map_or_else(|| "none".to_string(), |e| v_enum(e))
}

/// Explicit C++ enumerator names where the mechanical conversion of [`v_enum`] does not give
/// the magic_enum name (keyed by the Rust `Debug` text, which includes the type for
/// ambiguous variants via [`v_enum_in`]).
fn enum_override(rust: &str) -> Option<&'static str> {
    ENUM_OVERRIDES
        .iter()
        .find(|(r, _)| *r == rust)
        .map(|(_, c)| *c)
}

/// `(Rust Debug name or "CXX_TYPE::RustName", C++ enumerator)`.
const ENUM_OVERRIDES: &[(&str, &str)] = &[];

/// `VEnum`: the C++ enumerator name, derived from the Rust variant name (`PcbTraceT` ->
/// `PCB_TRACE_T`, `GrTextHAlignCenter` -> `GR_TEXT_H_ALIGN_CENTER`); a variant with data
/// keeps only its name.
pub fn v_enum<E: Debug>(e: E) -> String {
    let dbg = format!("{e:?}");
    let name = dbg.split(['(', ' ', '{']).next().unwrap_or("").to_string();

    if let Some(o) = enum_override(&name) {
        return o.to_string();
    }

    upper_snake(&name)
}

/// [`v_enum`] with a per-type override table (`type_name` is the C++ enum name, e.g.
/// `PAD_SHAPE`), for enumerators whose C++ spelling cannot be derived.
pub fn v_enum_in<E: Debug>(cxx_type: &str, e: E) -> String {
    let dbg = format!("{e:?}");
    let name = dbg.split(['(', ' ', '{']).next().unwrap_or("");

    if let Some(o) = enum_override(&format!("{cxx_type}::{name}")) {
        return o.to_string();
    }

    v_enum(e)
}

/// `CamelCase` -> `CAMEL_CASE`; a digit run is its own word only after a lowercase letter
/// (`In1Cu` -> `IN1_CU`).
pub fn upper_snake(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut r = String::with_capacity(s.len() + 4);

    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());

            if prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower)
            {
                r.push('_');
            }
        }

        r.push(c.to_ascii_uppercase());
    }

    r
}

// ---- item keys and walk -------------------------------------------------------------------

/// `ScanFileUuids`: the UUIDs written in a board file (36-char UUIDs and legacy tstamps).
pub fn scan_file_uuids(text: &[u8]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let is_hex = |c: u8| c.is_ascii_hexdigit();
    let mut i = 0;

    while i < text.len() {
        if !is_hex(text[i]) && text[i] != b'-' {
            i += 1;
            continue;
        }

        let mut j = i;

        while j < text.len() && (is_hex(text[j]) || text[j] == b'-') {
            j += 1;
        }

        let tok: String = text[i..j]
            .iter()
            .map(|c| char::from(c.to_ascii_lowercase()))
            .collect();
        let b = tok.as_bytes();

        if tok.len() == 36 && b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-' {
            set.insert(tok);
        } else if tok.len() <= 8 && !tok.contains('-') {
            set.insert(format!(
                "00000000-0000-0000-0000-{}{tok}",
                "0".repeat(12 - tok.len())
            ));
        }

        i = j;
    }

    set
}

/// `ForEachBoardItem`: every item of the board in dump order (PCB_DUMP_FORMAT.md section 3).
pub fn board_items(board: &Board) -> Vec<ItemId> {
    let mut out = Vec::new();

    fn visit(board: &Board, id: ItemId, out: &mut Vec<ItemId>) {
        out.push(id);

        match board.get(id) {
            Some(AnyItem::Footprint(_)) => {
                let fp = id.typed().expect("footprint id");

                for (c, _) in board.footprint_fields(fp) {
                    visit(board, c.into(), out);
                }

                for (c, _) in board.footprint_pads(fp) {
                    visit(board, c.into(), out);
                }

                for (c, _) in board.footprint_drawings(fp) {
                    visit(board, c, out);
                }

                for (c, _) in board.footprint_zones(fp) {
                    visit(board, c.into(), out);
                }

                for (c, _) in board.footprint_groups(fp) {
                    visit(board, c.into(), out);
                }

                for (c, _) in board.footprint_points(fp) {
                    visit(board, c.into(), out);
                }
            }
            Some(AnyItem::Table(_)) => {
                let t = id.typed().expect("table id");

                for (c, _) in board.table_cells(t) {
                    visit(board, c.into(), out);
                }
            }
            _ => {}
        }
    }

    for (id, _) in board.footprints() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.drawings() {
        visit(board, id, &mut out);
    }

    for (id, _) in board.tracks() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.zones() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.generators() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.groups() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.board_points() {
        visit(board, id.into(), &mut out);
    }

    for (id, _) in board.markers() {
        visit(board, id.into(), &mut out);
    }

    out
}

/// `InitItemKeys`.
pub fn init_item_keys(o: &mut Out, board: &Board) {
    o.keys.clear();
    let items = board_items(board);
    let mut counts: HashMap<Kiid, usize> = HashMap::new();

    for &id in &items {
        if let Some(item) = board.get(id) {
            *counts.entry(*item_uuid(item)).or_default() += 1;
        }
    }

    for (ordinal, &id) in items.iter().enumerate() {
        let Some(item) = board.get(id) else { continue };
        let uuid = *item_uuid(item);
        let key = if uuid == Kiid::nil() || counts[&uuid] > 1 || !o.uuid_in_file(&uuid) {
            format!("#{ordinal}")
        } else {
            v_kiid(&uuid)
        };
        o.keys.insert(id, key);
    }
}

fn item_uuid<'a>(item: AnyItem<'a>) -> &'a Kiid {
    match item {
        AnyItem::Net(_) => &kicad_core::kiid::NILUUID,
        other => &other.as_dyn().base().uuid,
    }
}

/// `CopperLayers`: the board's copper layers in stack order; 2 without a board.
pub fn copper_layers(copper_layer_count: Option<i32>) -> Vec<PcbLayerId> {
    Lset::all_cu_mask_n(copper_layer_count.unwrap_or(2))
        .cu_stack()
        .iter()
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_format_matches_printf_g17() {
        assert_eq!(v_f64(90.0), "90");
        assert_eq!(v_f64(0.25), "0.25");
        assert_eq!(v_f64(0.1), "0.10000000000000001");
        assert_eq!(v_f64(1e-7), "9.9999999999999995e-08");
        assert_eq!(v_f64(-0.0), "0");
        assert_eq!(v_f64(1.5e20), "1.5e+20");
        assert_eq!(v_f64(-45.0), "-45");
        assert_eq!(v_f64(1e16), "10000000000000000");
        assert_eq!(v_f64(1e17), "1e+17");
        assert_eq!(v_f64(0.0001), "0.0001");
        assert_eq!(v_f64(f64::INFINITY), "inf");
    }

    #[test]
    fn names_and_strings() {
        assert_eq!(upper_snake("PcbTraceT"), "PCB_TRACE_T");
        assert_eq!(upper_snake("GrTextHAlignCenter"), "GR_TEXT_H_ALIGN_CENTER");
        assert_eq!(upper_snake("Roundrect"), "ROUNDRECT");
        assert_eq!(v_str("a\"b\\\n\x01é"), "\"a\\\"b\\\\\\n\\x01é\"");
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn pending_and_errors_are_reported() {
        let _quiet = SilentPanics::new();
        let mut o = Out::new(false);
        o.scope("s", "k", |o| {
            emit!(o, "a", crate::port_pending!("X::Y"));
            emit!(o, "b", panic!("boom"));
            emit!(o, "c", v_int(1));
        });
        assert_eq!(o.text, "pending|s|k|a=X::Y\nrust-error|s|k|b=boom\ns|k|c=1\n");
    }
}
