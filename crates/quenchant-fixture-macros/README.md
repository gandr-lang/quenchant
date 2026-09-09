# quenchant-fixture-macros

The attribute macros the Dylint UI matrix expands, so a rule about foreign expansions is read against a real one.

`quenchant-dylints`'s gates decide whether an item is the author's own syntax. rustc answers that with span hygiene, and hygiene only says "external" when the macro really is defined in another crate. A fixture cannot stage that from inside the crate under test, and a macro written in the lint crate itself would answer the opposite way, so the shapes those gates are read against live here.

The crate is a workspace member, so the lint wall covers its own source, and it is a development dependency of `quenchant-dylints` alone. Nothing outside the UI matrix expands it.

## Provision

- **`client`.** Re-emits the annotated inherent `impl` unchanged and generates a client `impl` beside it. Each generated method is named by passing the author's own method identifier token through, so its name span is the author's while its declaration is the macro's. The generated methods carry no rustdoc.

That combination is the shape a presence rule cannot decide from a name alone: the name reads as authored, and no author can document the declaration it names.

## License

Apache-2.0 OR Apache-2.0 WITH LLVM-exception.
