# Domain docs

Read the applicable domain documents before you examine the code.

## Documents

- Read `CONTEXT.md` at the repository root.
- If `CONTEXT-MAP.md` exists, read it and each applicable `CONTEXT.md`.
- Read applicable ADRs in `docs/adr/`.
- For a multi-context repository, also read applicable ADRs in `src/<context>/docs/adr/`.

If a file does not exist, continue without an error.

## Layout

This repository uses the single-context layout:

/
├── CONTEXT.md
├── docs/adr/
└── src/

## Vocabulary

Use the terms that `CONTEXT.md` defines. Do not use a different term for the same concept.

If the necessary term is not present, check if the term is necessary. If it is necessary, record the gap for the domain-modeling skill.

## ADR conflicts

If work conflicts with an ADR, identify the conflict. Do not replace the ADR decision without notice.
