# Local CI

`.github/workflows/ci.yml` has separate build/test, Miri, Dylint, and policy jobs. `.github/actions/setup-rust` installs the active `rust-toolchain.toml` selection through rustup, including components and targets. Dylint consumers MUST use the matching compiler for a cached driver or library.

## Reproduce the relevant job

Run from the workspace root:

| Change                                         | Local verification                                                                                                     |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Rust implementation or public feature          | `mise run check`                                                                                                       |
| Compiler plugin                                | `mise run check:dylint` and `mise run check:tests`                                                                     |
| Specification wrapper or backend configuration | `mise run check:tests`, `mise run check:anodized-enforcing`, and `mise run check:no-std`                               |
| Witness references or inventory                | `mise run check:witnesses` and the gate package tests                                                                  |
| Pins, workflow YAML, or repository policy      | `mise run check:ci-pins`, `mise run check:action-pins`, `mise run check:public-boundary`, and `mise run treefmt:check` |
| Documentation or formatting                    | `mise run treefmt`, then `mise exec -- prek run --all-files`; execute changed examples                                 |

The root gate is the integration wall, not a substitute for understanding a failed narrow command. `dylint:tests`, `dylint:clippy`, and `anodized:specifications` select the package working directory so its linker configuration applies; the corresponding `check:*` tasks depend on them. NEVER replace those tasks with a root invocation that only supplies `--manifest-path`. `tests:enforcing` and `check:doc` declare compiler environments through native task fields, not shell bodies.

## Configuration-sensitive evidence

The default library configuration and the enabled `std` interpretation are separate builds. CI and local tasks exercise a real target without `std`; the enforcing runs select the published backend and its host cfg, then execute a deliberately violated postcondition. A listed target cfg is insufficient evidence for a host procedural-macro artifact.

Miri runs the strict and `fast` library tests on valid inputs. Both development and test profiles retain assertions and overflow checks because those configurations affect the Miri path. A panic from the wrong cause is not a successful enforcement witness.

## Updating a pin

Read the toolchain and dependency comments before changing a value. Compiler channel, rustc internals, Clippy tag, Dylint tools, and cached driver/library compatibility form one boundary. Updating only the manifest version or copying a driver from another toolchain does not establish it.

Workflow action references use immutable revisions. Keep the action-pin gate, Rust selection, package working directories, and feature/mode coverage aligned between the task graph and CI. The record of a verification run states the selected manifest, target, features, enforcement mode, and result; it does not promote a skipped job to evidence.
