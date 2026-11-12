## Agent skills

### Issue tracker

Issues are tracked as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Testing

- While implementing a ticket, run only the tests for the code you touched (one test file or `-p <crate>` with a filter). Do not run `cargo test --workspace` or the Dashboard e2e suite per ticket.
- Run the full suite (`cargo test --workspace`, Dashboard e2e) once, after every ticket of a spec is merged into the integration branch. Fix failures there.
- Write few tests: one test per acceptance criterion that needs it, extending an existing test file before adding a new one. No tests for edge cases the ticket does not ask for, and no unit tests for code already covered through a higher seam.
