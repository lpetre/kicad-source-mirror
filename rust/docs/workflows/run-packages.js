export const meta = {
  name: 'run-packages',
  description: 'Run port work packages by id: each agent reads its assignment from rust/docs/packages/<ID>.md; chains run in parallel, packages inside a chain in order',
  phases: [
    { title: 'Port', detail: 'faithful C++ to Rust ports with self-verification against the reference build' },
  ],
}

const PRE = `You are part of a team porting KiCad core features from C++ to Rust. The C++ source tree (read-only reference; it is the specification) is at /home/user/kicad-source-mirror; the Rust workspace is at /home/user/kicad-source-mirror/rust. FIRST read /home/user/kicad-source-mirror/rust/PORTING.md completely - section 7 (working alongside other agents, incl. the MEMORY and DISK rules: every C++ compile or link runs under \`flock /home/user/kicad-oracle/cxx-build.lock\` with at most -j2; no scratch file over 200 MB; the machine has already crashed once from breaking these) and section 9 ("Project-wide decisions") are binding - and rust/docs/ORACLE.md (reference kicad-cli, static C++ libs, the item-level dump tool pcb-dump with its Rust counterpart pcb-dump-rs and compare.py). rust/docs/PLAN.md shows the overall state. Run cargo from /home/user/kicad-source-mirror/rust. The machine is shared (4 cores, 15 GB) and about ten agents are working in this workspace right now, several of them in the same crate as you but in other files: one cargo command at a time, always \`-p <crate>\` (never workspace-wide), prefer \`cargo check -p <crate> --lib\` for quick iterations; "Blocking waiting for file lock" is normal. Make your edits in complete, compiling bursts; if a build fails because of a file you do not own, wait (\`sleep 90\`) and retry - never edit it. Format only your own files (\`rustfmt --edition 2024 <files>\`). You own exactly the files your assignment names; anything else you need changed goes into \`change_requests\`. Never run git commands that change the index or working tree. Never modify the C++ tree. Token budget is finite: read what you need once, keep your final answer short.

Fidelity (PORTING.md section 1) is the point of this project: port the C++ statement by statement so results are identical. Self-verification is mandatory (section 9): prove your port against the real C++ - a differential oracle (C++ driver linking the reference build, compact golden, Rust replay test), the reference kicad-cli, or pcb-dump vs pcb-dump-rs - whichever fits; code that cannot be verified yet because it depends on unported parts must say so explicitly in your result. Calls into parts of the Rust port that are still \`port_pending!\` stubs are fine (they panic with a clear message until their package lands): never work around a pending stub by re-implementing it. Keep the crate's PORT_STATUS.md current (append/maintain your own section only).`

const SCHEMA = {
  type: 'object',
  properties: {
    summary: { type: 'string', description: 'max 250 words' },
    tests_ported: { type: 'number' },
    verification: { type: 'string', description: 'what was compared against the C++ and the result, with numbers; what could not be verified yet and why' },
    all_checks_green: { type: 'boolean' },
    decisions: { type: 'array', items: { type: 'string' }, description: 'every design/API decision the specification did not prescribe, one line each (they are reviewed)' },
    deviations: { type: 'array', items: { type: 'string' } },
    known_gaps: { type: 'array', items: { type: 'string' } },
    change_requests: { type: 'array', items: { type: 'string' }, description: 'changes needed in files you do not own' },
    notes_for_next: { type: 'string', description: 'max 350 words: what the next package / downstream users need to know (API cheat-sheet, conventions, pitfalls)' },
  },
  required: ['summary', 'tests_ported', 'verification', 'all_checks_green', 'decisions', 'deviations', 'known_gaps', 'change_requests', 'notes_for_next'],
}

const cfg = args || {}
const IMPL_MODEL = cfg.implModel   // model for implementation agents (pass it in args); reviewers inherit the session's
const chains = cfg.chains || []
const interrupted = new Set(cfg.interrupted || [])
const extra = cfg.extra || {}
const models = cfg.models || {}
const runChain = async (chain) => {
  const out = []
  let prev = null
  for (const id of chain) {
    const prompt = `${PRE}

## Your work package: ${id}
Your assignment is the file /home/user/kicad-source-mirror/rust/docs/packages/${id}.md - read it now, completely; it names the crate, the files you own, the C++ to port and how to verify.
${interrupted.has(id) ? `\nIMPORTANT - this package was already started: a previous agent worked on it for a while and was cut off by a machine restart before reporting. Its partial work is on disk in your files (and possibly a section in the crate's PORT_STATUS.md, test files, oracle drivers or goldens). Start by taking stock: build, read what exists, find what is unfinished (port_pending!, todo, failing or missing tests, half-written files - a file may even be truncated). Keep what is sound, finish the rest, and verify the whole package as if you had written all of it. Do not start over.\n` : ''}${extra[id] ? `\nAdditional note from the orchestrator: ${extra[id]}\n` : ''}${prev && !prev.failed ? `\nNotes from the package that finished just before yours in this chain (${prev.pkg}): ${prev.notes_for_next}\n` : ''}
Acceptance: the checks of PORTING.md section 6 are green for your crate as far as your files are concerned (report failures caused by other agents' in-progress files separately), incl. the wasm32-unknown-unknown build for library crates.`
    const r = await agent(prompt, { label: `${id}`, phase: 'Port', schema: SCHEMA, model: models[id] || IMPL_MODEL })
    prev = { pkg: id, ...(r || { failed: true }) }
    out.push(prev)
  }
  return out
}
const results = await parallel(chains.map(c => () => runChain(c)))
return results
