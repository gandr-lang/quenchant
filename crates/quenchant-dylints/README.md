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

The plugin is a policy floor, not a proof of totality or complete semantics. Call edges erased by function-pointer coercion, compiler-generated drop behavior, and unresolved type relationships require the corresponding review or evidence boundary. The ownership and call analyses address different mechanisms; neither subsumes the other.

The primitive-signature rule does not establish that a wrapper's meaning, visibility, conversions, or absence policy is correct. The shared policy has obligations beyond this predicate, including its treatment of `Option`; passing this lint alone does not establish compliance with the whole policy.

## Structured documentation

`# Specification` states an item's behavior. `# Adequacy` states a falsifiable evidence hypothesis and names witnesses. The plugin checks source shape and authorship; [quenchant-gates](../quenchant-gates/README.md) separately resolves witness names against runnable tests in the owning package and target.

A generated sibling can reuse an author's identifier token without becoming an authored declaration. The presence analysis therefore inspects both declaration and name provenance. The specification UI fixtures use actual facade expansion and a separate fixture macro to distinguish these cases. Generated assertion failures still carry evidence and require classification; generation alone is not an exclusion.

`# Termination` accompanies an approved recursive exception. Its reason, measure, boundedness, and input-recursion statements are independent obligations. Inherited lint configuration is not inherited approval. The analysis can refute some claims against discovered argument flow, but a well-shaped block is not a termination proof.

## Ownership exceptions

A foreign generic known not to own its arguments can be listed in `dylint.toml` under `[quenchant-dylints].non_owning_generics`. Each entry needs its canonical type path and a concrete justification. An unjustified entry is refused; unknown foreign generics remain conservatively owning. No allowlist entry supplies a missing semantic proof by itself.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
