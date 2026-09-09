# AGENTS.md

Read this file, then `docs/agents/baseline.md`, then every matching row before acting. Rules apply cumulatively; the nearest file wins only where two rules conflict.

| About to                                    | Read                               |
| ------------------------------------------- | ---------------------------------- |
| write or review an exchange on this tracker | `docs/agents/publication.md`       |
| write, change, or review Rust               | `docs/agents/rust.md`              |
| write specification blocks or tests         | `docs/agents/testing-contracts.md` |
| build, test, format, gate, or commit        | `docs/agents/source-workflow.md`   |
| change or run CI locally                    | `docs/agents/ci-local.md`          |

The repository is bound for public visibility. Never commit credentials, machine-specific paths, private operations, plaintext session identifiers, or unreviewed generated output. Opaque provenance marks are permitted. Every package remains unpublished: each keeps `publish = false`, and no automation runs `cargo publish`.

## Layout

One Cargo workspace, one toolchain pin, one gate set. Every crate lives under `crates/` and carries the `quenchant-` package prefix; a package's directory name is its package name, and its module names take underscores.

| Crate                             | Concern                                                               |
| --------------------------------- | --------------------------------------------------------------------- |
| `crates/quenchant-arith`          | profile-independent integer arithmetic at nominal type boundaries     |
| `crates/quenchant-shape`          | nominal non-failure absence and transparent type scaffolding          |
| `crates/quenchant-dylints`        | the Dylint policy library: the gates Clippy cannot express            |
| `crates/quenchant-gates`          | the non-lint gate binary: specification state and adequacy witnesses  |
| `crates/quenchant-fixture-macros` | attribute macros the Dylint UI matrix expands to stage foreign shapes |

The Dylint wall self-hosts: `[[workspace.metadata.dylint.libraries]]` loads `crates/quenchant-dylints` by path, so the policy and the code it governs move in one commit and no pinned revision stands between them.

## Shared-page bindings

`docs/agents/rust.md` and `docs/agents/testing-contracts.md` are byte-identical shared pages, taken verbatim and never edited here. Their source examples bind to this repository as follows; package READMEs own policy semantics and public names.

| Source example                                                  | Local binding                                                                                                                          |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Crate categories and package prefixes                           | Directory `crates/quenchant-<name>`; package `quenchant-<name>`. One prefix, no category axis: a category column would carry one value |
| Flat data path                                                  | Discovery, parsing, policy analysis, diagnostics                                                                                       |
| Checker/machine correctness keystone and owned policy machinery | The lint and gate engines. Policy analysis is owned here rather than depended on                                                       |
| Dependency `consumers` example                                  | Workspace crate `quenchant-gates`                                                                                                      |
| `Maybe<T, R>` representation                                    | `crates/quenchant-shape` is that home: the shape is defined there and consumed by name, never re-derived per crate                     |

The workspace both produces the Dylint policy and runs under it, so the producer's obligations and the consumer's wall apply to the same tree: the library owns its unit and UI suites, and `mise run check:dylint` loads it over every member. Repository gate commands are in `docs/agents/source-workflow.md`.
