#!/usr/bin/env python3
"""Golden generator of m23_generator_properties.txt (package M23, see the .sh next to it).

Extract (generated ...) blocks of an input board and its kicad-cli-upgraded copy.

Writes one golden record per generator found in BOTH files (matched by uuid):
  G|<board>|<uuid>|<type>
  I|<input block, whitespace collapsed>
  O|<output block, whitespace collapsed>
Generators missing from the output (dropped by the writer) get a D| line.
"""
import re
import sys


def blocks(text):
    out = []
    i = 0
    while True:
        j = text.find("(generated", i)
        if j < 0:
            return out
        # make sure the token is exactly "generated"
        k = j + len("(generated")
        if text[k] not in " \t\r\n)":
            i = k
            continue
        depth = 0
        in_str = False
        esc = False
        p = j
        while p < len(text):
            c = text[p]
            if in_str:
                if esc:
                    esc = False
                elif c == "\\":
                    esc = True
                elif c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            p += 1
        out.append(text[j : p + 1])
        i = p + 1


def collapse(b):
    # collapse whitespace outside strings
    res = []
    in_str = False
    esc = False
    ws = False
    for c in b:
        if in_str:
            res.append(c)
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == '"':
                in_str = False
            continue
        if c in " \t\r\n":
            ws = True
            continue
        if ws and res and res[-1] != "(" and c != ")":
            res.append(" ")
        ws = False
        res.append(c)
        if c == '"':
            in_str = True
    return "".join(res)


def strip(s):
    """Drop the (members ...) and (templates ...) lists: not properties."""
    for tok in ("(members", "(templates"):
        while True:
            j = s.find(tok)
            if j < 0:
                break
            d = 0
            p = j
            while True:
                c = s[p]
                if c == "(":
                    d += 1
                elif c == ")":
                    d -= 1
                    if d == 0:
                        break
                p += 1
            s = s[:j] + s[p + 1 :]
    return s


def uuid_of(b):
    m = re.search(r'\((?:uuid|id) "?([0-9a-fA-F-]+)"?\)', b)
    return m.group(1) if m else "?"


def type_of(b):
    m = re.search(r"\(type ([a-z_]+)\)", b)
    return m.group(1) if m else "?"


def main():
    name, fin, fout = sys.argv[1], sys.argv[2], sys.argv[3]
    bin_ = blocks(open(fin, encoding="utf-8").read())
    bout = {uuid_of(b): b for b in blocks(open(fout, encoding="utf-8").read())}
    for b in bin_:
        u = uuid_of(b)
        print(f"G|{name}|{u}|{type_of(b)}")
        print("I|" + strip(collapse(b)))
        if u in bout:
            print("O|" + strip(collapse(bout[u])))
        else:
            print("D|dropped")


main()
