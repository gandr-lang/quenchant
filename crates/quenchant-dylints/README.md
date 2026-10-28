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
- At every intended registry release head, run `mise run check:publish-dry-run`. This verifies the five registry packages, not the Git tools; the plugin build and UI tests remain separate mandatory evidence. Do not credit `--no-verify` as compilation or infer readiness from one unrelated package.

## Example: validate the source policy

Run the library's own tests from `crates/quenchant-dylints/`, where its linker configuration applies:

```sh
mise exec -- cargo test --lib
```

For normal work, prefer the repository's `mise run check:tests` task, which selects the package working directory explicitly. A manifest argument does not itself change Cargo's configuration-search directory.

## Shipped rules

| Lint | Observable boundary |
| ---- | ------------------- |
| `single_field_struct_needs_transparent_repr` | A single-field struct states its transparent layout |
| `primitive_signature` | A crate-authored signature reaches an appropriate nominal boundary instead of exposing a primitive through the inspected structural layers |
| `recursion_forbidden` | A function in a discovered local recursive call cycle needs the specified item-level exception and termination evidence |
| `recursive_owned_pointer` | A local data type cannot own a path back to itself through the analyzed field graph |
| `specification_present` | Authored functions and methods carry a specification section; `trivial` cannot sit beside substantive clauses |
| `unsafe_safety_documentation` | Unsafe declarations carry a `# Safety` section with a nonempty `- unsafe invariants:` clause, including C++ bridge extern blocks |
| `raw_pointer_through_reference` | A raw pointer is taken with `&raw`, not through a reference created only to become it |
| `adequacy_block_grammar` | An authored adequacy section has the required hypothesis and witness shape |
| `mode_dispatch_wildcard` | A declared judgment scrutinee cannot be hidden behind a fallback match arm |
| `primitive_arithmetic` | Primitive integer operators and resolved inherent arithmetic families use the nominal arithmetic surface |
| `option_signature` | Authored signatures preserve absence reasons; foreign methods admit only the `Option` layers their declarations require |
| `option_field` | Fields of authored structs and enum variants preserve absence reasons unless the item is a serde or clap target, where `Option` is wire form |
| `spec_attribute_present` | A clause-bearing specification carries a `#[spec]` predicate that can fail, or a reasoned exemption; opt-in |
| `spec_attribute_unqualified` | A resolved specification attribute uses an imported, single-segment path rather than a qualified path; opt-in |
| `spec_clause_traversal` | A `#[spec]` clause does not iterate, search, copy, or compare a container reached through the receiver or a mutable parameter, itself or through the bodies it calls; opt-in |
| `adequacy_present` | A clause-bearing specification carries an `# Adequacy` section; opt-in |
| `maybe_shape` | A crate-defined enum does not reimplement `Maybe`'s shape of two variants over two type parameters; opt-in |
| `erased_error_signature` | A crate-defined signature does not return a `Result` whose error is erased behind `dyn Error`, `anyhow`, or `eyre`; opt-in |
| `dishonest_answer` | A configured placeholder answer — a constructor or a literal spelling — is built only under an `expect` with its reason at the site; `answer_totality` group, configured |
| `presenter_coverage` | Every variant of a configured enum is named by an arm of its configured presenter functions; `answer_totality` group, configured |
| `mold_coverage` | The rules a printer's productions name through a configured marker equal the rules the grammar's builders define, less a justified exclusion table; `answer_totality` group, configured |
| `module_doc_heading` | A module's own documentation is a lead, then sections from `# Examples`, `# Witnesses`, `# Boundaries`, `# Items`, `# Dependencies` and `# References` and the evidence headings, each once, one level deep; `rustdoc_shape` group, opt-in |
| `doc_decision_record` | Module and item documentation carry no `Choice`, `Alternatives` or `Reversal` record and no decision table; `rustdoc_shape` group, opt-in |
| `doc_issue_reference` | Module and item documentation cite no issue number; `rustdoc_shape` group, opt-in |
| `module_doc_budget` | A crate root's documentation fits 16 KiB, any other module's 8 KiB, a lead 1.5 KiB and a section 2 KiB; `rustdoc_shape` group, opt-in |
| `module_doc_duplication` | A paragraph of 25 words or more is written once, in the README or in one module; `rustdoc_shape` group, opt-in |
| `doc_history_prose` | Reports the phrases that narrate a past in module and item documentation; a report selected with `--force-warn`, outside the group |
| `module_doc_measure` | Prints each documented module's bytes, lead, largest section and decision records; opt-in |
| `reverse_after_build` | A `Vec` just built by `collect`, `to_vec`, `clone` or `split_off`, or a suffix `extend` just appended, is built in its final order instead of reversed |
| `front_shift_in_loop` | A loop does not shift a `Vec`'s every element through `remove(0)` or `insert(0, _)`; `VecDeque` is accepted |
| `linear_search_in_loop` | A loop does not scan a `Vec` or slice by equality per element of another collection; arrays are accepted |
| `count_against_constant` | A `count()` compared with a constant stops after the constant, unless the iterator is `ExactSizeIterator` |
| `collect_read_once` | A vector is not collected only to be read once by a move or copy, where `needless_collect` does not reach |
| `bucket_per_key` | A map whose buckets are only searched for one entry does not allocate a growable bucket per key through `entry(..).or_default().push(..)`; grouped and bulk-filled maps and inline-first buckets are accepted |
| `allocation_in_repeated_body` | A loop or per-element closure does not allocate a buffer, box, string or collection on every pass only to drop it before the next; a value moved out of the pass, a buffer never grown and a panic's message are accepted |
| `clone_in_loop` | A loop or per-element closure does not deep-copy, through `clone`, `cloned`, `to_vec` or `to_owned`, a value owning a heap allocation only to drop the copy before the next pass; a copy that leaves the pass, a diverging path, and `Copy`, plain-data, `Rc` and `Arc` values are accepted |
| `allocation_free_region` | A function whose rustdoc `# Cost` section carries `- allocation: none in steady state` reaches no allocation through any call the build can read, follows into `std` and other crates' MIR, and refuses a trait object, a function pointer or an unmarked trait method of a type parameter; a marked trait declaration holds every implementation to it |
| `pool_only_threads` | Opt-in, `allow` by default. No call outside the tests starts a thread or a pool beside the consumer's work pool: `std::thread::spawn`, `scope` and `Builder`'s spawns, rayon's `ThreadPoolBuilder::new`, `scope`, `in_place_scope`, `spawn`, `join`, their FIFO forms and broadcasts; a site that owns its thread states why with `#[expect(pool_only_threads, reason = "...")]` at the site, and an inherited, unreasoned or `allow` exception is refused |

The plugin is a policy floor, not a proof of totality or complete semantics. Call edges erased by function-pointer coercion, compiler-generated drop behavior, and unresolved type relationships require the corresponding review or evidence boundary. The ownership and call analyses address different mechanisms; neither subsumes the other.

The eight cost-shape rules are `Deny` by default and type-directed: the receiver's type decides, and a form cheap on its receiver is never refused. `src/cost_shape.rs` states the shared repetition and placement analysis; each lint's module states its cost claim, its accepted shapes and the Clippy lint it complements. A `#[spec]` clause is left to `spec_clause_traversal`.

The primitive-signature rule does not establish a wrapper's meaning, visibility, conversions, or absence policy. `option_signature` checks absence exposure separately; neither rule proves that a chosen wrapper or reason enum models the domain correctly.

The primitive-signature rule reads authorship from the item's name: a declaration under a name the author wrote is the author's, and one under a name a foreign macro made up is not. `cxx` gives the shims behind a shared struct carried in a `Vec`, behind a `SharedPtr`, and behind each `extern "Rust"` function the author's spans, but names them itself, so their `usize` lengths and primitive wire shapes stay outside the rule. A bridge's own declarations keep the author's names and stay under it, including those whose signature `cxx` writes out itself, as it does for a function returning a `Result`. A crate-local `macro_rules!` expansion stays under it too.

### Arithmetic and absence activation

`primitive_arithmetic`, `option_signature`, and `option_field` are opt-in lints; the nine rules without an activation section retain their default levels. Select them explicitly in the consumer's Dylint invocation or through `cfg_attr(dylint_lib = "quenchant_dylints", deny(primitive_arithmetic, option_signature, option_field))` on its crate roots. Registration alone does not enable any of the three. The producer's UI suites deny both the selected predicate and unknown lint names; a missing predicate cannot satisfy their expected diagnostics.

### Primitive arithmetic

The predicate rejects integer `+`, `-`, `*`, `/`, `%`, shifts, their assignment forms, and unary negation. It also rejects compiler-resolved inherent integer methods in the `checked`, `strict`, `wrapping`, `saturating`, `overflowing`, and `unchecked` arithmetic families, including UFCS, type aliases, autodereferenced receivers, and function-item references. The inspected operations are addition/subtraction and their signed/unsigned forms, multiplication, division/remainder including Euclidean forms, negation, powers, absolute value, shifts, signed/unsigned differences, next multiples/powers of two, integer square root, and integer logarithms.

Unprefixed partial arithmetic methods obey the same rule: `pow`, `abs`, `div_euclid`, `rem_euclid`, `div_floor`, `div_ceil`, `next_multiple_of`, `next_power_of_two`, `ilog`, `ilog2`, `ilog10`, and signed `isqrt`. Their overflow, zero-divisor, or invalid-domain inputs make them partial even where Clippy emits no diagnostic. Funnel shifts and their unchecked forms also require the arithmetic boundary. Total operations such as unsigned `isqrt`, `unsigned_abs`, `abs_diff`, and `midpoint` remain accepted. The arithmetic library currently supplies the five binary operation families; rejection of another primitive operation does not imply that a corresponding library adapter exists.

Method definition identity and primitive operand types are both checked. The compiler's language items identify `Add`, `Sub`, `Mul`, `Div`, `Rem`, `Shl`, `Shr`, their assignment traits, and `Neg`. Method calls, UFCS, and function-item references to those traits are rejected when every instantiated input is a primitive integer after peeling references, including the mutable receiver of assignment methods. Binary and assignment operators likewise require primitive integer operands on both sides: a nominal right-hand operand can define a legitimate overload even when the left-hand operand is an integer. Same-spelled nominal methods, unrelated extension-trait methods, and nominal overloaded operators remain accepted. Boolean/bitwise operations and comparisons are outside this predicate, except the partial funnel-shift operations above. Floating-point arithmetic, conversions, other non-arithmetic partial APIs, and the justification of modular or clamping semantics remain separate policy obligations. Existing Clippy denials complement this predicate; they do not establish coverage of its method inventory.

The representation boundary is an implementation of the local `quenchant_arith::arith::Integer` trait for the local `quenchant_arith::arith::Int<primitive integer>` type. Those methods implement the library's explicit arithmetic families and therefore must reach primitive operations. Closures within the method retain that boundary; nested functions, unrelated producer helpers, different traits, and different nominal self types do not. This is canonical definition-path recognition, not a crate-wide exemption or an authenticity check on a package bearing that name.

Resolved function items remain visible, including before pointer coercion. An already-erased function pointer has no recoverable method identity. Generic operator inputs that remain unresolved during body type checking are not treated as primitive; this pass does not recheck generic bodies at monomorphization. Rustc's external-expansion diagnostic filtering remains a reporting boundary. Passing this lint is not a proof that overflow is unreachable or that a strict operation cannot panic.

### Raw pointers

A reference asserts alignment, a valid value and, for `&mut`, uniqueness, and a pointer derived from it inherits those assertions. Beside `unsafe` code the memory may be uninitialized, shared with a device, or aliased by another raw pointer that the new reference invalidates. `raw_pointer_through_reference` refuses four origins:

- a borrow coerced to a raw pointer through an unsizing step, such as `&words` passed as `*const [u32]`; the suggestion is `&raw const words`;
- a reference value coerced to a raw pointer; the suggestion is `ptr::from_ref(r)`, `ptr::from_mut(r)`, or `ptr::from_mut(r).cast_const()`;
- a slice's `as_ptr` or `as_mut_ptr` whose receiver borrows an owned place, a place behind owned boxes, or a place behind a raw pointer, with or without an explicit `&mut`; the suggestion is `(&raw mut place).cast::<T>()`, dereferencing through each box method resolution passed, so `(&mut boxed).as_mut_ptr()` becomes `(&raw mut *boxed).cast::<T>()`;
- a fresh borrow passed to `ptr::from_ref` or `ptr::from_mut`; the suggestion is `&raw const place` or `&raw mut place`. When the constructor's type parameter unsizes the borrow, as in `ptr::from_ref::<[u32]>(&words)`, the suggestion casts to the call's pointer type, `&raw const words as *const [u32]`, because `&raw` keeps the place's own type and `cast` cannot unsize. The replacement is parenthesized where it is a method receiver or a field or index base.

Every suggestion is machine-applicable except a constructor cast whose pointee names an item, such as `dyn Trait` or a struct, which may not be in scope as printed; the UI suite applies the machine-applicable ones and recompiles the result. A receiver or argument that is already a reference value, or a place reached through one or through an overloaded dereference such as `Rc`'s, creates no new borrow `&raw` could avoid and is accepted. Vectors' own `as_ptr` and `as_mut_ptr` are accepted because they take no reference to the elements. The plain implicit coercion `let p: *mut T = &mut x;` is Clippy's `borrow_as_ptr`, which this lint leaves to it; a consumer relying on that form needs Clippy's `pedantic` group or that lint. Macro-expanded code and borrows of temporaries are not inspected.

### Absence signatures

The predicate identifies the standard `Option` through rustc's diagnostic-item identity. It checks all visibilities of free, const, async, and extern functions, inherent methods, local trait declarations/defaults/implementations, and foreign declarations. Inferred closure signatures are implementation details rather than authored APIs.

Normalization exposes resolvable aliases and associated substitutions. Inspection follows references, pointers, tuples, arrays, slices, function pointers, generic arguments, dynamic-trait bounds, and explicit opaque/future bounds. It stops at a local transparent nominal type or the canonical `quenchant_shape::shape::Maybe`; it does not inspect representation fields. Unresolved projections are not a claim of checked absence.

The shape producer registers `Maybe` as the `quenchant_maybe` rustc diagnostic item under the internal `quenchant_compiler_policy` cfg. The predicate compares the resolved `DefId`, so dependency aliases and re-exports preserve the boundary while an unrelated same-path definition does not acquire it. An unmarked rlib receives no canonical exemption. This compiler-owned identity is not package authentication: deliberately registering the reserved diagnostic item is a producer declaration, and duplicate registrations are compiler errors.

Policy builds must pass `--cfg=quenchant_compiler_policy` through Cargo's whole-graph compiler flags, including external shape dependencies. Dylint uses a workspace wrapper; its own `dylint_lib` cfg does not reach external dependencies. `mise run check:dylint` preserves `CARGO_ENCODED_RUSTFLAGS` when present, including an empty value; otherwise it encodes the whitespace-separated `RUSTFLAGS`, then appends the policy cfg. This composes with instrumentation flags such as `--cfg anodized_panic`. The cfg is expected by the producer's lint configuration and is confined to policy builds.

Later consumer activation must adopt that flag composition alongside the reviewed plugin and shape revisions, then explicitly enable `option_signature` and `primitive_arithmetic`. Enabling only the lint does not identify a prebuilt unmarked shape library. Ordinary and `--all-features` runtime-library builds need no compiler attributes or nightly-only Cargo feature. This separation is revisited only if the selected compiler or invocation cannot propagate the cfg reliably without changing those ordinary builds; no name fallback or global bootstrap substitutes for identity.

A foreign trait implementation is paired with the foreign method's unsubstituted signature. Only corresponding required `Option` layers are admitted. `Iterator::next` may return its required `Option<Item>`, but substituting `Option<Value>` for `Item` introduces another refused layer. `From<Option<Value>>::from` likewise introduces absence where the foreign declaration has a generic parameter and remains refused. Required callback, dynamic, and future shapes follow the same rule. A comment justifying primitive unpacking does not waive a prohibited signature.

`option_field` applies the same traversal to the fields of crate-defined structs and enum variants. A field is wire form, and keeps its `Option`, when its item implements serde's `Serialize` or `Deserialize` or clap's `FromArgMatches`: a serialized `Option` is a value the writer has not supplied and a parsed `Option` is an argument the user did not pass, so the protocol is the reason. The compiler's implementation index decides, so a hand-written implementation counts and a derive on a neighbouring type does not. A `#[repr(transparent)]` wrapper over `Option` is a nominal boundary for `option_signature` and still answers to `option_field` for its own field: the wrapper's author names the reason, or the wrapper is a `Maybe`.

The signature walker instantiates an opaque type's item bounds to reach its associated outputs and the trait's own arguments; the opaque itself, the bound's `Self`, is never re-entered, and visited types are compared up to regions, so the fresh regions rustc mints on each instantiation cannot make the same structure look new. Both rules terminate on any signature.

### Hand-rolled `Maybe` and erased errors

`maybe_shape` reads a crate-authored enum at its definition: exactly two variants, each carrying one value whose type is a type parameter of the enum, the two parameters distinct. That is `Maybe`'s shape, and `Result`'s, under another name. The canonical `Maybe` is excluded by its `quenchant_maybe` diagnostic identity. The definition-site check reports each hand-rolled type once rather than at every signature that names it.

When checking the producer itself, the exact local compiler definition `quenchant_shape::shape::Maybe` is also excluded without requiring the policy cfg: crate, root module, and item must all match. Other definitions in that crate, nested lookalikes, and the same module/item names in another crate remain checked. This producer-only exemption does not change the diagnostic identity required at external absence-signature boundaries.

A unit variant beside a type-parameter payload, `Direction<Expected> { Synthesise, Check(Expected) }`, is not reported: the discipline models a closed state set as one enum, and the unit variant's name is its reason. A per-site enum that fixes its reason type, `Found(Value)` beside `Missing(Reason)` with a concrete reason, is a closed state enum by the same reading. A generic two-way choice that is neither absence nor failure has `Maybe`'s shape and is reported; it allows the lint at the item with its reason. If per-site specializations turn out to stand in for `Maybe` in practice, the shape widens to a parameter payload beside a fieldless local reason enum.

`erased_error_signature` reads the output of every crate-authored function and method, through aliases, nested types, and what an `impl Trait` or `async fn` output declares, for a `Result` whose error is a `dyn Error` behind `Box`, `Arc`, `Rc`, or a reference, or is `anyhow::Error` or `eyre::Report`. A named error that carries an erased source is a named error. Parameters are not read: a function that accepts any error in order to report it returns nothing erased. A method implementing a foreign trait answers to that trait. A `--test` compilation is exempt as a whole, because test code reports failures and never matches on them; a library's unit-test build is also checked by its ordinary build, and an integration-test crate has no other. A foreign trait's associated error type, such as `FromStr::Err`, is chosen at the implementation and is not read; if erasure enters that way in practice, associated error types join the rule.

Both are `allow` by default and are selected like the specification gates, at a crate root through `cfg_attr(dylint_lib = "quenchant_dylints", deny(maybe_shape, erased_error_signature))`.

Single-character lifetime names are not a plugin rule: Clippy's `single_char_lifetime_names` already refuses them where a workspace denies it, as this one does.

## Answer totality

A presenter — a printer, a hover — promises an answer at every node it is handed and a spelling its grammar reads back. A corpus sweep checks the promise at run time; the `answer_totality` group checks what the compiler can see of it, over tables the consumer writes in `dylint.toml`. Each rule reads nothing until its table is configured, and is `deny` by default once it is. [`src/answer_totality.rs`](src/answer_totality.rs) records the shared decisions; each rule's module records its own.

```toml
[quenchant-dylints]
dishonest_constructors = ["printer::former::Former::Unreadable", "printer::former::Unspelled"]
dishonest_spellings = [{ text = "?", crates = ["printer", "hover"] }]
presenters = [
  { enum = "printer::former::Former", functions = ["printer::walk::Walker::visit", "printer::walk::Walker::form"] },
]

[quenchant-dylints.productions]
marker = "printer::walk::spells"
argument = 0
rule_constructor = "RuleName"
rule_sources = ["grammar/src/rules.rs"]
unproduced = [{ rule = "shebang", reason = "a printer writes terms; a shebang is no term" }]
```

- `dishonest_answer` reports each place that builds a listed placeholder: a constructor path — a variant, an enum standing for all its variants, or a struct — reached as a value, or a string or character literal of a listed spelling in a listed crate. Patterns, operands of `==` and `!=`, test code and external macro expansions build nothing.
- `presenter_coverage` reports each variant of a listed enum that no pattern of its listed functions names; a wildcard or a bare binding names nothing. A presenter keyed by data rather than by a Rust enum — node kinds read as strings from a grammar table, a hover answering by typed span — has no such table, and its coverage stays with the runtime sweep.
- `mold_coverage` runs in the crate defining the marker. It collects the literal at `argument` of every call of the marker, function or tuple struct, and compares that set with the rule names the builders give `rule_constructor`, scanned token by token from `rule_sources` outside every `#[cfg(test)]` item. A source path is relative to the directory the compiler runs in, which Cargo makes the workspace root, so the table reads the same on every machine; the files are listed among the crate's dependencies, so a builder edit lints the printer again. A production naming no rule is reported where it is written; a rule with no production is reported with its builder's file and line unless `unproduced` states why no presenter writes it; a stale `unproduced` entry is refused.

A finding is excepted only where the residue is built: `#[expect(answer_totality, reason = "…")]`, or the rule's own name, on the site or on a node between it and its enclosing item. Under Dylint the attribute rides on `cfg_attr(dylint_lib = "quenchant_dylints", expect(...))`. An expectation on an enclosing module or crate, an expectation without its reason, and an `allow` written in the source are refused; a level set on the command line is the consumer's selection of the rule. The group is a plain lint name rather than a `quenchant::` tool path, because a tool namespace needs the unstable `register_tool` in every consumer crate.

## Structured documentation

`# Specification` states an item's behavior. `# Adequacy` states a falsifiable evidence hypothesis and names witnesses. The plugin checks source shape and authorship; [quenchant-gates](../quenchant-gates/README.md) separately resolves witness names against runnable tests in the owning package and target.

A generated sibling can reuse an author's identifier token without becoming an authored declaration. The presence analysis therefore inspects both declaration and name provenance. The specification UI fixtures use actual facade expansion and a separate fixture macro to distinguish these cases. Generated assertion failures still carry evidence and require classification; generation alone is not an exclusion.

`# Termination` accompanies an approved recursive exception. Its reason, measure, boundedness, and input-recursion statements are independent obligations. Inherited lint configuration is not inherited approval. The analysis can refute some claims against discovered argument flow, but a well-shaped block is not a termination proof.

`# Safety` and its `- unsafe invariants:` clause apply to every authored unsafe function, trait, implementation, and extern block. The pre-expansion pass sees extern blocks inside `#[cxx::bridge]` before cxx consumes them. Cxx rejects doc attributes on those blocks, so their bridge module carries the section instead. The pass does not inspect unsafe items introduced after expansion or prove the stated invariants; one module section can cover multiple C++ blocks.

### Executable predicates and adequacy presence

`spec_attribute_present` and `adequacy_present` apply to the items `specification_present` checks, once the block is well formed and states a clause: anything but `trivial.`. Such a block owes two things:

- an executable predicate: a `#[spec(...)]` attribute with at least one `requires:`, `maintains:`, or `ensures:` clause, or an exemption in the block. The attribute is matched by its path's last segment, so `anodized::spec`, a `cfg_attr`-applied attribute, and a nested marker inside a `#[spec]` trait count.
- an `# Adequacy` section naming the tests that distinguish a violation of the clauses. `adequacy_block_grammar` checks its shape; `quenchant-gates` resolves its witnesses.

The proposed exemption is one clause, the block's last before an optional `- intension:`:

```text
/// - executable: none — <why no runtime predicate expresses the obligation>
```

`none`, the em dash, and a nonempty reason are required. A second exemption, a clause after it, or an exemption beside a `#[spec]` that states a predicate is refused. The exemption sits in the block rather than in a lint `allow` so that it travels with the clauses it excuses and renders in rustdoc; review weighs the reason, and the gate checks only that one is stated.

A predicate that cannot fail checks nothing, and is refused at its span:

- a literal `true`, also as `!false`, behind parentheses, or as an output closure's body;
- an expression compared with itself by `==`, `<=`, or `>=`;
- a type check: `matches!` against a wildcard or against every variant of `Option` or `Result`, or a disjunction of complementary queries such as `is_ok() || is_err()`;
- a predicate repeated token for token under the same clause key.

The gate reads attributes before expansion, because expansion consumes `#[spec]`, and decides in the late pass, which owns authorship and the test and derive classes. Items a crate-local `macro_rules!` expands carry no recorded attribute, because the pre-expansion pass sees the macro's tokens rather than its items; such an item states the exemption in its template.

Neither gate decides whether a predicate expresses the clause beside it, whether it holds for a reason its syntax does not show (`count >= 0` on an unsigned count), whether an exemption's reason is true, or whether a named witness distinguishes anything. The enforcing build, the witness resolver, and review own those. Identity with the signature is read syntactically, within one clause key: a predicate that restates what the parameter types already guarantee needs type information and stays a review question.

### Specification attribute spelling

`spec_attribute_unqualified` requires imported `#[spec(...)]` and `#[spec_helper]` attributes instead of qualified paths. It joins authored paths to resolved macro identities, so leading `::`, crate aliases, and re-exported paths cannot evade it. Unrelated macros with either name are ignored. Active `cfg_attr` paths, including nested conditions, are checked.

The lint is `allow` by default. Select it with `-D spec_attribute_unqualified` or at a crate root:

```rust
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
```

Item and module lint levels still apply. A bare renamed import is outside the rule: it checks qualification, not import names. Inactive attributes, macro-generated attributes, and nested markers consumed without their own macro expansion are not resolved authored invocations and are not checked. The UI matrix uses the facade as `anodized` and checks the backend macro separately; it covers qualified paths, conditional paths, methods and traits, imports, unrelated macros, default activation, and scoped allowances.

The resolved check follows surviving item definitions. Run it with the facade backend enabled to check `spec_helper`: feature-off erasure leaves no definition to resolve. Local `let` statements likewise supply no item definition; their spelling remains a source-review obligation.

### Specification clause cost

An enforcing build evaluates every clause on every call. A clause that walks state accumulated across calls costs as much as everything the state has seen: a frontier query that scans every frontier runs in quadratic time over a parse. `spec_clause_traversal` refuses four walks of a standard container reached through `self` or a `&mut` parameter, whether the clause makes them itself or a function it calls makes them:

- iterates: a method call whose result is an iterator, or a `for` loop;
- searches: `contains` or `is_sorted`, `is_sorted_by`, `is_sorted_by_key` on a sequence (`Vec`, `VecDeque`, `String`, slice, `str`);
- copies: `clone`, `to_owned`, or `to_vec` of a container;
- compares: `==` or `!=` between containers, or `eq` and `ne` calls.

Bounded reads pass: `len`, `last`, `get`, `binary_search`, map and set `contains`. So do walks over a fixed-size array, a shared input or the call's output. Clauses are found by source span inside recorded `spec` attributes, both plain and under `cfg_attr`, because the attributes do not survive expansion.

A call in a clause is followed. Each function body the clause reaches is summarized from its MIR: the parameters, and for a closure the captured values, whose reachable state the body walks, directly through the standard library or through the bodies it calls in turn, across crates. The summary covers closures the body builds and excludes the callee's own clauses, which are checked where they are written. A call that hands rooted state to a position its callee walks is refused, with a note at the walk.

Following stops at a function whose rustdoc states its bound:

```rust
/// The entries recorded after `mark`, summed.
///
/// # Clause cost
/// - bound: reads only the entries recorded after `mark`.
pub fn since(&self, mark: usize) -> u32 { /* ... */ }
```

That is how a combinator that checks only what changed since a captured point declares itself; the section documents the claim where readers look. A heading without a non-empty `- bound:` is refused at the function. A trait method that resolves to no single body, such as a call on a type parameter or through `dyn`, is refused when handed rooted state unless its declaration states the bound every implementation keeps. Rustdoc crosses crates in metadata for exported items, which are the only foreign functions a clause can call.

Foreign bodies are read from metadata, which carries every function's MIR only when the whole graph is built with `-Zalways-encode-mir`. The policy invocation also passes `-Zinline-mir=no`, so that standard-library calls remain calls in optimized profiles. On gandr's workspace the two flags cost about 1.4 % CPU and 22 % more checked metadata in a cold check, and following adds about 1 %. A callee whose MIR is missing is refused with that remedy rather than passed silently.

There are three remedies: check the structural claim once where the state is built, check only what the call returns, or call a combinator that states its bound. A walk that stays bounded in practice is an exception: `#[expect(spec_clause_traversal, reason = "...")]`. The lint is `allow` by default; select it at a crate root:

```rust
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_clause_traversal))]
```

### Specification gate activation

The predicate and adequacy-presence gates are `allow` by default. A consumer crate opts in at its root once its backlog is cleared:

```rust
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_present, adequacy_present))]
```

This workspace denies all five executable-specification lints through `DYLINT_RUSTFLAGS` in `check:dylint`. CI invokes the same task across every workspace target. Crate roots carry no duplicate activation; UI fixtures retain their independent lint levels. Plugin defaults remain unchanged for external consumers.

The compiler plugin's internal predicates check input membership, parsing bounds and roundtrips, compiler identities, input domains, and monotone state transitions rather than reconstructing an expected result by replaying the implementation. Classifiers whose bodies are their specifications instead name the UI fixtures as their oracle. Other item-local exemptions cover compiler effects without a per-call observation, shared-index operations without a stable post-call snapshot, and fallible configuration reads.

Consumers may use module-scoped lint levels during a rollout. After the backlog is cleared, workspace-wide `DYLINT_RUSTFLAGS` keep activation in one place without changing plugin defaults. If the exemption grammar cannot state an item's obligations, revise the grammar before enabling the gate.

### Module documentation shape

The README's budget sends detail into rustdoc; the `rustdoc_shape` group holds that rustdoc in turn. A module's `//!` documentation is a lead paragraph, then sections from one vocabulary, each within a byte budget; documentation states what the code does, so it carries no decision record and cites no issue, and a paragraph is never copied between the README and the crate's documentation. [`src/doc_shape.rs`](src/doc_shape.rs) states each rule, its vocabulary and its caps. A test compilation is not read.

Every rule is `allow` by default. A consumer denies the group in its `DYLINT_RUSTFLAGS` (`-D rustdoc_shape`), selects the history report beside it (`--force-warn doc_history_prose`, which denied warnings cannot turn into a refusal), and carries an `allow` per crate, under `cfg_attr(dylint_lib = "quenchant_dylints", …)`, where its documentation does not yet meet the shape. `-W module_doc_measure` prints the sizes of every documented module.

## Ownership exceptions

A foreign generic known not to own its arguments can be listed in `dylint.toml` under `[quenchant-dylints].non_owning_generics`. Each entry needs its canonical type path and a concrete justification. An unjustified entry is refused; unknown foreign generics remain conservatively owning. No allowlist entry supplies a missing semantic proof by itself.

## Specification dependency records

When the configuration names a directory, the plugin also writes what each executable specification depends on, for [`quenchant-gates obligations`](../quenchant-gates/README.md#re-verification-obligations):

```toml
[quenchant-dylints]
evidence_directory = "/absolute/path/to/target/evidence"
```

Each compilation unit writes one JSON-lines file there: an `item` record for every function, method, constant, static, type, trait, variant and field (definition path, file, start line and column, extent from its first attribute or documentation line, documentation lines, and a fingerprint of its source text), and a `clause` record per specified item and clause key listing the items the clause names. Names are the definitions the compiler resolved inside the clause: paths, struct literals and patterns, method calls resolved to their implementation, field reads. Locals, the standard library and the instrumentation are not dependencies, and a callee's body is not followed. An integration test, benchmark or binary prefixes its own paths with `package/`, since its crate name need not be unique in a workspace. The passes are not registered without the setting, and they emit no diagnostics; a failed write is a compilation error. [`src/evidence.rs`](src/evidence.rs) records the decisions; the `ui_evidence` fixtures pin the record shape.

The setting can also arrive through the `DYLINT_TOML` environment variable. Every unit lists that variable, set or unset, among its inputs, so Cargo lints a unit again when a run switches the records on or off; a unit left fresh would keep whatever record it had.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
