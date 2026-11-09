#!/usr/bin/env python3
# This program source code file is part of KiCad, a free EDA CAD application.
#
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation, either version 3 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program.  If not, see <http://www.gnu.org/licenses/>.
"""Convert the parse tables of a Lemon-generated grammar.c into Rust source.

Usage: gen_tables.py <grammar.c> <grammar.h> <TOKEN_PREFIX>

Prints the token constants and the `YyTables` initialiser that are pasted into
`src/libeval_compiler/grammar.rs` and `src/numeric_evaluator/grammar.rs` between the
"BEGIN GENERATED TABLES" / "END GENERATED TABLES" markers.  The reduce actions below those
markers are ported by hand from the grammar file (they are C++ code in the original).
"""

import re
import sys


def strip_comments(text):
    return re.sub(r"/\*.*?\*/", "", text, flags=re.S)


def array(text, name):
    m = re.search(r"static const [\w ]+ " + name + r"\[\] = \{(.*?)\};", text, flags=re.S)
    if not m:
        raise SystemExit("array %s not found" % name)
    return [int(x) for x in re.findall(r"-?\d+", strip_comments(m.group(1)))]


def define(text, name):
    m = re.search(r"#define\s+" + name + r"\s+\(?(-?\d+)\)?", text)
    if not m:
        raise SystemExit("define %s not found" % name)
    return int(m.group(1))


def fmt(values, per_line=12):
    lines = []
    for i in range(0, len(values), per_line):
        lines.append("        " + ", ".join(str(v) for v in values[i:i + per_line]) + ",")
    return "\n".join(lines)


def main():
    c = open(sys.argv[1]).read()
    h = open(sys.argv[2]).read()
    prefix = sys.argv[3] if len(sys.argv) > 3 else ""

    print("// Token codes (grammar.h).")
    for name, value in re.findall(r"#define\s+(\w+)\s+(\d+)", h):
        print("pub(crate) const %s%s: i32 = %s;" % (prefix, name, value))

    print()
    print("// Rule names (yyRuleName[]), for reference:")
    m = re.search(r"static const char \*const yyRuleName\[\] = \{(.*?)\};", c, flags=re.S)
    for i, rule in enumerate(re.findall(r'"(.*?)"', m.group(1))):
        print("//   (%d) %s" % (i, rule))

    print()
    print("pub(crate) static TABLES: YyTables = YyTables {")
    for name in ["YYNOCODE", "YYSTACKDEPTH", "YYNSTATE", "YYNRULE", "YYNRULE_WITH_ACTION",
                 "YYNTOKEN", "YY_MAX_SHIFT", "YY_MIN_SHIFTREDUCE", "YY_MAX_SHIFTREDUCE",
                 "YY_ERROR_ACTION", "YY_ACCEPT_ACTION", "YY_NO_ACTION", "YY_MIN_REDUCE",
                 "YY_MAX_REDUCE", "YY_ACTTAB_COUNT", "YY_SHIFT_COUNT", "YY_REDUCE_COUNT"]:
        print("    %s: %d," % (name.lower(), define(c, name)))
    for name in ["yy_action", "yy_lookahead", "yy_shift_ofst", "yy_reduce_ofst", "yy_default"]:
        print("    %s: &[\n%s\n    ]," % (name, fmt(array(c, name))))
    print("    yy_rule_info_lhs: &[\n%s\n    ]," % fmt(array(c, "yyRuleInfoLhs")))
    print("    yy_rule_info_nrhs: &[\n%s\n    ]," % fmt(array(c, "yyRuleInfoNRhs")))
    print("};")


if __name__ == "__main__":
    main()
