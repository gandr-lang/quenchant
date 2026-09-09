# quenchant

Explicit arithmetic and domain boundaries for Rust, with the compiler and source gates that enforce them.

Rust libraries for explicit arithmetic and domain boundaries, paired with the compiler and source gates used to maintain them. One workspace pins the toolchain, dependencies, and verification tasks. The policy library is built from the same tree it checks.

## Crate map

| Package                                                               | Consumer surface                             | Role                                                                                           |
| --------------------------------------------------------------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| [quenchant](crates/quenchant/README.md)                               | `quenchant::arith`, `quenchant::shape`       | Umbrella package re-exporting the publishable libraries under one namespace                    |
| [quenchant-arith](crates/quenchant-arith/README.md)                   | `quenchant_arith::arith`                     | Nominal integers with explicit strict, checked, wrapping, saturating, and unchecked arithmetic |
| [quenchant-shape](crates/quenchant-shape/README.md)                   | `quenchant_shape::shape` and exported macros | Reason-preserving absence, closed reason sites, and transparent domain types                   |
| [quenchant-anodized](crates/quenchant-anodized/README.md)             | Dependency named `quenchant`                 | Public `#[quenchant::spec(...)]` facade and optional published instrumentation                 |
| [quenchant-spec-macros](crates/quenchant-spec-macros/README.md)       | Used through the facade                      | Dependency-free token forwarding and disabled-mode marker removal                              |
| [quenchant-dylints](crates/quenchant-dylints/README.md)               | Dylint compiler plugin                       | Signature, layout, recursion, ownership, specification, and evidence-shape checks              |
| [quenchant-gates](crates/quenchant-gates/README.md)                   | `quenchant-gates` executable                 | Invocation-state reporting and runnable adequacy-witness resolution                            |
| [quenchant-fixture-macros](crates/quenchant-fixture-macros/README.md) | Test-only procedural attributes              | Real foreign expansions for the compiler-plugin fixtures; never published                      |

The arithmetic and shape libraries default to `no_std`. Procedural macros run on the build host; their use does not itself require a standard library on the target. No Anodized runtime or logic implementation is vendored here.

## Specification and checking modes

A specification states admitted behavior. Satisfaction relates an implementation to that statement; evidence supports a particular obligation under stated assumptions. Adequacy concerns whether the specification, observations, and chosen evidence distinguish the deviations that matter. Neither a passing suite nor a mutation score defines completeness.

The libraries' `anodized` feature opts into published specification instrumentation and a `std`-enabled build. Without it, the annotation and supported nested markers are removed while ordinary code remains. Required validation and safety checks are not optional instrumentation. The authored obligations remain authoritative even when no checks appear in a binary.

Backend selection does not by itself enable violation panics. The verification tasks set the backend's enforcing host cfg and execute negative witnesses. A future proof adapter must read authored source or a preserved specification representation; it cannot recover removed clauses merely by inspecting stripped HIR.

## Use a library

From an application beside a checkout, one dependency covers the family:

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

The compiler plugin loads locally:

```toml
[[workspace.metadata.dylint.libraries]]
path = "crates/quenchant-dylints"
```

External consumers must pair a chosen plugin revision with its matching compiler and gate binary. Updating only one side does not establish compatibility.

## Distribution and licensing

Publication is manual. The manifest gate checks an explicit eligible package set and keeps the fixture package disabled; eligibility is not evidence of a completed release. Cargo can package and dry-run the interdependent library family together using its temporary packaging registry without uploading anything.

Every package is licensed `Apache-2.0 WITH LLVM-exception`. The workspace states that value once and each manifest inherits it. The exception covers the Dylint library's compiler linking and GPL-2.0 compatibility, so no package needs a second license. The [Apache-2.0](LICENSE.Apache-2.0.txt) text and its [exception](LICENSE.LLVM-exception.txt) sit at the repository root as the single copy.
