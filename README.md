# quenchant

Specification-first hardening for Rust: types, obligations, tests, and gates that make evidence explicit.

Arithmetic and domain boundaries are stated in types, obligations are authored beside the code they constrain, and the compiler and source gates refuse what leaves either implicit. One workspace pins the toolchain, the dependencies, and the verification tasks; the policy library is built from the same tree it checks.

Written for code that is generated as much as written. A synthesizing agent reproduces the shapes its context rewards: with these instruments in the tree, the rewarded shape is a nominal type, a stated obligation, a conclusion whose strength is in its type, and a gate that refuses the rest. The libraries make the correct shape the path of least resistance; the gates make every other shape a compile error.

## Crate map

| Package | Consumer surface | Role |
| ------- | ---------------- | ---- |
| [quenchant](crates/quenchant/README.md) | `quenchant::arith`, `quenchant::shape` | Umbrella package re-exporting the publishable libraries under one namespace |
| [quenchant-arith](crates/quenchant-arith/README.md) | `quenchant_arith::arith` | Nominal integers with explicit strict, checked, wrapping, saturating, and unchecked arithmetic |
| [quenchant-shape](crates/quenchant-shape/README.md) | `quenchant_shape::shape` and exported macros | Reason-preserving absence, closed reason sites, and transparent domain types |
| [quenchant-anodized](crates/quenchant-anodized/README.md) | `anodized::spec` | Specification facade with optional development instrumentation |
| [quenchant-spec-macros](crates/quenchant-spec-macros/README.md) | Used through the facade | Dependency-free token forwarding and disabled-mode marker removal |
| [quenchant-dylints](crates/quenchant-dylints/README.md) | Dylint compiler plugin | Signature, layout, recursion, ownership, specification, and evidence-shape checks |
| [quenchant-gates](crates/quenchant-gates/README.md) | `quenchant-gates` executable | Invocation-state and witness checks; repository boundary, pin, action, and publication refusals |
| [quenchant-fixture-macros](crates/quenchant-fixture-macros/README.md) | Test-only procedural attributes | Real foreign expansions for the compiler-plugin fixtures; never published |

The arithmetic and shape libraries support `no_std` in stripped and panic-enforcing builds; print mode uses `std`. Procedural macros run on the build host and do not themselves require a target standard library. No Anodized runtime or logic implementation is vendored here.

## Specification and checking modes

A specification states admitted behavior. Satisfaction relates an implementation to that statement; evidence supports a particular obligation under stated assumptions. Adequacy concerns whether the specification, observations, and chosen evidence distinguish the deviations that matter. Neither a passing suite nor a mutation score defines completeness.

Specifications use `use anodized::spec;` from the `quenchant-anodized` package. Its development-only `anodized` feature compiles [gandr-lang/anodized](https://github.com/gandr-lang/anodized) for `no_std` enforcement, `const fn` specifications, and type refinements without the logic layer. The build cfg—not a consumer feature—selects checking: `anodized_panic` enforces and `anodized_print` prints violations. With neither cfg and the backend enabled, predicates remain type-checked without being evaluated, while captures retain the fork's evaluation semantics. With the backend feature off, annotations and supported nested markers are removed. Required validation and safety checks stay unconditional, and authored obligations remain authoritative.

Backend selection does not by itself enable violation panics. The verification tasks set the backend's enforcing host cfg and execute negative witnesses. A future proof adapter must read authored source or a preserved specification representation; it cannot recover removed clauses merely by inspecting stripped HIR.

## Use a library

From an application beside a checkout, one dependency supplies arithmetic and shapes:

```toml
[dependencies]
quenchant = { version = "=0.0.0", path = "../quenchant/crates/quenchant" }
```

Selecting a single library instead keeps the same versions:

```toml
[dependencies]
quenchant-arith = { version = "=0.0.0", path = "../quenchant/crates/quenchant-arith" }
quenchant-shape = { version = "=0.0.0", path = "../quenchant/crates/quenchant-shape" }
```

The path form works before a registry release. Version declarations identify the intended family; they are not a claim that a package has already been published. Each package README gives its own installation and example.

## Work on the workspace

Run at the repository root:

```sh
mise install
mise run check
mise exec -- prek run --all-files
```

`rust-toolchain.toml` selects the compiler, its components, and the bare-metal verification target. Dylint and Clippy internals require that exact compiler pairing. The gate set covers the strict/fast arithmetic configurations, optional instrumentation, native tests, real `no_std` builds, private rustdoc, witness resolution, Miri, policy, and formatting.

The Dylint package's own tests and Clippy invocation run from its package directory for linker configuration. Repository tasks arrange that boundary; a root command that ignores it is not an equivalent verification run. [AGENTS.md](AGENTS.md) maps the shared guidance's source examples to the actual local commands and packages.

CI installs mise-managed tools in both workspace-test and Dylint jobs so witness inventory uses the consumer-selected nextest. Tool setup retains Rustup proxies on `PATH`, preserving compiler-plugin toolchain selection. Nested consumer fixtures clear the parent nextest profile because that profile belongs to the test workspace, not the fixture. Dylint CI targets use absolute workspace-rooted paths so temporary compiler fixtures retain a writable output location. Image-consuming jobs inherit `contents: read` and `packages: read` through one permissions anchor; GHCR pulls authenticate with the job's `GITHUB_TOKEN`. Jobs without containers retain their existing permissions.

Rust, test, and workflow changes run every hosted lane on pull requests and merge groups. Manual dispatch exercises the same lanes. The Dylint job uploads plain and enforcing JUnit reports separately, before the next invocation replaces `target/nextest/ci/junit.xml`; compiler build directories do not determine that report path.

Workspace and Dylint jobs retain separate immutable artifact-cache lanes. The Dylint lane shares ordinary test, Clippy, and witness artifacts under the same linker flags; enforcing tests keep their own target directory. UI dependency builds occupy a sibling Cargo target tree, outside the profile's disposable `deps` entries. Successful compiler artifacts remain cacheable after a test failure. Main warms caches visible to other branches; pull-request and manual runs can retain their own ref-scoped build state.

Local tasks and CI use the pinned Cargo's [content-based freshness](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#checksum-freshness) so a fresh checkout can reuse unchanged workspace artifacts. Source checksums replace timestamp-only freshness; changed content and compiler flags still invalidate builds. Source timestamps are not backdated. Build-script input tracking retains Cargo's mtime behavior, and whole-crate policy checks keep incremental compilation disabled.

The UI dependency build selects fixture packages, with foreign-library development dependencies owned by `quenchant-fixture-macros`. Both nextest profiles prioritize compiler-backed UI groups over in-memory tests, keeping plugin and fixture-dependency builds concurrent.

The compiler plugin loads locally:

```toml
[[workspace.metadata.dylint.libraries]]
path = "crates/quenchant-dylints"
```

External consumers select the plugin through [Git-based Dylint metadata](crates/quenchant-dylints/README.md#git-distribution), pairing a reviewed revision with its matching compiler and gate binary. The [version-pair selection procedure](crates/quenchant-dylints/README.md#selecting-compiler-and-utility-versions) explains the stable-release anchor, source identity, nearby-nightly checks, and future bumps.

Co-author trailers credit people only. Commit validation rejects known assistant identities even without session trailers, while accepting human names and personal email addresses.

## Distribution and licensing

Publication is manual. Five library/macro packages are eligible for crates.io; the gate executable and Dylint plugin are Git-distributed, and fixture macros are internal. These three packages retain `publish = false`; all eight remain workspace members under the normal gate wall. `mise run check:publish-dry-run` selects publishable members from their manifests and runs locked Cargo publication checks in dependency order, resolving unpublished siblings through Cargo's temporary packaging registry. The gate verifies every tarball with the pinned compiler and aborts upload; `mise run check` and the CI build/test lane both run it. Registry queries require network access even for a dry run.

Every package is licensed `Apache-2.0 WITH LLVM-exception`. The workspace states that value once and each manifest inherits it. The exception covers the Dylint library's compiler linking and GPL-2.0 compatibility, so no package needs a second license. The [Apache-2.0](LICENSE.Apache-2.0.txt) text and its [exception](LICENSE.LLVM-exception.txt) sit at the repository root as the single copy.
