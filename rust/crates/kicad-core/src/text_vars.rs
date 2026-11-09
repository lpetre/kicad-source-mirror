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

//! Port of the generic text-variable expansion of `common/common.cpp`: `ExpandTextVars` (the
//! overload taking a resolver callback), `ResolveTextVars`, `FinalizeTextVarExpansion` and
//! `NormalizeFilePathForTextVars`, plus `enum RESOLUTION_CONTEXT` (`include/common.h`).
//!
//! `${VAR}` references are handed to a resolver callback; `@{expr}` math expressions are kept
//! (with nested variables expanded) for a separate evaluator. KiCad evaluates them with
//! `EXPRESSION_EVALUATOR` (`common/text_eval/`), which is not part of this crate: callers pass
//! an evaluator in [`TextVarOptions`]; without one the expressions are left as they are.
//! Escaped references `\${..}` / `\@{..}` become `<<<ESC_DOLLAR:..}` / `<<<ESC_AT:..}` markers
//! that survive multi-pass expansion and are turned back by [`finalize_text_var_expansion`].
//! Indices count code points, as `wxString` does on Linux.

/// Port of `enum RESOLUTION_CONTEXT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionContext {
    /// `FOR_CANVAS`
    ForCanvas,
    /// `FOR_GUI`
    ForGui,
    /// `FOR_NETNAME`
    ForNetname,
    /// `FOR_ERC_DRC`: also resolves `ERC_WARNING`/`DRC_ERROR`... assertions.
    ForErcDrc,
    /// `INTERNAL`: keeps the escape on literal variable references (such as `\${foo}`).
    Internal,
    /// `RESOLVED`
    Resolved,
    /// `RAW_VALUE`: no expansion at all.
    RawValue,
    /// `FOR_URI`: a backslash is a path separator here, so no escapes are recognized.
    ForUri,
}

/// A text variable resolver (`std::function<bool( wxString* )>`): replaces the token by its
/// value and returns true, or returns false to leave the reference unresolved.
pub type TextVarResolver<'a> = dyn Fn(&mut String) -> bool + 'a;

/// Settings the C++ reads from globals.
#[derive(Clone, Copy)]
pub struct TextVarOptions<'a> {
    /// `ADVANCED_CFG::m_ResolveTextRecursionDepth` (default 6, range 0..10).
    pub max_depth: i32,
    /// `EXPRESSION_EVALUATOR::Evaluate`: evaluates the `@{...}` expressions of a text. `None`
    /// leaves them unevaluated.
    pub evaluator: Option<&'a dyn Fn(&str) -> String>,
}

impl Default for TextVarOptions<'_> {
    fn default() -> Self {
        Self {
            max_depth: 6,
            evaluator: None,
        }
    }
}

const ESC_DOLLAR: &str = "<<<ESC_DOLLAR:";
const ESC_AT: &str = "<<<ESC_AT:";

/// Port of `NormalizeFilePathForTextVars`: `\${` -> `/${` and `\@{` -> `/@{`, so that a
/// Windows path separator before a variable is not read as an escape.
pub fn normalize_file_path_for_text_vars(path: &str) -> String {
    path.replace("\\${", "/${").replace("\\@{", "/@{")
}

/// Port of `FinalizeTextVarExpansion`: escape markers back to `${` / `@{` for display contexts,
/// to the escaped `\${` / `\@{` for `FOR_ERC_DRC` / `INTERNAL` (and the others).
pub fn finalize_text_var_expansion(text: &mut String, context: ResolutionContext) {
    use ResolutionContext::*;

    if matches!(context, ForCanvas | ForGui | ForNetname | Resolved) {
        *text = text.replace(ESC_DOLLAR, "${").replace(ESC_AT, "@{");
    } else {
        *text = text.replace(ESC_DOLLAR, "\\${").replace(ESC_AT, "\\@{");
    }
}

fn starts_at(s: &[char], i: usize, pat: &str) -> bool {
    let n = pat.chars().count();
    i + n <= s.len() && s[i..i + n].iter().copied().eq(pat.chars())
}

/// Copies an escape marker's contents up to its matching `}` (the marker itself already
/// appended); returns the index after the copied text.
fn copy_marker(s: &[char], mut i: usize, out: &mut String) -> usize {
    let mut brace_count = 1;

    while i < s.len() && brace_count > 0 {
        if s[i] == '{' {
            brace_count += 1;
        } else if s[i] == '}' {
            brace_count -= 1;
        }

        out.push(s[i]);
        i += 1;
    }

    i
}

/// Port of `ExpandTextVars( aSource, aResolver, aContext, aDepth = 0 )` with the default
/// options (recursion depth 6, no math evaluator).
pub fn expand_text_vars(
    source: &str,
    resolver: Option<&TextVarResolver<'_>>,
    context: ResolutionContext,
) -> String {
    expand_text_vars_ext(source, resolver, context, 0, &TextVarOptions::default())
}

/// Port of `ExpandTextVars( const wxString&, const std::function<bool( wxString* )>*,
/// RESOLUTION_CONTEXT, int aDepth )`.
pub fn expand_text_vars_ext(
    source: &str,
    resolver: Option<&TextVarResolver<'_>>,
    context: ResolutionContext,
    depth: i32,
    opts: &TextVarOptions<'_>,
) -> String {
    if context == ResolutionContext::RawValue {
        return source.to_string();
    }

    let s: Vec<char> = source.chars().collect();
    let len = s.len();
    let mut newbuf = String::with_capacity(source.len());
    let max_depth = opts.max_depth;
    let mut i = 0usize;

    while i < len {
        // Skip over existing escape markers without processing their contents. This prevents
        // expanding ${} or @{} that are inside escaped expressions.
        if starts_at(&s, i, ESC_DOLLAR) {
            newbuf.push_str(ESC_DOLLAR);
            i = copy_marker(&s, i + 14, &mut newbuf);
            continue;
        } else if starts_at(&s, i, ESC_AT) {
            newbuf.push_str(ESC_AT);
            i = copy_marker(&s, i + 10, &mut newbuf);
            continue;
        }

        // Handle escaped variable references: \${...} or \@{...}. Replace with escape markers
        // that won't be expanded by multi-pass loops; they are converted back to ${...} or
        // @{...} only at the final display stage.
        if s[i] == '\\' && i + 1 < len && context != ResolutionContext::ForUri {
            if (s[i + 1] == '$' || s[i + 1] == '@') && i + 2 < len && s[i + 2] == '{' {
                newbuf.push_str(if s[i + 1] == '$' { ESC_DOLLAR } else { ESC_AT });
                i += 2;

                // Copy everything until the matching closing brace, including the brace.
                let mut brace_depth = 1;
                i += 1;

                while i < len && brace_depth > 0 {
                    if s[i] == '{' {
                        brace_depth += 1;
                    } else if s[i] == '}' {
                        brace_depth -= 1;
                    }

                    newbuf.push(s[i]);
                    i += 1;
                }

                continue;
            }
        }

        if (s[i] == '$' || s[i] == '@') && i + 1 < len && s[i + 1] == '{' {
            let is_math_expr = s[i] == '@';
            let mut token = String::new();
            // Track brace depth for nested expressions like @{${VAR}}
            let mut brace_depth = 1;
            i += 2;

            while i < len {
                // Skip over escape markers - don't count their braces. This prevents
                // <<<ESC_DOLLAR:X} from interfering with outer brace counting.
                if starts_at(&s, i, ESC_DOLLAR) {
                    token.push_str(ESC_DOLLAR);
                    i = copy_marker(&s, i + 14, &mut token);
                    continue;
                } else if starts_at(&s, i, ESC_AT) {
                    token.push_str(ESC_AT);
                    i = copy_marker(&s, i + 10, &mut token);
                    continue;
                }

                if s[i] == '{' {
                    brace_depth += 1;
                    token.push(s[i]);
                } else if s[i] == '}' {
                    brace_depth -= 1;

                    if brace_depth == 0 {
                        break; // Found the matching closing brace
                    }

                    token.push(s[i]);
                } else {
                    token.push(s[i]);
                }

                i += 1;
            }

            // The C++ for loop increments past the closing brace (or past the end).
            i += 1;

            if token.is_empty() {
                continue;
            }

            let nested = token.contains("${") || token.contains("@{");

            if is_math_expr {
                // For math expressions @{...}, recursively expand any nested ${...} variables
                // but DON'T evaluate the math - leave that for EvaluateText().
                if nested && depth < max_depth {
                    token = expand_text_vars_ext(&token, resolver, context, depth + 1, opts);
                }

                newbuf.push_str("@{");
                newbuf.push_str(&token);
                newbuf.push('}');
            } else {
                // Recursively expand nested variables BEFORE passing to resolver. This ensures
                // innermost variables are expanded first (standard evaluation order).
                if nested && depth < max_depth {
                    token = expand_text_vars_ext(&token, resolver, context, depth + 1, opts);

                    // Also evaluate math expressions after expanding variables
                    if token.contains("@{") {
                        if let Some(eval) = opts.evaluator {
                            token = eval(&token);
                        }
                    }
                }

                let assertion = token.starts_with("ERC_WARNING")
                    || token.starts_with("ERC_ERROR")
                    || token.starts_with("DRC_WARNING")
                    || token.starts_with("DRC_ERROR");

                if context != ResolutionContext::ForErcDrc && assertion {
                    // Only show user-defined warnings/errors during ERC/DRC
                } else if resolver.is_some_and(|r| r(&mut token)) {
                    newbuf.push_str(&token);
                } else {
                    // Token not resolved: leave the reference unchanged
                    newbuf.push_str("${");
                    newbuf.push_str(&token);
                    newbuf.push('}');
                }
            }
        } else {
            newbuf.push(s[i]);
            i += 1;
        }
    }

    newbuf
}

/// Port of `ResolveTextVars`: multi-pass resolution of nested variables such as
/// `${J601:UNIT(${ROW})}` and math expressions such as `@{${ROW}-1}`. `depth` is incremented
/// once per pass and limits the passes to `opts.max_depth`.
pub fn resolve_text_vars(
    source: &str,
    resolver: Option<&TextVarResolver<'_>>,
    depth: &mut i32,
    opts: &TextVarOptions<'_>,
) -> String {
    let mut text = source.to_string();

    while text.contains("${") || text.contains("@{") {
        *depth += 1;

        if *depth > opts.max_depth {
            break;
        }

        // Always expand when ${} or @{} present to handle escape sequences (\${} and \@{}).
        if text.contains("${") || text.contains("@{") {
            text = expand_text_vars_ext(&text, resolver, ResolutionContext::Internal, 0, opts);
        }

        // Only evaluate if there are @{} expressions present (not escape markers)
        if text.contains("@{") {
            if let Some(eval) = opts.evaluator {
                text = eval(&text);
            }
        }
    }

    text
}
