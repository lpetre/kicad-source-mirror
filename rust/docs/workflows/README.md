# Workflow scripts

Two orchestration scripts that drove most of this port, kept so that another session can run
them again (Workflow tool, `scriptPath`). They contain absolute paths of the machine they were
written on (`/home/user/kicad-source-mirror`, `/home/user/kicad-oracle`): adjust them first.

- `run-packages.js` - runs work packages by id. Each agent reads its assignment from
  `rust/docs/packages/<ID>.md`; chains run in parallel, packages inside a chain in order.
  Arguments: `{ "chains": [["A", "B"], ["C"]], "interrupted": [], "extra": {}, "models": {},
  "implModel": "<model for the implementation agents>" }`.
- `perf-loop.js` - the performance loop of `docs/PERF.md`: a gate and baseline step, then rounds
  of plan -> optimise (one agent per task, disjoint files) -> review of the diffs (can any
  output byte change?) -> judge (revert what was rejected, run the gate, measure base against
  head interleaved, record the round in `docs/PERF.md`, commit). It stops after two rounds
  that keep nothing or after `maxRounds`. Arguments: `{ "maxRounds": 8, "tasksPerRound": 3,
  "implModel": "<model>", "interrupted": [] }`.

Both expect what the cloud machine had: the reference `kicad-cli` and the comparison drivers
(`docs/ORACLE.md`), and the caches the gate reads (`scripts/perf-gate.sh`, header comment).
Without them the review step still works; the gate and every "identical to KiCad" check do not.

The policy they were run under: implementation agents on a cheaper model, every result read
and every non-trivial decision reviewed by a second, independent agent on the strongest model
before anything was built on it. The reviews found real defects each time
(`docs/design/review-*.md`, `docs/PLAN.md`).
