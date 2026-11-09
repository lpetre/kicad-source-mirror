// This file is part of libeval, a simple math expression evaluator
//
// Copyright (C) 2007 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `LIBEVAL::CONTEXT` from `include/libeval_compiler/libeval_compiler.h` and
//! `common/libeval_compiler/libeval_compiler.cpp`.

use std::cell::RefCell;
use std::fmt;

use super::ucode::UcodeBinder;
use super::value::Value;

/// Handle of a [`Value`] owned by a [`Context`] (C++: a `VALUE*` handed out by
/// `CONTEXT::AllocValue` / `StoreValue`).
///
/// A handle is valid until the next [`Context::reset`] of the context that issued it; using
/// it afterwards, or with another context, is a programming error (the accessors panic or
/// return an unrelated value, where C++ would dereference a dangling pointer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ValueId(usize);

/// The error callback type of [`Context`] (C++:
/// `std::function<void( const wxString& aMessage, int aOffset )>`).
type ErrorCallback<'e> = Box<dyn FnMut(&str, i32) + 'e>;

/// Port of `class LIBEVAL::CONTEXT`: the evaluation stack, the owner of every [`Value`]
/// produced by a run, and the run-time error callback.
///
/// C++ clients subclass `CONTEXT` to add the data their variables and functions read
/// (`PCBEXPR_CONTEXT`: the two items under test, the layer, the constraint...).  Here that
/// data is the binder's [`UcodeBinder::Env`] value stored in the context and reached with
/// [`Context::env`] / [`Context::env_mut`]; the lifetime `'e` is whatever the environment
/// borrows (e.g. the board).
///
/// Ownership: C++ hands out raw `VALUE*` that stay valid until `Reset()` or destruction.
/// The Rust context owns the values in an arena and hands out [`ValueId`] handles with the
/// same validity; read a value with [`Context::value`], change it with
/// [`Context::value_mut`].
pub struct Context<'e, B: UcodeBinder> {
    env: B::Env<'e>,
    /// C++: `m_ownedValues` plus the inline buffer, which only exists to avoid allocations.
    owned_values: Vec<Value<'e, B>>,
    /// C++: `VALUE* m_stack[100]` and `m_stackPtr`.  The C++ array has no overflow check;
    /// this stack simply grows.
    stack: Vec<ValueId>,
    error_callback: RefCell<Option<ErrorCallback<'e>>>,
}

impl<'e, B: UcodeBinder> Default for Context<'e, B>
where
    B::Env<'e>: Default,
{
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<'e, B: UcodeBinder> Context<'e, B> {
    /// Port of `CONTEXT::CONTEXT()`; `env` is the client data (the members a C++ subclass
    /// would add).
    pub fn new(env: B::Env<'e>) -> Self {
        Context {
            env,
            owned_values: Vec::with_capacity(8),
            stack: Vec::new(),
            error_callback: RefCell::new(None),
        }
    }

    /// The client data.
    pub fn env(&self) -> &B::Env<'e> {
        &self.env
    }

    /// The client data, mutably.
    pub fn env_mut(&mut self) -> &mut B::Env<'e> {
        &mut self.env
    }

    /// Port of `CONTEXT::Reset()`.
    ///
    /// Release every value allocated by the previous evaluation and rewind the stack so the same
    /// CONTEXT can be reused for another Run() without being reconstructed.  This keeps the owned-
    /// value vector's buffer allocated, so a reused context performs zero heap allocation for the
    /// common evaluation.
    ///
    /// The C++ method is virtual so that subclasses rewind their own per-run state first;
    /// here the caller resets its [`Context::env_mut`] data itself.  Every [`ValueId`]
    /// issued before the call becomes invalid.
    pub fn reset(&mut self) {
        self.owned_values.clear();
        self.stack.clear();
        *self.error_callback.get_mut() = None;
    }

    /// Port of `CONTEXT::AllocValue()`: a new undefined value owned by the context.
    pub fn alloc_value(&mut self) -> ValueId {
        self.store_value(Value::new())
    }

    /// Port of `CONTEXT::StoreValue( VALUE* aValue )`: hand a value over to the context.
    pub fn store_value(&mut self, value: Value<'e, B>) -> ValueId {
        self.owned_values.push(value);
        ValueId(self.owned_values.len() - 1)
    }

    /// The value behind a handle (C++: dereferencing the `VALUE*`).
    ///
    /// # Panics
    ///
    /// Panics if `id` was not issued by this context since its last [`Context::reset`].
    pub fn value(&self, id: ValueId) -> &Value<'e, B> {
        &self.owned_values[id.0]
    }

    /// The value behind a handle, mutably.
    ///
    /// # Panics
    ///
    /// Panics if `id` was not issued by this context since its last [`Context::reset`].
    pub fn value_mut(&mut self, id: ValueId) -> &mut Value<'e, B> {
        &mut self.owned_values[id.0]
    }

    /// Port of `CONTEXT::Push( VALUE* v )`.
    pub fn push(&mut self, v: ValueId) {
        self.stack.push(v);
    }

    /// Port of `CONTEXT::Pop()`.  Popping an empty stack reports "Malformed expression" and
    /// yields a fresh undefined value.
    pub fn pop(&mut self) -> ValueId {
        match self.stack.pop() {
            Some(v) => v,
            None => {
                self.report_error("Malformed expression");
                self.alloc_value()
            }
        }
    }

    /// Port of `CONTEXT::Top()`: peek the top of the stack without popping (used by the
    /// short-circuit jumps).
    pub fn top(&mut self) -> ValueId {
        match self.stack.last() {
            Some(v) => *v,
            None => {
                self.report_error("Malformed expression");
                self.alloc_value()
            }
        }
    }

    /// Port of `CONTEXT::SP()`: the number of values on the stack.
    pub fn sp(&self) -> i32 {
        self.stack.len() as i32
    }

    /// Port of `CONTEXT::SetErrorCallback`.  `None` removes the callback (C++: assigning
    /// `nullptr`).
    ///
    /// The callback receives the message and an offset, which is always -1 for run-time
    /// errors.
    pub fn set_error_callback(&mut self, callback: Option<ErrorCallback<'e>>) {
        *self.error_callback.get_mut() = callback;
    }

    /// Port of `CONTEXT::HasErrorCallback()`.
    pub fn has_error_callback(&self) -> bool {
        match self.error_callback.try_borrow() {
            Ok(callback) => callback.is_some(),
            // Only reachable while the callback itself is running.
            Err(_) => true,
        }
    }

    /// Port of `CONTEXT::ReportError( const wxString& aErrorMsg )`.
    pub fn report_error(&self, error_msg: &str) {
        if let Ok(mut callback) = self.error_callback.try_borrow_mut() {
            if let Some(callback) = callback.as_mut() {
                callback(error_msg, -1);
            }
        }
    }
}

impl<'e, B: UcodeBinder> fmt::Debug for Context<'e, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("owned_values", &self.owned_values)
            .field("stack", &self.stack)
            .field("has_error_callback", &self.has_error_callback())
            .finish()
    }
}
