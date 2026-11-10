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

//! Port of `pcbnew/pcbexpr_functions.cpp`.
//!
//! Package DRC-21 ported the registration (`PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions`:
//! every name, signature and geometry flag). The function bodies are package DRC-22: until it
//! lands every function is a [`not_yet_ported`] stub (see its documentation).

use kicad_libeval::{Context, FuncCallRef};

use super::evaluator::{PcbExprBuiltinFunctions, PcbExprUcode, PcbExprVarRef};

/// A stand-in for a function body of package DRC-22.
///
/// It behaves like the C++ functions do on the paths that need no item: it pops `arg_count`
/// arguments, pushes the result 0 and, when the receiver resolves to an item (i.e. during a
/// real evaluation, never during the compile-time pre-flight call, which has no items),
/// reports the rule error "Function '<name>' is not yet ported (DRC-22)." through the context's
/// error callback. Compile results are therefore those of C++ except for the argument checks
/// the C++ bodies perform at pre-flight ("Missing ... argument", unknown layer names, ...).
fn not_yet_ported(name: &'static str, arg_count: usize) -> FuncCallRef<PcbExprUcode> {
    FuncCallRef::new(
        move |ctx: &mut Context<'_, PcbExprUcode>, vref: Option<&PcbExprVarRef>| {
            let item = vref.and_then(|v| v.object(ctx.env()));

            for _ in 0..arg_count {
                ctx.pop();
            }

            let result = ctx.alloc_value();
            ctx.value_mut(result).set_double(0.0);
            ctx.push(result);

            if item.is_some() && ctx.has_error_callback() {
                ctx.report_error(&format!("Function '{name}' is not yet ported (DRC-22)."));
            }
        },
    )
}

/// The registration table of `RegisterAllFunctions`: signature, geometry dependent.
const FUNCTIONS: &[(&str, bool)] = &[
    ("existsOnLayer('x')", false),
    ("isPlated()", false),
    // Geometry-dependent functions depend on item position/shape rather than item properties.
    // The third argument marks them so that CreateFuncCall() can detect them automatically.
    ("insideCourtyard('x') DEPRECATED", true),
    ("insideFrontCourtyard('x') DEPRECATED", true),
    ("insideBackCourtyard('x') DEPRECATED", true),
    ("intersectsCourtyard('x')", true),
    ("intersectsFrontCourtyard('x')", true),
    ("intersectsBackCourtyard('x')", true),
    ("insideArea('x') DEPRECATED", true),
    ("intersectsArea('x')", true),
    ("intersectsKeepout('x')", true),
    ("enclosedByArea('x')", true),
    ("isMicroVia()", false),
    ("isBlindVia()", false),
    ("isBuriedVia()", false),
    ("isBlindBuriedVia()", false),
    ("isStackedVia()", false),
    ("memberOf('x') DEPRECATED", false),
    ("memberOfGroup('x')", false),
    ("memberOfFootprint('x')", false),
    ("memberOfSheet('x')", false),
    ("memberOfSheetOrChildren('x')", false),
    ("fromTo('x','y')", false),
    ("isCoupledDiffPair()", false),
    ("inDiffPair('x')", false),
    ("inNetChain('x')", false),
    ("hasNetChain()", false),
    ("inNetChainClass('x')", false),
    ("getField('x')", false),
    ("hasNetclass('x')", false),
    ("hasExactNetclass('x')", false),
    ("hasComponentClass('x')", false),
    ("customProperty('x')", false),
    ("hasCustomProperty('x')", false),
];

/// Port of `PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions` (after its `m_funcs.clear()`).
pub fn register_all_functions(registry: &mut PcbExprBuiltinFunctions) {
    for &(signature, geometry_dependent) in FUNCTIONS {
        let name = signature.split('(').next().unwrap_or(signature);
        let arg_count = signature.matches('\'').count() / 2;

        registry.register_func(
            signature,
            not_yet_ported(name, arg_count),
            geometry_dependent,
        );
    }
}
