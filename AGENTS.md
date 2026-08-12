# Agent guidance

Before planning or implementing any evolution of this project (features, refactors, schema changes, new modules), **read the documentation first** and align with it.

## Required reading

| Doc | When to consult |
|-----|-----------------|
| [README.md](README.md) | Project purpose, stack, how to run |
| [doc/architecture.md](doc/architecture.md) | Design boundaries, layering, anti-bloat rules |
| [doc/roadmap.md](doc/roadmap.md) | Feature priority and stability chores |
| [doc/data-model.md](doc/data-model.md) | Config, SQLite, caches, catalogs |
| [doc/modules.md](doc/modules.md) | Where code belongs; dependency direction |

## Rules of engagement

1. **Plan against the docs** — do not invent parallel architectures, ORMs, UI frameworks, or position formats that contradict `doc/architecture.md`.
2. **Respect roadmap order** — prefer the prioritized backlog in `doc/roadmap.md` unless the user explicitly overrides it.
3. **Update docs when reality changes** — if you add a module, change persistence, or complete a roadmap item, update the relevant `doc/*.md` (and README if user-facing) in the same change set.
4. **Keep the codebase lean** — follow the anti-bloat rules in the architecture doc; put new code in the owning module listed in `doc/modules.md`.

## Debugging

During development, **Debug mode** is the preferred way to investigate Rust code problems (runtime failures, incorrect ephemeris, UI/state issues). Reproduce under a debug build, gather concrete evidence (stack traces, failing inputs, DB contents), then fix — do not guess-and-patch without investigation.

## Testing

Maintain a **clean unit test set for critical code paths**: ephemeris/position sampling, encode/decode of cached blobs, view-window and visibility filters, geo sectoring, config validation, and similar domain/infra logic. Prefer focused unit tests over fragile UI-only checks. Add or update tests when changing those paths; keep tests deterministic and free of network/DB side effects unless explicitly testing persistence.

## Database and schema evolution

Database management and schema evolution must be **intentional and managed** (Diesel migrations, [doc/data-model.md](doc/data-model.md), cache invalidation notes).

- Do **not** casually alter tables, blob layouts, or sector keys during ordinary feature work.
- Schema or position-blob changes are allowed only for a **critical fix** or an **explicit, intended database evolution** — then ship a proper migration, document the change, and plan cache/DB invalidation or upgrade.
