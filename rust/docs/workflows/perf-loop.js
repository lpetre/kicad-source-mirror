export const meta = {
  name: 'perf-loop',
  description: 'Speed up the Rust port without changing output: gate + quiet baseline, then rounds of plan, optimise, review, judge until two dry rounds',
  phases: [
    { title: 'Gate', detail: 'one script that proves outputs are unchanged; quiet baseline' },
    { title: 'Plan', detail: 'pick disjoint hotspots for the round' },
    { title: 'Optimise', detail: 'one optimisation per agent, disjoint files' },
    { title: 'Review', detail: 'can any kept diff change an output byte' },
    { title: 'Judge', detail: 'gate bit for bit, quiet benchmark, keep or revert, commit' },
  ],
}

const cfg = args || {}
const IMPL_MODEL = cfg.implModel   // model for implementation agents (pass it in args); reviewers inherit the session's
const INTERRUPTED = `\n\nIMPORTANT - this job was already started: a previous agent worked on it for about half an hour and was cut off before reporting. Its partial work is on disk in your files (uncommitted or in a work-in-progress checkpoint commit; scratch files may exist under rust/target). Start by taking stock: read what is there, build, find what is unfinished or half-written. Keep what is sound, finish the rest, and verify the whole job as if you had written all of it. Do not start over. The base commit of the round is in the planner's notes; 'git diff <base> -- <your files>' shows what the previous agent changed.`
const MAX_ROUNDS = cfg.maxRounds || 8
const TASKS_PER_ROUND = cfg.tasksPerRound || 3

const CTX = `You work on a Rust port of KiCad's PCB side (board load / save, connectivity, DRC, zone fill, Gerber / drill / SVG export, a CLI and a WebAssembly build). The C++ tree (read-only reference) is /home/user/kicad-source-mirror; the Rust workspace is /home/user/kicad-source-mirror/rust (run cargo from there, always with -p <crate>, one cargo command at a time; "Blocking waiting for file lock" is normal). Read /home/user/kicad-source-mirror/rust/PORTING.md sections 3, 7 and 9 (numeric rules, the shared-machine MEMORY and DISK rules - C++ builds only under flock /home/user/kicad-oracle/cxx-build.lock; no scratch file over 200 MB; disk is tight, check df before a release build - and the project-wide decisions) and /home/user/kicad-source-mirror/rust/docs/PERF.md (the benchmark instrument kicad-bench, the baseline tables, the ranked hotspot list, the history of earlier rounds).

THE RULE OF THIS PHASE: make the port faster WITHOUT changing a single output byte. Every result of the port is verified bit for bit against the reference KiCad build by differential oracles and goldens (board re-save, dumps of the board model, connectivity, zone fills vertex for vertex, DRC reports, Gerber / drill / SVG files). An optimisation must leave all of them identical. Pinned and not negotiable: the libstdc++-exact sort tie order (kicad_geom::stdsort), r-tree visit order, the vendored Clipper2 results, the order of floating-point operations, every iteration order that reaches an output (item order, zone order, report order), integer rounding (KiROUND) and the x86 cast rules. Wins come from avoiding repeated work, caching, fewer allocations and clones, better data layout, cheaper lookups, early exits that are provably equivalent - not from different algorithms that could change results. No unsafe code (the workspace forbids it). No new dependencies without saying why.`

const GATE_SCHEMA = {
  type: 'object',
  properties: {
    green: { type: 'boolean' },
    script: { type: 'string', description: 'path and usage of the gate script' },
    full_minutes: { type: 'number' },
    failures: { type: 'array', items: { type: 'string' }, description: 'tests red at the end, with cause and owner' },
    fixed: { type: 'array', items: { type: 'string' } },
    baseline_commit: { type: 'string' },
    baseline_summary: { type: 'string', description: 'the quiet baseline: key stage timings per board, port vs reference, max 300 words' },
    notes: { type: 'string' },
  },
  required: ['green', 'script', 'full_minutes', 'failures', 'fixed', 'baseline_commit', 'baseline_summary', 'notes'],
}

const PLAN_SCHEMA = {
  type: 'object',
  properties: {
    base_commit: { type: 'string', description: 'git rev-parse HEAD at planning time' },
    tasks: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'string', description: 'short id, e.g. R3-zone-knockout-cache' },
          title: { type: 'string' },
          target_stages: { type: 'array', items: { type: 'string' }, description: 'kicad-bench stage names expected to improve' },
          files: { type: 'array', items: { type: 'string' }, description: 'files the agent may edit (workspace-relative); disjoint from the other tasks' },
          approach: { type: 'string', description: 'what to do and why it cannot change output, with the evidence from the profile' },
          expected_gain: { type: 'string' },
          risk: { type: 'string' },
        },
        required: ['id', 'title', 'target_stages', 'files', 'approach', 'expected_gain', 'risk'],
      },
    },
    nothing_left: { type: 'boolean', description: 'true when no worthwhile task remains' },
    notes: { type: 'string' },
  },
  required: ['base_commit', 'tasks', 'nothing_left', 'notes'],
}

const IMPL_SCHEMA = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    done: { type: 'boolean', description: 'false if you made no change worth keeping (and left the files as they were)' },
    files_changed: { type: 'array', items: { type: 'string' }, description: 'every file you modified or created, workspace-relative' },
    summary: { type: 'string', description: 'what changed, max 200 words' },
    why_output_unchanged: { type: 'string', description: 'the argument, statement by statement where it matters' },
    touches_pinned: { type: 'boolean', description: 'true if the change is anywhere near a pinned behaviour (ordering, float op order, rounding, clipper, sort)' },
    measurement: { type: 'string', description: 'your own before / after numbers and how you took them' },
    tests_run: { type: 'string' },
  },
  required: ['id', 'done', 'files_changed', 'summary', 'why_output_unchanged', 'touches_pinned', 'measurement', 'tests_run'],
}

const REVIEW_SCHEMA = {
  type: 'object',
  properties: {
    verdicts: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'string' },
          ok: { type: 'boolean' },
          reason: { type: 'string' },
          must_check: { type: 'string', description: 'specific oracle or input the judge must run for this change' },
        },
        required: ['id', 'ok', 'reason', 'must_check'],
      },
    },
    notes: { type: 'string' },
  },
  required: ['verdicts', 'notes'],
}

const JUDGE_SCHEMA = {
  type: 'object',
  properties: {
    kept: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, gains: { type: 'string' } }, required: ['id', 'gains'] } },
    reverted: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, reason: { type: 'string' } }, required: ['id', 'reason'] } },
    gate_green: { type: 'boolean' },
    commit: { type: 'string', description: 'hash of the commit that holds the kept changes, or empty' },
    table: { type: 'string', description: 'stage timings before / after for the affected boards, compact' },
    notes: { type: 'string' },
  },
  required: ['kept', 'reverted', 'gate_green', 'commit', 'table', 'notes'],
}

phase('Gate')
const gate = await agent(`${CTX}

## Your job: the gate and the quiet baseline (you change no library code except to fix a test that is red for a stale reason)

1. Build rust/scripts/perf-gate.sh. It is the proof that an optimisation changed no output. Usage: "perf-gate.sh full" runs every test of every crate including the ignored corpus replays that compare against committed goldens or cached reference results (kicad-pcb io corpus and dump goldens; kicad-connectivity goldens; kicad-zone zone_fill_corpus and rev_zf_crafted; kicad-session upgrade_corpus, zf06_refill_save, drc12_corpus, gerbers_job, svg_job; kicad-drc provider replays, evalrules / engine / report oracles; kicad-fab plot, excellon, gerber drill, drill map corpora; kicad-cli e2e_cli replay with --include-ignored; kicad-geom oracle replays; kicad-plot, kicad-font, kicad-core, kicad-libeval, kicad-sexpr, kicad-zint) - find them by reading each crate's tests/ directory and PORT_STATUS.md, list them in the script with one line each, print PASS / FAIL and seconds per entry and a final summary line, exit non-zero on any failure, write a log under rust/target/perf/gate-<timestamp>.log. "perf-gate.sh changed <crate>..." runs the same entries restricted to the named crates and every crate that depends on them (compute the dependents from cargo metadata). Tests run in the dev profile (the workspace builds it with opt-level 1). Entries that need reference caches under rust/target (zone-corpus, drc12-cli, e2e/ref, io-corpus ...) must say so and fail loudly if the cache is missing rather than skip silently.
2. Run "perf-gate.sh full" once. Everything must be green before optimisation starts. For each red entry: find the cause; if the expectation is stale (a test that was not updated when a later package landed) fix the test; if it is a real difference from the reference, do NOT paper over it: report it in failures with the smallest reproducing input. Another agent (package WASM-API-1) may still be editing crates/kicad-session/src/board_api and crates/kicad-wasm: failures caused by its in-progress files are reported, not fixed.
3. Quiet baseline: docs/PERF.md says its first numbers were taken on a busy machine. Wait until the machine is quiet (uptime: 1-minute load below 1.0 apart from yourself; wait up to 20 minutes, then go ahead and say what the load was), build the release harness (cargo build --release -p kicad-bench; check df first) and re-run crates/kicad-bench/scripts/bench.sh for the small and medium sets and each large board once, plus crates/kicad-bench/scripts/ref_baseline.py for the reference kicad-cli on the same boards (default thread pool and single-thread modes). Write the tables into docs/PERF.md under "Quiet baseline at <commit>" (replace nothing: add), with a port-vs-reference ratio per stage and board. If PERF.md has no ranked hotspot list yet (package PERF-1 may still be writing it: wait for it up to 15 minutes), produce one from your quiet numbers plus a profile (perf if the kernel allows; otherwise the harness's stage timers and allocation counters): top functions for zone refill, DRC, connectivity build, Gerber export and parse on a medium board and on one large board.
Never run git commands that change the index or the working tree. Report the commit you measured (git rev-parse --short HEAD).`, { label: 'gate + quiet baseline', phase: 'Gate', schema: GATE_SCHEMA })

const history = []
let dry = 0
let round = 0
while (round < MAX_ROUNDS && dry < 2) {
  round += 1
  const hist = history.length ? history.map(h => `Round ${h.round}: kept [${h.kept.map(k => k.id + ' (' + k.gains + ')').join('; ')}] reverted [${h.reverted.map(k => k.id + ' (' + k.reason + ')').join('; ')}]`).join('\n') : 'none yet'

  const plan = await agent(`${CTX}

## Your job: plan round ${round} of the optimisation loop (you edit no code)

The gate script: ${gate ? gate.script : 'rust/scripts/perf-gate.sh'}. Quiet baseline summary: ${gate ? gate.baseline_summary : 'see docs/PERF.md'}
Earlier rounds:
${hist}

Read docs/PERF.md completely (baseline tables, port-vs-reference ratios, hotspot list, round history). Then look at the code behind the top hotspots yourself (read the hot functions and their callers; where the instrument is too coarse, run kicad-bench with its allocation counters or add a temporary timer in a scratch copy - not in the tree) and choose up to ${TASKS_PER_ROUND} optimisation tasks for this round:
- highest expected gain first, measured in wall-clock of the stages that dominate (zone refill, DRC run and its providers, connectivity build, Gerber export, parse, write) on the medium and large boards, single-threaded; prefer changes that help every board over ones that help one;
- each task names the files it may edit; the file sets of the tasks are disjoint, and no task edits a file under crates/kicad-session/src/board_api or crates/kicad-wasm (another agent works there) ; check with git status that the files are unmodified;
- each task states the approach concretely enough for an implementer (which repeated computation to cache and what invalidates the cache, which allocation to remove, which lookup to index) and WHY the output cannot change;
- do not plan anything that reorders floating-point operations, changes an iteration order that reaches an output, replaces a sort, or touches the vendored Clipper2; a cache must be keyed by everything the cached value depends on;
- options beyond micro-optimisation, when the profile supports them: the wasm build profile (opt-level 3 vs s, LTO, wasm-opt if it can be installed from npm as the binaryen package; measured with crates/kicad-bench/scripts/wasm_bench.mjs; output bytes of the port must stay identical natively and in wasm), and - only after single-thread hotspots are exhausted - an optional native "parallel" cargo feature (rayon) for stages whose results can be merged in a fixed order (per zone-layer fills inside one dependency level, DRC providers that only read) with wasm staying single-threaded.
Record git rev-parse HEAD as base_commit. If nothing worthwhile is left (every remaining hotspot is within noise, pinned, or already tried and reverted), return an empty task list with nothing_left true.`, { label: `plan round ${round}`, phase: 'Plan', schema: PLAN_SCHEMA })

  if (!plan || plan.nothing_left || !plan.tasks.length) { log(`round ${round}: planner found nothing left`); break }
  log(`round ${round}: ${plan.tasks.map(t => t.id).join(', ')}`)

  const impls = (await parallel(plan.tasks.map(t => () => agent(`${CTX}

## Your job: one optimisation (round ${round}, task ${t.id})

${t.title}
Target stages (kicad-bench names): ${t.target_stages.join(', ')}
Files you may edit - and ONLY these: ${t.files.join(', ')}
Approach proposed by the planner: ${t.approach}
Expected gain: ${t.expected_gain}. Risk: ${t.risk}

Do it properly: read the hot code and the C++ it was ported from (the port is statement by statement; comments name the C++ functions), confirm the hotspot with a measurement of your own (cargo build --release -p kicad-bench, then kicad-bench on one medium board for the target stage; other agents are compiling, so repeat and take the minimum), implement ONE coherent optimisation, and prove it changed nothing: run the tests and corpus replays of the crate you touched and of the crates that consume the changed code (${gate ? gate.script : 'rust/scripts/perf-gate.sh'} changed <crate> does exactly that; it takes a while - run it once at the end, not in a loop). If the tests show any difference, your change is wrong: fix it or undo it - never touch a golden or a test expectation. If your measurement shows no gain beyond noise on the target stage, undo the change completely (restore the files exactly) and report done=false with what you learned. Keep the code readable and keep the comments that tie it to the C++; add a comment where a cache or shortcut needs an invariant to stay correct. Do not edit docs/PERF.md (the judge does). Never run git commands that change the index or the working tree (git diff and git status are fine).${(cfg.interrupted || []).includes(t.id) ? INTERRUPTED : ''}`, { label: t.id, phase: 'Optimise', schema: IMPL_SCHEMA }).then(r => r ? { ...r, id: t.id, task: t } : null)))).filter(Boolean)

  const done = impls.filter(i => i.done && i.files_changed.length)
  if (!done.length) { dry += 1; history.push({ round, kept: [], reverted: impls.map(i => ({ id: i.id, reason: 'no gain: ' + i.summary.slice(0, 120) })) }); log(`round ${round}: no change survived implementation (dry ${dry})`); continue }

  const review = await agent(`${CTX}

## Your job: review the diffs of round ${round} before they are measured (you edit no code)

Base commit of the round: ${plan.base_commit}. For each change below read the full diff (git diff ${plan.base_commit} -- <files>) and the surrounding code, and decide one thing: can this change alter any output of the port for any input - a board file byte, a dumped model value, a net assignment, a zone fill vertex, a DRC entry or its order, a Gerber / drill / SVG byte? Look specifically for: a cache whose key misses an input (time stamp, layer, clearance, max error, footprint transform, connectivity state, rule resolver); a reused buffer that carries state between calls; an early exit that skips a side effect later code relies on; a changed iteration order (HashMap / HashSet iteration reaching a result, sort replaced or made unstable, dedup changed); floating-point operations reordered, fused or moved across a cast; integer arithmetic that now overflows or no longer wraps where the reference wraps; a clone replaced by a borrow that changes when a value is observed; behaviour that differs between native and wasm32. Tests passing is not an argument - the corpus does not cover every input. ok=false for anything you cannot convince yourself is equivalent, with the concrete input that would differ; for ok=true give the specific oracle the judge must run in must_check.

${done.map(i => `### ${i.id}: ${i.task.title}
files: ${i.files_changed.join(', ')}
what: ${i.summary}
implementer's argument: ${i.why_output_unchanged}
touches pinned behaviour: ${i.touches_pinned}`).join('\n\n')}`, { label: `review round ${round}`, phase: 'Review', schema: REVIEW_SCHEMA })

  const verdict = id => (review && review.verdicts.find(v => v.id === id)) || { ok: true, reason: 'no verdict', must_check: '' }

  const judge = await agent(`${CTX}

## Your job: judge round ${round} - keep or revert each change, prove the gate, record, commit

Base commit of the round: ${plan.base_commit} (the state before any change of this round). Gate script: ${gate ? gate.script : 'rust/scripts/perf-gate.sh'}.
Changes of this round with the reviewer's verdicts:
${done.map(i => `- ${i.id} | files: ${i.files_changed.join(', ')} | target stages: ${i.task.target_stages.join(', ')} | implementer measured: ${i.measurement} | reviewer: ${verdict(i.id).ok ? 'OK' : 'REJECTED'} - ${verdict(i.id).reason} | must check: ${verdict(i.id).must_check}`).join('\n')}

Procedure:
1. Save each change as a patch first: git diff ${plan.base_commit} -- <its files> > rust/target/perf/round-${round}/<id>.patch (create the directory; new files: copy them there too).
2. Revert every change the reviewer REJECTED: git checkout ${plan.base_commit} -- <its files> (delete files the change created). You are the only agent allowed to run these git commands, and only on the files listed above.
3. Gate: run the gate for the crates touched by the remaining changes (perf-gate.sh changed <crates>), plus each must_check. Any failure: find which change causes it (revert one at a time), revert that change, rerun. The gate must end green.
4. Measure on a quiet machine: wait for a 1-minute load below 1.0 apart from yourself (up to 20 minutes; then proceed and note the load). cargo build --release -p kicad-bench, run the bench sets (small and medium always; each large board only for the stages a change targets) with enough repetitions, and compare with the latest quiet numbers in docs/PERF.md (the baseline or the previous round), using minima. Attribute gains by target stage; where two changes target the same stage, revert one temporarily to separate them. A change is KEPT only if its target stage improves beyond the run-to-run spread you observe (state the spread) on at least the medium boards and nothing else gets slower beyond noise; otherwise revert it (step 2 commands) and say why.
5. If you reverted anything after step 3, run the gate again for the affected crates.
6. Record in docs/PERF.md: a "Round ${round}" section with the changes kept and reverted (one line each: what, files, gain or reason) and the new quiet table for the measured boards with before / after and the remaining gap to the reference; update the ranked hotspot list if the order changed.
7. Commit the kept state: git add -A rust && git commit with a subject "Rust port: performance round ${round}" , a body listing the kept changes, and at the end the trailers this branch's commits carry: "Assisted-by: Claude Code" (AGENTS.md requires an Assisted-by line), "Co-Authored-By: Claude <noreply@anthropic.com>" (no model name), and the session line of the session running this loop if it has one (see git log -3). Do not push, do not amend, do not touch history. If nothing differs from HEAD there is nothing to commit.`, { label: `judge round ${round}`, phase: 'Judge', schema: JUDGE_SCHEMA })

  const kept = judge ? judge.kept : []
  const reverted = judge ? judge.reverted : done.map(i => ({ id: i.id, reason: 'judge did not report' }))
  history.push({ round, kept, reverted, table: judge ? judge.table : '' })
  if (kept.length) { dry = 0; log(`round ${round}: kept ${kept.map(k => k.id).join(', ')}`) } else { dry += 1; log(`round ${round}: nothing kept (dry ${dry})`) }
}

return { gate, history, rounds: round }