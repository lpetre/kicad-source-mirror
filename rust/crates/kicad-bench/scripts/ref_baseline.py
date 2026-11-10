#!/usr/bin/env python3
# This program source code file is part of KiCad, a free EDA CAD application.
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Reference baseline of the performance phase (PERF-1, docs/PERF.md): wall-clock and peak RSS
# of the reference kicad-cli (docs/ORACLE.md) for the operations kicad-bench times.
#
#   ref_baseline.py [--reps N] [--warmup W] [--modes default,threads1,taskset0] BOARD...
#
# Modes: `default` (KiCad's thread pool with all cores), `threads1` (kicad_advanced
# MaximumThreads=1 in the private config home), `taskset0` (default pool pinned to core 0 with
# taskset -c 0). Every run works on a fresh copy of the board (and its .kicad_pro / .kicad_prl /
# .kicad_dru) in a scratch directory under rust/target/bench, as ORACLE.md requires; outputs are
# deleted after each run. Prints one TSV line per board x operation x mode:
# board, op, mode, n, min_ms, median_ms, max_ms, peak_rss_mb, exit code.

import os
import shutil
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.abspath(os.path.join(HERE, '..', '..', '..'))
CLI = os.environ.get('KICAD_ORACLE_CLI', '/home/user/kicad-oracle/bin/kicad-cli')
SCRATCH = os.path.join(RUST, 'target', 'bench', 'ref')

OPS = [
    ('startup', ['version']),
    ('upgrade', ['pcb', 'upgrade', '--force', '{b}']),
    ('drc', ['pcb', 'drc', '-o', '{o}/drc.rpt', '{b}']),
    ('drc_refill', ['pcb', 'drc', '--refill-zones', '-o', '{o}/drc.rpt', '{b}']),
    ('gerbers', ['pcb', 'export', 'gerbers', '-o', '{o}/', '{b}']),
    ('drill', ['pcb', 'export', 'drill', '-o', '{o}/', '{b}']),
]


def prepare(board, work):
    """Fresh copy of the board and its side files in `work`; returns the copied board path."""
    if os.path.exists(work):
        shutil.rmtree(work)
    os.makedirs(os.path.join(work, 'out'))
    stem = os.path.splitext(board)[0]
    for ext in ('.kicad_pcb', '.kicad_pro', '.kicad_prl', '.kicad_dru'):
        if os.path.exists(stem + ext):
            shutil.copy(stem + ext, work)
    return os.path.join(work, os.path.basename(board))


def run_once(cmd, env):
    """Wall-clock (ms), peak RSS (kB) and exit code of one run."""
    t0 = time.perf_counter()
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, status, ru = os.wait4(p.pid, 0)
    ms = (time.perf_counter() - t0) * 1e3
    p.returncode = os.waitstatus_to_exitcode(status)
    return ms, ru.ru_maxrss, p.returncode


def main():
    args = sys.argv[1:]
    reps, warmup, modes = 7, 1, ['default', 'threads1', 'taskset0']
    ops = [o[0] for o in OPS]
    boards = []
    while args:
        a = args.pop(0)
        if a == '--reps':
            reps = int(args.pop(0))
        elif a == '--warmup':
            warmup = int(args.pop(0))
        elif a == '--modes':
            modes = args.pop(0).split(',')
        elif a == '--ops':
            ops = args.pop(0).split(',')
        else:
            boards.append(os.path.abspath(a))

    homes = {}
    for mode in modes:
        home = os.path.join(SCRATCH, 'home-' + mode)
        cfg = os.path.join(home, '.config', 'kicad', '10.99')
        os.makedirs(cfg, exist_ok=True)
        with open(os.path.join(cfg, 'kicad_advanced'), 'w') as f:
            f.write('MaximumThreads=1\n' if mode == 'threads1' else '')
        homes[mode] = home

    print('board\top\tmode\tn\tmin_ms\tmedian_ms\tmax_ms\tpeak_rss_mb\texit')
    for board in boards:
        label = os.path.splitext(os.path.basename(board))[0]
        for op, argv in OPS:
            if op not in ops:
                continue
            for mode in modes:
                env = dict(os.environ, KICAD_ORACLE_HOME=homes[mode])
                prefix = ['taskset', '-c', '0'] if mode == 'taskset0' else []
                times, rss, code = [], 0, 0
                for i in range(warmup + reps):
                    work = os.path.join(SCRATCH, 'work')
                    b = prepare(board, work)
                    cmd = prefix + [CLI] + [x.format(b=b, o=os.path.join(work, 'out')) for x in argv]
                    ms, kb, code = run_once(cmd, env)
                    if i >= warmup:
                        times.append(ms)
                        rss = max(rss, kb)
                shutil.rmtree(os.path.join(SCRATCH, 'work'), ignore_errors=True)
                print(f'{label}\t{op}\t{mode}\t{len(times)}\t{min(times):.1f}\t'
                      f'{statistics.median(times):.1f}\t{max(times):.1f}\t{rss / 1024:.1f}\t{code}',
                      flush=True)


if __name__ == '__main__':
    main()
