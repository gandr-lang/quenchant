# quenchant-dylints

A Dylint compiler plugin for policy boundaries that require Rust HIR, type identity, call edges, or structured rustdoc. It is a `cdylib`, not an application dependency. The workspace uses the plugin it builds, so analysis and consumer code are checked at the same source revision.

## Install and load

Run from the workspace root:

```sh
mise install
mise run check:ci-pins
mise run check:dylint
```

The root manifest already selects this library through a local Dylint metadata path. The pinned compiler includes `rustc-dev` and `llvm-tools`; the Dylint tool versions and Clippy source tag must match that compiler. A previously cached library or driver from another compiler is not interchangeable.

External consumers select a full plugin revision in their Dylint workspace metadata and use its matching compiler and gate binary. This crate depends on compiler-internal APIs, so a registry version alone does not establish compatibility.

## Git distribution

This package has `publish = false`. It remains a workspace member and participates in builds, tests, and the self-hosted policy wall. Only registry publication excludes it. The `quenchant` facade does not depend on it, so library users do not acquire a compiler plugin or its toolchain requirements.

Consumers use [Dylint workspace metadata](https://github.com/trailofbits/dylint#workspace-metadata), not an ordinary Cargo dependency:

```toml
[[workspace.metadata.dylint.libraries]]
git = "https://github.com/gandr-lang/quenchant"
tag = "quenchant-v0.0.0"
pattern = "crates/quenchant-dylints"
```

For reproducible project pins, replace `tag` with `rev` containing the reviewed full commit. Run `cargo dylint --all` in that consumer workspace. Dylint downloads and builds the plugin; [its driver checks consumer code with the plugin's compiler](https://github.com/trailofbits/dylint/blob/master/docs/how_dylint_works.md#limitations). The consumer must therefore compile under that compiler for the lint run, even if its ordinary builds use another supported compiler.

Git development dependencies are permitted in publishing projects, but adding a plugin to `[dev-dependencies]` does not run its lints. `clippy_utils` is a normal dependency of this plugin because the plugin needs it to compile. Keeping the plugin Git-distributed preserves the upstream lint-authoring API without copying helpers or adding a registry fork.

## Selecting compiler and utility versions

Choose the stable Rust release first. Start with the nightly named by the Clippy source associated with that release, then find the nearest compatible nightly for that release's chosen `clippy_utils` source if the starting pair fails. Keep one selected nightly for the workspace: shared compiler-specific dependencies and caches avoid a second build lane. This is a cache-reuse strategy, not a claim that nightly intrinsically compiles faster than stable.

Source identity is part of the pair. The [standalone `rust-1.98.0` Clippy tag](https://github.com/rust-lang/rust-clippy/blob/rust-1.98.0/rust-toolchain.toml) and [Clippy bundled with Rust 1.98.1](https://github.com/rust-lang/rust/blob/1.98.1/src/tools/clippy/rust-toolchain.toml) both name `nightly-2026-06-25`; this workspace retains that nightly and Git-tagged utility source. Rust's bundled Clippy can diverge from the standalone source after synchronization. Matching toolchain-file text does not mean identical source, and stable plus `RUSTC_BOOTSTRAP` is not a substitute for the matched nightly.

Cargo's [multiple-location rule](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations) substitutes a registry version for a Git dependency when publishing. Registry `clippy_utils 0.1.98` is not the same source as Git tag `rust-1.98.0`: its [recorded revision](https://github.com/rust-lang/rust-clippy/blob/96bdb478cc04133104773a52a714a013433e2689/rust-toolchain.toml) names `nightly-2026-05-28`. Utility-only probes also compiled that registry source on June 8, while June 9 failed after compiler API removals. Those probes do not establish whole-workspace compatibility. This plugin's Git distribution avoids the substitution entirely; publishing the library family is not a reason to change its compiler or utility source.

For future bumps:

- Identify the chosen stable release and exact Clippy source before comparing version strings. If a stable patch has no standalone Clippy tag, inspect its bundled Clippy and the applicable existing tag; never invent a tag or substitute a later release train.
- `mise run toolchain:bump <stable>` invokes the Rust `quenchant-gates toolchain-bump` command. It resolves a supported stable tag's nightly and updates the tag and compiler pin together. It does not perform an automatic nearest-nightly search.
- Test the starting pair with the actual utility source and Dylint version. If incompatible, inspect the breaking compiler APIs and test nearby nightlies by increasing distance from the starting nightly, remaining in the selected Rust release line. Verify the boundary with adjacent candidates; a package version number alone is not evidence.
- Record the stable release, source tag or revision, selected nightly, rejected nearer candidates, and executed checks together. If the selection differs from the tag's declared nightly, update the pin gate's compatibility model with the evidence; do not bypass its existing equality check or introduce an environment override.
- Retain required components and targets. Rebuild the driver and plugin through repository tasks, run `mise run check` and all-file hooks, and verify the actual consumer lint invocation. Keep caches keyed to the root toolchain file.
- At every intended registry release head, run `mise exec -- cargo publish --dry-run --locked --workspace`. This verifies the six registry packages, not the Git plugin; its build and UI tests remain separate mandatory evidence. Do not credit `--no-verify` as compilation or infer readiness from one unrelated package.

## Example: validate the source policy

Run the library's own tests from `crates/quenchant-dylints/`, where its linker configuration applies:

```sh
mise exec -- cargo test --lib
```

For normal work, prefer the repository's `mise run check:tests` task, which selects the package working directory explicitly. A manifest argument does not itself change Cargo's configuration-search directory.

## Shipped rules

| Lint                                         | Observable boundary                                                                                                                        |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `single_field_struct_needs_transparent_repr` | A single-field struct states its transparent layout                                                                                        |
| `primitive_signature`                        | A crate-authored signature reaches an appropriate nominal boundary instead of exposing a primitive through the inspected structural layers |
| `recursion_forbidden`                        | A function in a discovered local recursive call cycle needs the specified item-level exception and termination evidence                    |
| `recursive_owned_pointer`                    | A local data type cannot own a path back to itself through the analyzed field graph                                                        |
| `specification_present`                      | Authored functions and methods carry a specification section; `trivial` cannot sit beside substantive clauses                              |
| `adequacy_block_grammar`                     | An authored adequacy section has the required hypothesis and witness shape                                                                 |
| `mode_dispatch_wildcard`                     | A declared judgment scrutinee cannot be hidden behind a fallback match arm                                                                 |
| `primitive_arithmetic`                       | Primitive integer operators and resolved inherent arithmetic families use the nominal arithmetic surface                                   |
| `option_signature`                           | Authored signatures preserve absence reasons; foreign methods admit only the `Option` layers their declarations require                    |

The plugin is a policy floor, not a proof of totality or complete semantics. Call edges erased by function-pointer coercion, compiler-generated drop behavior, and unresolved type relationships require the corresponding review or evidence boundary. The ownership and call analyses address different mechanisms; neither subsumes the other.

The primitive-signature rule does not establish a wrapper's meaning, visibility, conversions, or absence policy. `option_signature` checks absence exposure separately; neither rule proves that a chosen wrapper or reason enum models the domain correctly.

### Arithmetic and absence activation

`primitive_arithmetic` and `option_signature` are opt-in lints; the seven established rules retain their default levels. Select both explicitly in the consumer's Dylint invocation or through `cfg_attr(dylint_lib = "quenchant_dylints", deny(primitive_arithmetic, option_signature))` on its crate roots. Registration alone does not enable either policy. The producer's UI suites deny both the selected predicate and unknown lint names; a missing predicate cannot satisfy their expected diagnostics.

### Primitive arithmetic

The predicate rejects integer `+`, `-`, `*`, `/`, `%`, shifts, their assignment forms, and unary negation. It also rejects compiler-resolved inherent integer methods in the `checked`, `strict`, `wrapping`, `saturating`, `overflowing`, and `unchecked` arithmetic families, including UFCS, type aliases, autodereferenced receivers, and function-item references. The inspected operations are addition/subtraction and their signed/unsigned forms, multiplication, division/remainder including Euclidean forms, negation, powers, absolute value, shifts, signed/unsigned differences, next multiples/powers of two, integer square root, and integer logarithms.

Unprefixed partial arithmetic methods obey the same rule: `pow`, `abs`, `div_euclid`, `rem_euclid`, `div_floor`, `div_ceil`, `next_multiple_of`, `next_power_of_two`, `ilog`, `ilog2`, `ilog10`, and signed `isqrt`. Their overflow, zero-divisor, or invalid-domain inputs make them partial even where Clippy emits no diagnostic. Funnel shifts and their unchecked forms also require the arithmetic boundary. Total operations such as unsigned `isqrt`, `unsigned_abs`, `abs_diff`, and `midpoint` remain accepted. The arithmetic library currently supplies the five binary operation families; rejection of another primitive operation does not imply that a corresponding library adapter exists.

Method definition identity and primitive operand types are both checked. The compiler's language items identify `Add`, `Sub`, `Mul`, `Div`, `Rem`, `Shl`, `Shr`, their assignment traits, and `Neg`. Method calls, UFCS, and function-item references to those traits are rejected when every instantiated input is a primitive integer after peeling references, including the mutable receiver of assignment methods. Binary and assignment operators likewise require primitive integer operands on both sides: a nominal right-hand operand can define a legitimate overload even when the left-hand operand is an integer. Same-spelled nominal methods, unrelated extension-trait methods, and nominal overloaded operators remain accepted. Boolean/bitwise operations and comparisons are outside this predicate, except the partial funnel-shift operations above. Floating-point arithmetic, conversions, other non-arithmetic partial APIs, and the justification of modular or clamping semantics remain separate policy obligations. Existing Clippy denials complement this predicate; they do not establish coverage of its method inventory.

The representation boundary is an implementation of the local `quenchant_arith::arith::Integer` trait for the local `quenchant_arith::arith::Int<primitive integer>` type. Those methods implement the library's explicit arithmetic families and therefore must reach primitive operations. Closures within the method retain that boundary; nested functions, unrelated producer helpers, different traits, and different nominal self types do not. This is canonical definition-path recognition, not a crate-wide exemption or an authenticity check on a package bearing that name.

Resolved function items remain visible, including before pointer coercion. An already-erased function pointer has no recoverable method identity. Generic operator inputs that remain unresolved during body type checking are not treated as primitive; this pass does not recheck generic bodies at monomorphization. Rustc's external-expansion diagnostic filtering remains a reporting boundary. Passing this lint is not a proof that overflow is unreachable or that a strict operation cannot panic.

### Absence signatures

The predicate identifies the standard `Option` through rustc's diagnostic-item identity. It checks all visibilities of free, const, async, and extern functions, inherent methods, local trait declarations/defaults/implementations, and foreign declarations. Inferred closure signatures are implementation details rather than authored APIs.

Normalization exposes resolvable aliases and associated substitutions. Inspection follows references, pointers, tuples, arrays, slices, function pointers, generic arguments, dynamic-trait bounds, and explicit opaque/future bounds. It stops at a local transparent nominal type or the canonical `quenchant_shape::shape::Maybe`; it does not inspect representation fields. Unresolved projections are not a claim of checked absence.

The shape producer registers `Maybe` as the `quenchant_maybe` rustc diagnostic item under the internal `quenchant_compiler_policy` cfg. The predicate compares the resolved `DefId`, so dependency aliases and re-exports preserve the boundary while an unrelated same-path definition does not acquire it. An unmarked rlib receives no canonical exemption. This compiler-owned identity is not package authentication: deliberately registering the reserved diagnostic item is a producer declaration, and duplicate registrations are compiler errors.

Policy builds must pass `--cfg=quenchant_compiler_policy` through Cargo's whole-graph compiler flags, including external shape dependencies. Dylint uses a workspace wrapper; its own `dylint_lib` cfg does not reach external dependencies. `mise run check:dylint` preserves `CARGO_ENCODED_RUSTFLAGS` when present, including an empty value; otherwise it encodes the whitespace-separated `RUSTFLAGS`, then appends the policy cfg. This composes with instrumentation flags such as `--cfg anodized_panic`. The cfg is expected by the producer's lint configuration and is confined to policy builds.

Later consumer activation must adopt that flag composition alongside the reviewed plugin and shape revisions, then explicitly enable `option_signature` and `primitive_arithmetic`. Enabling only the lint does not identify a prebuilt unmarked shape library. Ordinary and `--all-features` runtime-library builds need no compiler attributes or nightly-only Cargo feature. This separation is revisited only if the selected compiler or invocation cannot propagate the cfg reliably without changing those ordinary builds; no name fallback or global bootstrap substitutes for identity.

A foreign trait implementation is paired with the foreign method's unsubstituted signature. Only corresponding required `Option` layers are admitted. `Iterator::next` may return its required `Option<Item>`, but substituting `Option<Value>` for `Item` introduces another refused layer. `From<Option<Value>>::from` likewise introduces absence where the foreign declaration has a generic parameter and remains refused. Required callback, dynamic, and future shapes follow the same rule. A comment justifying primitive unpacking does not waive a prohibited signature.

## Structured documentation

`# Specification` states an item's behavior. `# Adequacy` states a falsifiable evidence hypothesis and names witnesses. The plugin checks source shape and authorship; [quenchant-gates](../quenchant-gates/README.md) separately resolves witness names against runnable tests in the owning package and target.

A generated sibling can reuse an author's identifier token without becoming an authored declaration. The presence analysis therefore inspects both declaration and name provenance. The specification UI fixtures use actual facade expansion and a separate fixture macro to distinguish these cases. Generated assertion failures still carry evidence and require classification; generation alone is not an exclusion.

`# Termination` accompanies an approved recursive exception. Its reason, measure, boundedness, and input-recursion statements are independent obligations. Inherited lint configuration is not inherited approval. The analysis can refute some claims against discovered argument flow, but a well-shaped block is not a termination proof.

## Ownership exceptions

A foreign generic known not to own its arguments can be listed in `dylint.toml` under `[quenchant-dylints].non_owning_generics`. Each entry needs its canonical type path and a concrete justification. An unjustified entry is refused; unknown foreign generics remain conservatively owning. No allowlist entry supplies a missing semantic proof by itself.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
