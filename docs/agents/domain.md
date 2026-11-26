# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Layout: single-context

`GLOSSARY.md` + `docs/adr/` at the repo root.

## Before exploring, read these

- **`GLOSSARY.md`** at the repo root
- **`docs/adr/`**: read ADRs that touch the area you're about to work in

If any of these files don't exist, **proceed silently**. Don't flag their absence; don't suggest creating them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
/
├── GLOSSARY.md
├── docs/adr/
│   ├── 0001-json-abi-as-the-rewrite-boundary.md
│   └── 0002-utf16-entity-offsets.md
├── rust/            ← the rewritten implementation (cdylib tdjson_rs)
└── td/              ← upstream C++ (read-only for us)
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `GLOSSARY.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0001 (JSON ABI as the rewrite boundary), but worth reopening because…_

## Repo-specific notes

- Facts live in `docs/facts.md` and outrank prose anywhere else; a glossary term enters only when it is `已证` or a pure naming decision.
- The C++ tree under `td/` is upstream (fork of `tdlib/td`). Treat it as the oracle: read it for semantics, never rewrite it as part of the Rust work.