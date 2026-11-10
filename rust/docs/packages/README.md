# Work package assignments

One file per work package of the port, as handed to the agent that implemented it (id, title,
crate, the files it owns, the C++ it ports, how it verifies itself). They are the orchestrator's
record of what was asked; the outcome of each package is in the owning crate's `PORT_STATUS.md`.
The workflow script `run-packages` starts agents by package id and tells them to read their file
here first.
