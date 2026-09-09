# quenchant-dylints

Reusable Dylint rules that enforce Rust boundaries where Clippy has no opinion.

Each rule mechanizes a convention the Rust coding conventions state in prose. A convention a tool checks survives review fatigue; the failures these rules catch are quiet ones — a bare primitive at a crate boundary compiles perfectly and erases the meaning of the value crossing it, and a recursive cycle compiles perfectly and overflows the stack on real input.

Load-bearing invariants are gated on two planes. `recursion_forbidden` reads the call graph and has recorded blind spots there; `recursive_owned_pointer` reads the type graph and closes the class those blind spots leave open, because a type that cannot be defined has no drop glue and no derived traversal left to recurse through.

Specification attributes use `anodized` at revision `981bc892c979fd59c46aaaa2dc3075b8184f035f`, with defaults disabled. Its `#[spec]` entry point combines predicate fields, evaluates function bodies in closures before postconditions, and preserves author documentation without injecting predicate prose.

Three more read rustdoc off the items the crate authors. `specification_present` requires the `# Specification` block itself, so an item cannot arrive with its specification undecided; the `# Adequacy` block states an item's test plan, and the `# Judgement` block marks the checking judgement's own scrutinees. A gate that reads a declaration written beside the code is the one that stays correct as the code moves: the mark travels with the definition, and removing it is an edit a reviewer reads.

The crate is a workspace member and carries no dependency on any language crate.

## Provision

- `single_field_struct_needs_transparent_repr`. Every single-field named or tuple struct declares `#[repr(transparent)]`. The policy uses single-field structs as semantic domain wrappers; stating the transparent representation keeps the layout specification explicit while the wrapper stays nominal at the type boundary.

- `primitive_signature`. A function or method signature in a workspace-owned API does not expose a Rust primitive — directly, or beneath references, pointers, tuples, arrays, slices, generic containers, or type aliases — before reaching a local `#[repr(transparent)]` type. The sole exception is a method implementing a trait defined in an external crate whose required signature contains primitives. The rule establishes only that the signature reaches a local nominal transparent boundary; the wrapper's field visibility, conversions, and documentation remain Clippy's and review's.

- `recursion_forbidden`. Every function in a crate-local recursive strongly connected component is denied. Rust guarantees no tail-call optimization, so recursion whose depth scales with input is a latent stack overflow; the iterative form — worklist, explicit frame stack, loop — is the sanctioned shape.

- `recursive_owned_pointer`. Every locally defined ADT that owns its way back to itself is denied. Drop glue and derived traversals recurse over the pointee at a depth that scales with the data, and neither is a function the call plane can see; denying the type denies the glue. The sanctioned shape is the flat id-addressed form — a child is an index into an arena or interning table.

- `adequacy_block_grammar`. An item's `# Adequacy` rustdoc section, wherever one is present, states exactly one `- hypothesis:` bullet naming a rung of the adequacy ladder (`L0` types, `L1` evidence, `L2` agreement, `L3` pointwise), followed by at least one `- witness:` bullet naming an exact backticked test path — or, on a required trait method alone, one reasoned `- declaration-only:` bullet and no witness. Whether an item must carry a block stays with review; whether a named witness resolves to exactly one runnable test needs the workspace test inventory and is the `quenchant-gates` G0 gate.

- `mode_dispatch_wildcard`. A wildcard or bare-binding arm is denied in a match whose scrutinee is the checking judgement's direction, or a parameter declared to carry the type a term is checked against. A bidirectional judgement forces each rule's direction by the syntactic class of the term, so a fallback arm answers for the cases the judgement has no rule for — it turns an absent rule into a silent synthesis. The crate that declares the judgement names its own scrutinees in a `# Judgement` rustdoc block; where no such block exists the rule reports nothing.

- `specification_present`. Every function and method the crate authors carries a `# Specification` rustdoc block: free functions, inherent methods, trait impl methods, provided and required trait methods, foreign declarations, and functions a crate-local `macro_rules!` expands. The specification is authored before the body it governs, so a body arriving without one records that nothing was decided about what it owes. Where an item has nothing to state, the block's whole body is `trivial.`, and the rule denies that marker written beside any other line or written as a bullet. Outside the rule: `#[test]` functions, `#[automatically_derived]` items, closures, and any item whose own name is not this crate's syntax — a macro from another crate, or the test harness's generated entry point.

All seven are declared at `deny`.

## Specification expansion

`#[spec]` and `#[anodized::spec]` resolve to the same macro in `anodized_macros`. Predicate fields such as `requires`, `ensures`, and `maintains` need no separate imports.

Synchronous bodies run through `anodized::__::eval_once`; asynchronous bodies run through an awaited async closure. `?` and explicit `return` therefore return to the generated postcondition checks. Runtime tests distinguish the exact `postcondition failed` panic from an unrelated failure.

The macro preserves author doc attributes without adding predicate documentation. Native `missing_docs` remains responsible for missing public documentation; the workspace's Clippy wall governs private items. Hand-written `#[doc]` remains ordinary author documentation.

An attribute macro's whole-item span can suppress a lint as externally generated. `recursion_forbidden` reports at the function's author-spanned name when needed; signature findings already use the author's type tokens. The real-expansion matrix checks that both gates survive.

## The recursion exception

An exception has two parts, and the lint checks both:

1. `#[expect(recursion_forbidden, reason = "...")]` on the item itself. The `reason` is mandatory: an expectation without one is reported. Lint levels are inherited, but approval is not — an expectation on an enclosing module or on the crate root approves nothing inside it, and a recursive function that relies on one is reported for exactly that.
2. A `# Termination` rustdoc section on the same item, with exactly four bullets, in this order, each carrying a value:

   ```rust
   /// # Termination
   /// - reason: why recursion is the appropriate control structure here.
   /// - measure: the quantity that strictly decreases on every recursive edge.
   /// - boundedness: where the finite or well-founded bound comes from.
   /// - input recursion: none.
   ```

A bullet value may wrap across source lines; continuation lines fold into the bullet above them. The section terminates at the next heading of any level, so a doc block appended after the author's prose — as attribute macros that re-emit their predicates as `#[doc]` lines do — cannot complete a short section by accident, whether it opens with `#` or nests under `##`.

`- input recursion: none.` is a checkable claim, and the lint refutes it: when some call inside the recursive component passes an argument derived from the calling function's own parameters, the exception is denied. Any other value states that the recursion does descend on caller input, and passes on its face. The claim is the value's first word taken on its own boundary, so prose written after it explains the claim without weakening it, and no choice of punctuation — `none?`, `none!`, `none —` — turns the claim into a declaration. Prose that merely opens with the word, as in `none of the arguments descend`, is not the claim.

When the exception is claimed but unjustified, the report is emitted at the crate root rather than at the item, because the item's own `#[expect]` would otherwise swallow the report that its exception is unjustified.

Outside a Dylint run the lint name is unknown to rustc, so a suppression in ordinary workspace code is written under the driver's cfg:

```rust
#[cfg_attr(
    dylint_lib = "quenchant_dylints",
    expect(recursion_forbidden, reason = "...")
)]
```

## The specification block

`specification_present` asks one question of every function and method the crate writes — is there a `# Specification` block — and two refinements of it. The two accepted shapes are the clause list in its fixed order and the marker alone:

```rust
/// Convert a zero-based protocol coordinate into a one-based coordinate.
///
/// # Specification
/// - requires: `value` is a zero-based coordinate from an external protocol.
/// - ensures: returns the successor of `value`.
/// - panics: none.

/// Return the number of nodes in the arena.
///
/// # Specification
/// trivial.
```

The canonical `trivial` body is `trivial.`, with the terminal period, because clippy's `doc_paragraphs_missing_punctuation`, denied in the workspace lint wall, refuses a doc paragraph that ends without one. The rule also accepts `trivial` without the period, and in either case no bullet marker and no clause stands beside it. Those are the two remaining denials. The marker beside another line is one: a block claiming at once that there is nothing to specify and that here is the specification has stated neither. The marker behind a bullet is the other: `- trivial` reads as one oddly named clause rather than as a claim about the whole block, so it satisfies neither shape while the author who wrote it believes the item is marked.

Three item classes are outside the rule, and each has its fixture. A `#[test]` function states its subject in its name and its specification in its assertions; an `#[automatically_derived]` item is a derive expansion whose rustdoc the deriving type's author cannot write; a closure is not an item and carries no rustdoc, so a closure inside a documented function is covered by that function's block. Beyond those, an item is exempt exactly when its own syntax is not this crate's: neither its name nor its declaration is text the author wrote. That covers a function a macro from another crate emits and the entry point the test harness generates. The declaration is the item's **signature** span rather than its whole item span, because rustc counts every attribute macro as external and the item span carries the attribute: reading the item span would take every `#[spec]`-expanded function out of the rule with it. `ui_specifications/specification_presence.rs` is the fixture that pins that against a real expansion.

A name span whose source text is not the item's own identifier is exempt on the same terms: the `#[doc(hidden)]` `__anodized_*` sibling a specification attribute on a trait definition synthesizes out of the author's method name is manufactured rather than authored, while the author's own declaration at that same span stays inside the rule, and `ui_specifications/specification_trait_expansion.rs` pins both halves.

Both spans are asked because a macro from another crate can manufacture either half alone. It can build the name, which the name span's source text answers; it can also build the declaration while passing the author's own identifier through into it, which the signature span answers. The second shape is a generated client method named after the method it forwards to: the name reads as authored, and the declaration it names is nobody's prose to write. `ui_specifications/specification_generated_sibling.rs` pins it, with the authored method of the same expansion and name still reported beside it. Reversal: a consumer appears whose generated siblings warrant documentation, which makes the declaration test too broad and moves the decision to an allow-list of macro sources.

A function a crate-local `macro_rules!` expands is **not** exempt. The author writes that macro's body, so the block is written there once and every expansion carries it; the denial is reported at the invocation that produced the item, which is where a reader finds the macro.

Section boundaries follow the same rule as `# Termination` and `# Adequacy`: the block ends at the next heading of any level, so a `# Adequacy` block written under the marker adds no clause beside it.

## The adequacy block

The block is the item's test plan and the reviewer's index into its evidence, and each defect the rule denies reads as a discharged obligation while discharging nothing:

```rust
/// # Adequacy
/// - hypothesis: L3 — where the decision surface sits on the ladder, and why
///   that rung carries it.
/// - witness: `module::tests::the_test_a_reviewer_watches_fail`
```

The rung is found on its own alphanumeric token boundary, so `` `L1` `` and `(L2)` name rungs and `HL21` does not. A witness value is one nonempty backtick-delimited path and nothing else, because the resolution gate reads the path verbatim against the test inventory; prose, an unquoted path and two paths on one bullet are all denied here rather than silently failing to resolve there.

`- declaration-only:` is the exemption for a required trait method, which has no body a mutant could change. It must state a reason, it may not appear twice, and it may not be combined with witnesses — a block claiming both says at once that there is nothing to witness and here is the witness. On any item with a body, including a provided trait method, the exemption is denied.

Section boundaries follow the same rule as `# Termination`: the block ends at the next heading of any level, so bullets a rustdoc-injecting attribute macro appends after the author's prose cannot complete a short section. Within the block, only an **indented** line continues the bullet above it; an unindented line that is not a bullet — an ordinary paragraph, or a markdown link-reference definition written under the block — belongs to no bullet and is dropped rather than folded into the last witness.

## The judgement declaration

`mode_dispatch_wildcard` binds inside the crate that declares the checking judgement, and that crate says so on the items carrying the judgement's scrutinees. Two bullets exist, each on the item that owns what it declares:

```rust
/// The direction of the checking judgement.
///
/// # Judgement
/// - direction: the two modes, forced by the term's syntactic class.
enum Direction {
    Synthesises,
    Checks,
}

/// Check a value against the type it is offered at.
///
/// # Judgement
/// - expected: `expected`
fn check_value(term: Term, expected: TypeHead) -> Verdict {
    // every arm below names the case it handles
}
```

`- direction:` marks a type definition, and a match whose scrutinee has that type — references peeled — is gated. `- expected:` marks a function and names, in backticks, one of its parameters; a match in that function whose scrutinee *mentions* the parameter is gated, whether it reads the parameter directly or through a call that resolves it. Mentioning it is the question the discipline asks: an arm that reached its answer by inspecting what the term is checked against chose its mode from the expected type.

**The expected plane names a parameter rather than a type**, and deliberately. A checker's expected type is the core language's type vocabulary, owned by a crate the judgement never edits, so a gate keyed on the type would need a mark in a crate outside the judgement's own text — and would fire on every unrelated match over a core type besides. Keying on the declared parameter keeps mark and rule together and asks the narrower question.

A wildcard arm, an arm bound to a bare name, and a fallback alternative hidden inside an or-pattern are each denied; a binding carrying a subpattern (`checked @ Direction::Checks`) names its case and is not. A guard changes nothing: an arm that does not name its case is the fallback whatever else it tests.

Two scope facts belong beside that. The gate is **crate-local by construction** — the direction's `DefId` must resolve local and the declaration is read off local rustdoc — so a wildcard match on the judgement's direction written in a *consuming* crate is not denied. That is the intended scope: the constraint is the declaring crate's own specification, and code downstream of the judgement is not writing the judgement. And a `match` inside a **macro expansion** is not reported: the case that motivates the exemption is `matches!(direction, ..)`, which carries a fallback arm the author never wrote and asks a boolean question rather than choosing a mode, but the exemption is wider than that case and a crate-local `macro_rules!` hiding a fallback arm rides it too. Narrowing it would mean telling an author's own expansion from a dependency's, and a diagnostic reported into a macro body names a span the author of the *call* cannot act on.

Every way of writing the declaration without declaring anything is denied on its own: a section stating neither bullet, a `- direction:` with no value, an `- expected:` whose value is not one backticked name, an `- expected:` naming a parameter the function does not take, either bullet on the item that cannot carry it, and a section on an item holding no scrutinee at all. Each reads as an opted-in gate and gates nothing, which is the failure mode an opt-in mechanism has and a loud one does not.

A declaration *marks* a scrutinee only on a type definition or on a function with a body, which is where the judgement's scrutinees are. It is **read everywhere**: an associated const, an associated type, a body-less required trait method and a foreign function each draw the misplacement denial rather than silence. A required trait method is the case worth naming — its parameters bind nothing a match can dispatch on, and an implementation's parameters are the implementation's own, so the declaration belongs on the implementation that has the body. Reading the section nowhere would be exactly the silence this rule exists to refuse, reached by the route of never looking.

## The ownership graph

`recursive_owned_pointer` asks one question of every locally defined ADT: *does a field's type reach the defining type again, through owned positions only?* It is a question about the field's **type**, never about its spelling. A `Box` whose pointee returns only through an arena index closes no cycle, and a `Vec<Self>` closes one without naming a pointer at all. Both directions of the spelling census are errors, and the over-detecting direction is the more dangerous one, because an extra finding reads as thoroughness while the work it authorizes is flattening something already flat.

The graph is therefore ownership reachability over ADT definitions, and the denial set is its recursive strongly connected components. Nothing in the rule names `Box`, `Rc`, or `Arc`: Rust rejects an ADT of infinite size, so every cycle in this graph already passes through an indirection, and which one it was belongs to the diagnostic rather than to the verdict. Naming the container would also make the rule wrong in both directions at once — `Vec<Self>` is recursion the named-type floor cannot see, and `Box<[Row]>` where `Row` holds only an id is not recursion at all.

A position is owned when dropping the whole value drops what sits there.

| Position                                                        | Verdict            |
| --------------------------------------------------------------- | ------------------ |
| array and slice elements, pattern types, tuple components       | owning, descend    |
| type arguments of an ADT at an owned parameter                  | owning, descend    |
| references, raw pointers, function pointers, function items     | non-owning, stop   |
| closures, trait objects, unnormalized projections               | non-owning, stop   |
| `PhantomData`, `std::rc::Weak`, `std::sync::Weak`               | non-owning by name |
| every entry of the configured allow-list                        | non-owning by name |

An index newtype needs no rule: it holds an integer and reaches nothing, so the flat representation passes by construction.

**Owned parameters.** `Wrapper<Node>` owns a `Node` when `Wrapper`'s parameter occupies an owned field position, and owns nothing when the parameter only ever appears in a `PhantomData` — the shape of every typed arena index. Which of the two holds is computed for crate-local ADTs as a least fixed point over their own fields, so a typed `Id<Node>` stored inside `Node` is not a cycle.

An ADT defined outside the crate has no such analysis available, and reading its fields would be worse than not reading them: `Vec`'s own fields bottom out in a raw pointer, so a walk through them would accept every `Vec<Self>` in the workspace. Every parameter of an external ADT is therefore taken as owned. That is why a bare `*mut Node` forms no edge while a `NonNull<Node>` does — the raw pointer states its non-ownership in the type, and the wrapper around it does not.

## The non-owning allow-list

The allow-list is the evidence channel that overrides the conservative external rule. It lives in the workspace's `dylint.toml`, and every entry states both the def path and the evidence for it:

```toml
[quenchant-dylints]
non_owning_generics = [
  { path = "std::ptr::NonNull", justification = "holds a raw pointer; its drop never touches the pointee" },
]
```

`path` is rustc's def-path string for the type. An entry stating no `justification`, or an empty one, is refused rather than applied and reported at the crate root, so the type it names stays conservatively owning: a malformed allow-list denies rather than admits.

## Known blind spots

`recursion_forbidden` is a call-plane gate over crate-local HIR call edges, and it is a floor rather than a proof: a cycle whose edges the HIR does not carry is not denied.

- **Derived-trait recursion.** `Clone`/`PartialEq`/`Hash`/`Debug` on a recursive owned type routes through non-local `std` generics, so no crate-local edge exists.
- **Drop glue.** Compiler-generated destructors have no HIR function at all.
- **Function pointers.** Coercing a function item to `fn(..) -> ..` erases its definition id, so a cycle closed through a function-pointer binding or static records no edge. `ui/recursion.rs::fn_pointer_indirection` is the fixture that demonstrates the miss; its absence from `ui/recursion.stderr` is the record.

Two cases that look like the third but are seen: a function *item* held in a binding keeps its `FnDef` type and is resolved (`ui/recursion.rs::fn_item_indirection`), and a call written inside a closure counts as a call by the function that builds the closure (`ui/recursion.rs::closure_mediated`). The latter is an over-approximation — the closure may never be invoked — and that is the safe direction for a ban with an explicit escape hatch.

The first two are closed on the type plane: `recursive_owned_pointer` denies the type, so the derived traversal and the drop glue that would have recursed have no definition to exist on. The third is not, and the residual is the function-pointer cycle written between non-recursive types.

`mode_dispatch_wildcard` is a floor on two counts.

- **Arms only.** `if let`, `let .. else` and `while let` have none: their else branch is a fallback the HIR spells as a conditional, and it is not reported. `ui/mode_dispatch.rs::if_let_fallback` is the fixture that demonstrates the miss, and its absence from `ui/mode_dispatch.stderr` is the record.
- **The whole scrutinee.** The direction plane is a type-identity test on the scrutinee's own type, so `match (direction, term) { .., _ => .. }` — the mode-by-term-class dispatch a bidirectional checker writes by reflex — is not gated: the tuple's type is not the declared type, references peeled or not. The expected plane does not reach it either, because its search is for a declared expected parameter and never for the direction. `ui/mode_dispatch.rs::tupled_scrutinee` is that fixture, and its silence in the same `.stderr` is that record.

The judgement's own shape is what narrows both: the faces are separate functions taking separate terms, so a mode chosen anywhere is a mode chosen over a scrutinee the gate can see. Closing the tuple case on the lint plane would mean descending into the scrutinee's component types and pairing each with the arm position that matches it — a second, structural plane, which the gate set opens only where a load-bearing invariant needs one.

`recursive_owned_pointer` is a floor of its own. A cycle closed through a **trait object**, a **closure capture**, or a **raw pointer** is not denied, because none of the three names the type it reaches: `ui/owning_pointer.rs::DynCycle` is the fixture that demonstrates the miss, and its absence from `ui/owning_pointer.stderr` is the record. `Box<dyn Any>` reaching back is undecidable rather than merely unimplemented; the raw-pointer case is the deliberate boundary the allow-list is calibrated against. A field whose type is an unnormalized associated projection is likewise not followed.

`specification_present` is a presence gate, and presence is nearly all it decides.

- **The clause grammar.** A block whose body is empty satisfies the rule, as does one whose clauses are misspelled or out of order. Only the marker's own two shapes are read; a clause list is read for nothing beyond its presence.
- **The wrong item.** The rule asks each item for its own heading and never whether the prose beneath describes that item, so a block copied onto a neighbour passes.
- **Foreign expansions.** An item whose name is not this crate's syntax is exempt by design, and the exemption is wider than the cases that motivate it: a function a macro from another crate emits carries no obligation even where that macro's caller could have documented it. The narrower question — which foreign expansions the caller can reach — is not one the HIR answers.

## Toolchain

The lint library is a `cdylib` that links rustc's internal libraries, so it builds only under a nightly. The workspace pins that one toolchain in the root `rust-toolchain.toml` — the nightly the current stable release matches, read from rust-clippy's release tag — and `clippy_utils` is pinned at the workspace root to the same release's tag, so the two move together under `mise run toolchain:bump <stable>`: its own reviewed change with the full gate set re-run.

The whole workspace builds under that one toolchain, so the ordinary workspace commands reach this crate:

```console
cargo build --workspace --all-targets
```

The crate-local `.cargo/config.toml` sets `linker = dylint-link`. Cargo discovers configuration from the build's working directory upward, so the setting applies only to builds invoked from this directory — which is exactly what the UI harness does, and what the cdylib needs, since Dylint loads the artifact by its toolchain-suffixed name.

**The `cargo` on `PATH` must be rustup's proxy.** A distribution `cargo` — Homebrew's, for one — ignores the tree's `rust-toolchain.toml` and builds with its own bundled `rustc`, which has no `rustc_private` libraries; the build then fails inside `clippy_utils` with a wall of unresolved imports rather than with anything naming a toolchain. Where a distribution `cargo` shadows the proxy, invoke `~/.cargo/bin/cargo` and put `~/.cargo/bin` first on `PATH`:

```console
PATH="$HOME/.cargo/bin:$PATH" ~/.cargo/bin/cargo test
```

## Running it

```console
cargo dylint --lib quenchant_dylints --no-deps -- --workspace --all-targets
(cd crates/quenchant-dylints && cargo test)
```

The test command runs from the package directory rather than through `-p quenchant-dylints`: the crate-local `dylint-link` setting is resolved from the working directory, and the UI harness reuses that directory for its own builds.

Both need `dylint-link` and `cargo-dylint` on `PATH` and the pinned toolchain installed with the `rustc-dev` and `llvm-tools` components. Keep `CARGO_INCREMENTAL=0` for driver runs: the driver reproducibly crashes on reused incremental state.

`cargo test` runs the UI matrix under `ui/` — one fixture per rule, each paired with the exact diagnostics it must produce — plus the matrix under `ui_config/`, which runs the same driver against a supplied `dylint.toml` and pins both allow-list behaviours: a justified entry admits its type, an entry stating no justification is refused and the type it names stays denied. Beside them are unit tests for the strongly connected component search, the ownership cycle rendering, and the rustdoc section reader.

A third matrix, `ui_specifications/`, compiles real expansions from other crates: imported and qualified `#[spec]`, conditional attributes, `?` and explicit returns, async bodies, native missing-documentation findings, complete and truncated termination sections, specification presence through an expansion and through a specified trait definition, boundary gates on both sides of it, and the method `quenchant-fixture-macros` generates under an author's own identifier.

`ui/specification_present.rs` is compiled under `--test`, declared in its own `// compile-flags:` header: `#[test]` functions and `#[cfg(test)]` modules exist only in a test compilation, so the exclusion and the helper beside it can be exercised no other way. The generated harness entry point is in that fixture too, and its silence is the record of the foreign-expansion exemption. A fixture whose subject is another rule silences this one on its crate-level `allow` line, as `ui/recursion.rs` already silences `unconditional_recursion`.

`dylint_testing` compiles a `src_base` fixture without dependencies. Example targets would make intentionally rejected fixtures part of the workspace gate run, so the harness supplies each dev-dependency artifact explicitly, with `--edition=2024`. `build.rs` records the compiler version; the harness selects an artifact carrying that stamp rather than accidentally loading another compiler's, by kind — an rlib for `anodized`, the host dynamic library for the proc-macro fixture crate.

`specification_tests` executes early-exit paths in ordinary and enforcing lanes. `mise run check:anodized-enforcing` requires the invocation's panic cfg and runs these tests with graph-wide `RUSTFLAGS`. A target-only panic flag cannot replace runtime evidence from the actual macro artifact.

The workspace gate run covers this crate too: the gate library holds itself to its own rules, so every signature here crosses a nominal boundary (see `src/semantic.rs`), every function and method here carries its `# Specification` block, nothing here recurses, and no type here owns its way back to itself.

## Theory relied on

None of its own. The rules mechanize this project's conventions: nominal typing at a module boundary, where a wrapper's distinct name is the point rather than its representation, the termination argument as an explicit obligation attached to any approved recursive definition, and flat id-addressed data as the representation that makes destruction and duplication total by construction.
