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

//! Port of KiCad's expression evaluators.
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and this crate's
//! `PORT_STATUS.md`.
//!
//! * [`libeval_compiler`] (C++ namespace `LIBEVAL`: `include/libeval_compiler/libeval_compiler.h`,
//!   `common/libeval_compiler/libeval_compiler.cpp`, `common/libeval_compiler/grammar.lemon`) is
//!   the expression language of custom design rules (`.kicad_dru`): conditions such as
//!   `A.NetClass == 'HV' && B.Type == 'via'` and constraint values such as `0.2mm + 1mil`.
//!   An expression is compiled by a [`Compiler`] into microcode ([`Ucode`]) that is run on a
//!   small stack machine with a [`Context`].  The board specific part (what `A`, `B` and
//!   their properties and functions mean) is supplied by the caller through the
//!   [`UcodeBinder`] trait; the pcbnew binding itself lives in `kicad-drc`.
//! * [`numeric_evaluator`] (`include/libeval/numeric_evaluator.h`,
//!   `common/libeval/numeric_evaluator.cpp`, `common/libeval/grammar.lemon`) is the unit-aware
//!   calculator used by numeric input fields.
//!
//! Supporting modules: [`lemon`](crate::lemon) (the LALR(1) driver both grammars run on),
//! [`eda_units`] and [`string_utils`] (the few `common/` helpers the evaluators need).
//!
//! # Example
//!
//! ```
//! use kicad_libeval::{Compiler, Context, DefaultBinder, Ucode};
//!
//! let mut compiler = Compiler::new();
//! let mut code = Ucode::new(DefaultBinder);
//! let mut preflight = Context::new(());
//!
//! assert!(compiler.compile("3 * (7 + 8)", &mut code, &mut preflight));
//! assert!(!compiler.is_error_pending());
//!
//! let mut ctx = Context::new(());
//! let result = code.run(&mut ctx);
//! assert_eq!(ctx.value(result).as_double(&ctx), 45.0);
//! ```

mod crt;
pub mod eda_units;
mod lemon;
pub mod libeval_compiler;
pub mod numeric_evaluator;
pub mod string_utils;

pub use eda_units::EdaUnits;
pub use libeval_compiler::{
    CompilationStage, Compiler, Context, CustomValue, DefaultBinder, DefaultUnitResolver,
    ErrorStatus, FuncCallRef, NoCustomValue, NoVarRef, NodeId, TToken, TTokenValue, Tokenizer,
    TreeNode, Ucode, UcodeBinder, UnitResolver, Uop, UopValue, Value, ValueId, VarRef, VarType,
    escape_string_literal,
};
pub use numeric_evaluator::NumericEvaluator;
