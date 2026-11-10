#!/usr/bin/env python3
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Markdown summary tables for docs/PERF.md (PERF-1) from the TSV outputs of kicad-bench
# (bench-*.tsv), ref_baseline.py (reference and --tool rs) and wasm_bench.mjs.
#
#   summarize.py --bench A.tsv[,B.tsv] --ref ref.tsv --rs rs-e2e.tsv [--wasm wasm.tsv]

import sys


def read(paths):
    rows = []
    for p in paths.split(','):
        with open(p) as f:
            head = None
            for line in f:
                cols = line.rstrip('\n').split('\t')
                if head is None or cols[0] in ('commit', 'tool'):
                    head = cols
                    continue
                if len(cols) == len(head):
                    rows.append(dict(zip(head, cols)))
    return rows


def main():
    a = dict(zip(sys.argv[1::2], sys.argv[2::2]))
    bench = read(a['--bench'])
    ref = read(a['--ref']) if '--ref' in a else []
    rs = read(a['--rs']) if '--rs' in a else []
    wasm = read(a['--wasm']) if '--wasm' in a else []

    boards = []
    for r in bench:
        if r['board'] not in boards:
            boards.append(r['board'])

    def b(board, stage, col='median_ms'):
        for r in bench:
            if r['board'] == board and r['stage'] == stage:
                return r[col]
        return '-'

    def e(rows, board, op, mode='default', col='median_ms'):
        for r in rows:
            if r['board'] == board and r['op'] == op and r['mode'] == mode:
                return r[col]
        return '-'

    def w(board, stage):
        for r in wasm:
            if r['board'] == board and r['stage'] == stage:
                return r['median_ms']
        return '-'

    print('#### Stage medians, native port (kicad-bench, ms; peak RSS of the stage in MB)\n')
    stages = ['parse', 'load.BuildConnectivity', 'load_total', 'drc', 'zone_refill',
              'drc_after_refill', 'gerbers', 'drill', 'write']
    print('| board | ' + ' | '.join(stages) + ' |')
    print('|---' * (len(stages) + 1) + '|')
    for bd in boards:
        cells = []
        for s in stages:
            m, p = b(bd, s), b(bd, s, 'peak_rss_mb')
            cells.append(m if p == '-' else f'{m} ({p})')
        print(f'| {bd} | ' + ' | '.join(cells) + ' |')

    print('\n#### Cold first run in a process minus warm median (ms): one-time initialisations\n')
    print('| board | ' + ' | '.join(stages) + ' |')
    print('|---' * (len(stages) + 1) + '|')
    for bd in boards:
        cells = []
        for s in stages:
            c, m = b(bd, s + ' [cold]', 'min_ms'), b(bd, s)
            try:
                cells.append(f'{float(c) - float(m):+.1f}')
            except ValueError:
                cells.append('-')
        print(f'| {bd} | ' + ' | '.join(cells) + ' |')

    if ref or rs:
        print('\n#### End-to-end process wall-clock, median ms (min) [peak RSS MB]\n')
        print('kicad-cli: `default` = thread pool on 4 cores, `1 thread` = kicad_advanced '
              'MaximumThreads=1, `core 0` = taskset -c 0. kicad-rs: same command, release build.\n')
        print('| board | op | kicad-cli default | kicad-cli 1 thread | kicad-cli core 0 | kicad-rs | '
              'kicad-rs / cli 1 thread |')
        print('|---|---|---|---|---|---|---|')
        ops = ['startup', 'upgrade', 'drc', 'drc_refill', 'gerbers', 'drill']
        for bd in boards:
            for op in ops:
                cells = []
                for rows, mode in ((ref, 'default'), (ref, 'threads1'), (ref, 'taskset0'), (rs, 'default')):
                    m = e(rows, bd, op, mode)
                    cells.append('-' if m == '-' else
                                 f'{m} ({e(rows, bd, op, mode, "min_ms")}) [{e(rows, bd, op, mode, "peak_rss_mb")}]')
                try:
                    ratio = f'{float(e(rs, bd, op)) / float(e(ref, bd, op, "threads1")):.2f}'
                except ValueError:
                    ratio = '-'
                print(f'| {bd} | {op} | ' + ' | '.join(cells) + f' | {ratio} |')

    if wasm:
        print('\n#### wasm (node 22, web/pkg-node, wasm-release) vs native, median ms\n')
        ws = ['load_total', 'drc', 'zone_refill', 'drc_after_refill', 'gerbers', 'drill', 'write']
        print('| board | stage | wasm | native | wasm / native |')
        print('|---|---|---|---|---|')
        wb = []
        for r in wasm:
            if r['board'] not in wb:
                wb.append(r['board'])
        for bd in wb:
            for s in ws:
                x, n = w(bd, s), b(bd, s)
                try:
                    ratio = f'{float(x) / float(n):.2f}'
                except ValueError:
                    ratio = '-'
                print(f'| {bd} | {s} | {x} | {n} | {ratio} |')


if __name__ == '__main__':
    main()
