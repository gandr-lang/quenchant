//! Derive a Verus proof input from each declared specification and check it.
//!
//! No predicate is authored here. Each unit is assembled by relocating the
//! source's own tokens: the nominal declarations, the specification attribute's
//! `captures` bindings and `ensures` closure body, and the function's unchanged
//! signature and body. The derived postcondition is attached as
//! `ret => ensures (predicate)`, the form the verifier reads, and the verifier
//! is the commissioned binary `VERUS_BIN` names.
//!
//! # Every specification-bearing function is named
//!
//! A verdict over the functions carrying a specification attribute would be
//! vacuous over an empty set, and silent about a function whose rustdoc states
//! a specification that no attribute carries. The run therefore names three
//! populations, each read off the source rather than a list kept here: the
//! functions whose attribute declares a predicate, which reach the verifier;
//! the functions whose `# Specification` block declares a `requires` or
//! `ensures` clause with no attribute to derive from; and the `macro_rules!`
//! bodies whose expansions carry a block that no top-level item does.
//!
//! # Declared translations
//!
//! One translation is stated here rather than inferred from the source:
//!
//! - A `captures` binding is evaluated on entry by the specification attribute.
//!   In the derived unit it becomes a `let` inside the postcondition, where a
//!   by-value parameter still denotes its entry value. The two agree for a
//!   capture over by-value parameters, which is every capture this crate
//!   declares.
//!
//! A `reason_enum!` site is relocated whole: its `sealed` submodule, the
//! sealed supertrait, the enum, and both implementations. A derived signature
//! bounded by that site's reason trait therefore ranges over the source's own
//! sealed reason domain and nothing wider.
//!
//! A refusal is a recorded verdict rather than an operational failure: it says
//! the derived clause alone does not discharge under the verifier's own
//! obligations.

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use proc_macro2::TokenStream;
use quote::ToTokens as _;
use quote::quote;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;

/// Why a derivation stopped before producing a checked verdict.
#[derive(Debug)]
enum DerivationError
{
    /// A source, output, or verifier operation failed.
    Operational(String),
    /// A source construct falls outside the supported derivation.
    Rejected(String),
    /// A source file did not parse as Rust.
    Syntax(syn::Error),
}

impl core::fmt::Display for DerivationError
{
    /// Report which half of the derivation refused, and what it read.
    ///
    /// # Specification
    /// - ensures: names the refusing half and carries the underlying report
    ///   unchanged.
    /// - fails: propagates the formatter's own write failure unchanged.
    /// - panics: none.
    ///
    /// # Errors
    /// - `core::fmt::Error`: the formatter's sink refused the write.
    ///
    /// # Adequacy
    /// - hypothesis: L3 pointwise — the three arms differ only in the prefix
    ///   they write, so each is separated by its own variant and asserted as
    ///   the exact rendered string; the write failure is separated by a sink
    ///   that refuses, observed as the propagated error and the one prefix the
    ///   sink recorded.
    /// - witness: `tests::each_refusal_renders_its_half`
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        match *self {
            | Self::Operational(ref report) => {
                f.write_str("operation failed: ")?;
                f.write_str(report)
            },
            | Self::Rejected(ref report) => {
                f.write_str("unsupported source construct: ")?;
                f.write_str(report)
            },
            | Self::Syntax(ref error) => core::fmt::Display::fmt(error, f),
        }
    }
}

impl core::error::Error for DerivationError
{
}

impl From<std::io::Error> for DerivationError
{
    /// Carry an input, output, or process failure as an operational one.
    ///
    /// # Specification
    /// trivial.
    fn from(error: std::io::Error) -> Self
    {
        Self::Operational(error.to_string())
    }
}

impl From<syn::Error> for DerivationError
{
    /// Carry a parse failure with its own span report.
    ///
    /// # Specification
    /// trivial.
    fn from(error: syn::Error) -> Self
    {
        Self::Syntax(error)
    }
}

/// The answer to one classification question about a source construct.
#[repr(transparent)]
struct Answer(bool);

/// One entry of a specification attribute's argument list.
enum SpecEntry
{
    /// An entry-evaluated binding the postcondition reads.
    Capture
    {
        /// The name the postcondition reads.
        binder: syn::Ident,
        /// The expression evaluated on entry.
        value: syn::Expr,
    },
    /// The postcondition, written as a closure over the returned value.
    Ensures(syn::ExprClosure),
}

impl Parse for SpecEntry
{
    /// Read one labelled clause of the specification attribute.
    ///
    /// # Specification
    /// - requires: `input` is positioned at the start of one attribute entry.
    /// - ensures: reads a `captures` binding and an `ensures` closure in the
    ///   spelling the attribute uses, and consumes exactly that entry.
    /// - provides: the only reader of the attribute's surface, so an unknown
    ///   clause stops the derivation rather than being dropped unseen.
    /// - fails: any other label, and any entry whose shape does not parse.
    /// - panics: none.
    ///
    /// # Errors
    /// Reports the unsupported label at its own span.
    ///
    /// # Adequacy
    /// - hypothesis: L3 pointwise — the decision surface is the label equality
    ///   and the two entry shapes; the accepted labels are separated by the
    ///   exact binder and value a `captures` entry relocates and the exact
    ///   parameter an `ensures` closure binds, and every other label by the
    ///   exact refusal message.
    /// - witness: `tests::a_capture_entry_carries_its_binding`
    /// - witness: `tests::a_postcondition_entry_carries_its_closure`
    /// - witness: `tests::an_unknown_label_refuses_derivation`
    fn parse(input: ParseStream<'_>) -> syn::Result<Self>
    {
        let label: syn::Ident = input.parse()?;
        input.parse::<syn::Token![:]>()?;
        if label == "captures" {
            let binder: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            let value: syn::Expr = input.parse()?;
            return Ok(Self::Capture { binder, value });
        }
        if label == "ensures" {
            return Ok(Self::Ensures(input.parse()?));
        }
        Err(syn::Error::new(
            label.span(),
            "only `captures` and `ensures` entries are derived",
        ))
    }
}

/// A function whose declared specification reaches the verifier.
struct Target
{
    /// The function's own name, as the verdict reports it.
    name: syn::Ident,
    /// The complete Rust unit the verifier reads.
    unit: TokenStream,
}

/// A specification-bearing construct the derivation does not carry to the
/// verifier.
struct Unreached
{
    /// The construct's own name.
    name: syn::Ident,
    /// Why the derivation stops short of a proof input here.
    reason: String,
}

/// Everything one run read out of the two source files.
struct Derivation
{
    /// The functions carrying a derived postcondition.
    targets: Vec<Target>,
    /// The specification-bearing constructs the derivation names but does not
    /// carry.
    unreached: Vec<Unreached>,
}

/// Derive every proof input, check each one, and report its verdict.
///
/// # Specification
/// - requires: `VERUS_BIN` names the commissioned verifier, and the working
///   directory is the crate root.
/// - ensures: writes one unit per derivable function under `target/verus`, runs
///   the verifier on each, and reports one line per specification-bearing
///   construct in either source file; a verifier refusal is reported, never
///   raised.
/// - provides: the per-item record the specification sweep owes.
/// - fails: an absent verifier, an unreadable source file, an unsupported
///   attribute shape, and any output failure.
/// - panics: none.
///
/// # Errors
/// Reports the operational failure or the rejected source construct.
///
/// # Adequacy
/// - hypothesis: L2 agreement plus L3 boundary — the whole report is pinned
///   against a stub verifier external to this adapter, over this crate's own
///   two source files, so a dropped, reordered, or misnamed line diverges; the
///   absent-verifier boundary is separated by a child process with the variable
///   removed and asserted as the exact operational report.
/// - witness: `tests::main_reports_every_construct`
/// - witness: `tests::main_reports_an_absent_verifier`
fn main() -> Result<(), DerivationError>
{
    let verifier = PathBuf::from(std::env::var_os("VERUS_BIN").ok_or_else(|| {
        DerivationError::Operational(
            "set VERUS_BIN to the commissioned verifier before deriving".to_owned(),
        )
    })?);
    let shape = syn::parse_file(&std::fs::read_to_string("src/shape.rs")?)?;
    let tests = syn::parse_file(&std::fs::read_to_string("src/tests.rs")?)?;
    let mut relocated = declarations(&shape)?;
    relocated.extend(declarations(&tests)?);
    let mut derivation = derive(&shape, &relocated)?;
    let from_tests = derive(&tests, &relocated)?;
    derivation.targets.extend(from_tests.targets);
    derivation.unreached.extend(from_tests.unreached);
    let directory = Path::new("target/verus");
    std::fs::create_dir_all(directory)?;
    let mut report = Vec::new();
    for target in &derivation.targets {
        let path = directory.join(format!("{}.rs", target.name));
        std::fs::write(&path, format!("{}\n", target.unit))?;
        report.extend(verdict(&verifier, &target.name, &path)?.into_bytes());
    }
    for construct in &derivation.unreached {
        report.extend(
            format!("{}: not reached — {}\n", construct.name, construct.reason).into_bytes(),
        );
    }
    std::io::stdout().write_all(&report)?;
    Ok(())
}

/// Check one derived unit and read the verifier's own answer.
///
/// # Specification
/// - requires: `path` holds a complete derived unit and `verifier` is
///   executable.
/// - ensures: reports the verified verdict on success, and otherwise the
///   verifier's first reported error line verbatim.
/// - provides: the verifier's answer, never this adapter's opinion of it.
/// - fails: the verifier could not be run at all.
/// - panics: none.
///
/// # Errors
/// Reports a verifier that could not be launched.
///
/// # Adequacy
/// - hypothesis: L3 pointwise over the answers a stub verifier produces — a
///   zero exit, a refusal whose first `error` line is the payload, and a silent
///   refusal — each asserted as the exact reported line; the verifier that
///   cannot run is separated by a path that does not exist and asserted as the
///   exact operational report.
/// - witness: `tests::a_verifier_answer_is_reported_verbatim`
/// - witness: `tests::an_unlaunchable_verifier_is_operational`
fn verdict(
    verifier: &Path,
    name: &syn::Ident,
    path: &Path,
) -> Result<String, DerivationError>
{
    let output = Command::new(verifier)
        .arg("--crate-type=bin")
        .arg(path)
        .output()?;
    if output.status.success() {
        return Ok(format!("{name}: verified\n"));
    }
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    let first = diagnostics
        .lines()
        .find(|line| line.starts_with("error"))
        .unwrap_or("the verifier reported no error line")
        .to_owned();
    Ok(format!("{name}: not reached — {first}\n"))
}

/// Collect the nominal declarations a derived unit makes Verus-visible.
///
/// # Specification
/// - ensures: emits each top-level enum and struct with its non-doc attributes
///   dropped, emits each `reason_enum!` site in the translated form this file's
///   header declares, and ignores every other item kind.
/// - provides: declarations relocated from the source, never restated.
/// - fails: a `reason_enum!` site that is not one module carrying exactly one
///   enum.
/// - panics: none.
///
/// # Errors
/// Reports the site whose shape the derivation does not support.
///
/// # Adequacy
/// - hypothesis: L0 the generated enum, struct, and reason site are usable by a
///   real Rust consumer; duplicated or omitted declarations, retained
///   unsupported attributes, and emitted unrelated items fail compilation.
///   Documentation preservation remains an authored source-review obligation.
/// - witness: `tests::relocated_nominals_preserve_type_and_seal_boundaries`
fn declarations(file: &syn::File) -> Result<TokenStream, DerivationError>
{
    let mut emitted = TokenStream::new();
    for item in &file.items {
        match *item {
            | syn::Item::Enum(ref declaration) => {
                let mut declaration = declaration.clone();
                declaration.attrs.retain(|attribute| is_doc(attribute).0);
                emitted.extend(declaration.to_token_stream());
            },
            | syn::Item::Struct(ref declaration) => {
                let mut declaration = declaration.clone();
                declaration.attrs.retain(|attribute| is_doc(attribute).0);
                emitted.extend(declaration.to_token_stream());
            },
            | syn::Item::Macro(ref invocation)
                if invocation.mac.path.is_ident("reason_enum") && invocation.ident.is_none() =>
            {
                emitted.extend(reason_site(&invocation.mac.tokens)?);
            },
            | _ => {},
        }
    }
    Ok(emitted)
}

/// Translate one `reason_enum!` site into Verus-visible declarations.
///
/// # Specification
/// - requires: `tokens` are the site's module, carrying exactly its reason
///   enum.
/// - ensures: emits the module, its `sealed` submodule, the sealed reason
///   trait, the enum with non-doc attributes dropped, and both implementations,
///   so the relocated bound admits the site's own enum and nothing else.
/// - provides: the one macro surface a derived target's signature reaches.
/// - fails: a site that is not one module carrying exactly one enum.
/// - panics: none.
///
/// # Errors
/// Reports the site shape the derivation does not support.
///
/// # Adequacy
/// - hypothesis: L0 a real Rust consumer can use the relocated reason enum but
///   cannot implement its reason trait for an unrelated type. This establishes
///   the Rust type boundary, not correspondence of arbitrary Verus proofs. L3
///   rejected site shapes remain distinguished by the refusal witness.
/// - witness: `tests::relocated_nominals_preserve_type_and_seal_boundaries`
/// - witness: `tests::an_unsupported_site_is_refused`
fn reason_site(tokens: &TokenStream) -> Result<TokenStream, DerivationError>
{
    let site: syn::ItemMod = syn::parse2(tokens.clone())?;
    let name = &site.ident;
    let Some((_brace, ref items)) = site.content
    else {
        return Err(DerivationError::Rejected(
            "a reason site declares its enum inline".to_owned(),
        ));
    };
    let &[syn::Item::Enum(ref reason)] = items.as_slice()
    else {
        return Err(DerivationError::Rejected(
            "a reason site carries exactly one enum".to_owned(),
        ));
    };
    let mut reason = reason.clone();
    reason.attrs.retain(|attribute| is_doc(attribute).0);
    let implementer = &reason.ident;
    Ok(quote! {
        /// A closed absence site, relocated from its `reason_enum!` invocation.
        pub mod #name
        {
            /// Seal the reason set to the enum declared at this site.
            mod sealed
            {
                /// Marker implemented only by this site's reason enum.
                pub trait Sealed {}
            }

            /// A reason belonging to this closed absence site.
            pub trait Reason: sealed::Sealed {}

            #reason

            impl sealed::Sealed for #implementer {}
            impl Reason for #implementer {}
        }
    })
}

/// Whether an attribute is a doc comment.
///
/// # Specification
/// trivial.
fn is_doc(attribute: &syn::Attribute) -> Answer
{
    Answer(attribute.path().is_ident("doc"))
}

/// Whether an attribute is the specification attribute, in either spelling.
///
/// # Specification
/// - requires: `attribute` is one item's own attribute.
/// - ensures: answers affirmatively for exactly the two paths `spec` and
///   `quenchant::spec`, and negatively for every other path, so a `spec` scoped
///   to another crate never enters the derivation.
/// - provides: the one place the accepted spellings are named.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 pointwise, exhaustive over the accepted set and its
///   one-segment near misses: both accepted spellings, a `spec` under another
///   path, a longer path carrying an accepted prefix, and the bare crate
///   segment, each asserted as the exact answer.
/// - witness: `tests::two_attribute_spellings_are_accepted`
fn is_specification(attribute: &syn::Attribute) -> Answer
{
    let segments: Vec<String> = attribute
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    Answer(segments == ["quenchant", "spec"] || segments == ["spec"])
}

/// Whether a rustdoc block states a specification inside its specification
/// section.
///
/// # Specification
/// - requires: `attrs` are one item's attributes, doc comments included.
/// - ensures: answers affirmatively exactly when a `- requires:` or `-
///   ensures:` bullet stands under the `# Specification` heading; a bullet
///   under a later heading answers negatively, so an adequacy hypothesis or an
///   error enumeration is never read as a specification.
/// - provides: the presence half of the report — an item stating a
///   specification with no attribute to derive from is named rather than
///   skipped.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 pointwise, exhaustive over the finite class of heading and
///   bullet positions: a `requires` bullet and an `ensures` bullet under `#
///   Specification`, the same bullet under a later heading, the `trivial.`
///   body, and a summary with no block, each asserted as the exact answer.
/// - witness: `tests::a_clause_counts_under_its_heading`
fn declares_clause(attrs: &[syn::Attribute]) -> Answer
{
    let mut inside = false;
    for attribute in attrs {
        let syn::Meta::NameValue(ref pair) = attribute.meta
        else {
            continue;
        };
        if is_doc(attribute).0 {
            let syn::Expr::Lit(ref literal) = pair.value
            else {
                continue;
            };
            let syn::Lit::Str(ref text) = literal.lit
            else {
                continue;
            };
            let line = text.value();
            let line = line.trim();
            if line == "# Specification" {
                inside = true;
                continue;
            }
            if line.starts_with("# ") {
                inside = false;
                continue;
            }
            if inside && (line.starts_with("- requires:") || line.starts_with("- ensures:")) {
                return Answer(true);
            }
        }
    }
    Answer(false)
}

/// Derive one file's proof inputs and name every specification it cannot carry.
///
/// # Specification
/// - requires: `declarations` already carries every nominal declaration this
///   file's specification-bearing signatures reach.
/// - ensures: emits one unit per function whose attribute declares a
///   postcondition; names every other function that states a specification; and
///   names each `macro_rules!` definition whose body carries a specification
///   block, because its expansions are specification-bearing while no top-level
///   item is. A function inside a function body is not reached at all: it is
///   neither a top-level item nor a macro definition.
/// - provides: the derivation this run reports.
/// - fails: an attribute whose argument list does not parse, and a
///   postcondition closure that does not bind exactly one plain output name.
/// - panics: none.
///
/// # Errors
/// Reports the attribute the derivation does not support.
///
/// # Adequacy
/// - hypothesis: L2 agreement — the target names and the unreached
///   name-and-reason pairs are pinned exactly over a file carrying a derivable
///   function, a bare attribute, a block-only specification, a
///   specification-free function, a method, and a specification-bearing
///   `macro_rules!` definition, so a skipped population or a swapped reason
///   diverges.
/// - witness: `tests::derivation_names_every_population`
fn derive(
    file: &syn::File,
    declarations: &TokenStream,
) -> Result<Derivation, DerivationError>
{
    let mut derivation = Derivation {
        targets: Vec::new(),
        unreached: Vec::new(),
    };
    for item in &file.items {
        match *item {
            | syn::Item::Fn(ref function) => collect(
                &mut derivation,
                declarations,
                None,
                &function.attrs,
                &function.sig,
                &function.block,
            )?,
            | syn::Item::Impl(ref block) => {
                let generics = &block.generics;
                let self_ty = &block.self_ty;
                let container = quote!(impl #generics #self_ty);
                for member in &block.items {
                    if let syn::ImplItem::Fn(ref method) = *member {
                        collect(
                            &mut derivation,
                            declarations,
                            Some(&container),
                            &method.attrs,
                            &method.sig,
                            &method.block,
                        )?;
                    }
                }
            },
            | syn::Item::Macro(ref definition) => {
                let Some(ref name) = definition.ident
                else {
                    continue;
                };
                if definition
                    .mac
                    .tokens
                    .to_string()
                    .contains("# Specification")
                {
                    derivation.unreached.push(Unreached {
                        name: name.clone(),
                        reason: "the specification is written in a `macro_rules!` body, so every \
                                 expansion carries it and no top-level item does"
                            .to_owned(),
                    });
                }
            },
            | _ => {},
        }
    }
    Ok(derivation)
}

/// Record one function as a derived target or as a specification not carried.
///
/// # Specification
/// - requires: `container` carries the enclosing impl header for a method, and
///   nothing for a free function.
/// - ensures: derives a unit from an attribute that declares a postcondition;
///   names a function whose attribute declares none; names a function whose
///   block states a specification with no attribute at all; and records nothing
///   for a function that states no specification.
/// - provides: the single place a function enters the derivation, so a
///   specification-bearing item cannot be skipped unseen.
/// - fails: an attribute the derivation does not support.
/// - panics: none.
///
/// # Errors
/// Reports the attribute the derivation does not support.
///
/// # Adequacy
/// - hypothesis: L2 agreement on the emitted unit, pinned exactly for a method
///   so the impl header, the relocated declarations and the derived attribute
///   are all observed; L3 pointwise over the inputs that carry no derivable
///   predicate — a bare attribute, an empty argument list, a block-only
///   specification, and a function stating no specification — each asserted as
///   its exact record or as no record at all.
/// - witness: `tests::a_method_unit_carries_its_header`
/// - witness: `tests::a_specification_without_a_predicate_is_named`
fn collect(
    derivation: &mut Derivation,
    declarations: &TokenStream,
    container: Option<&TokenStream>,
    attrs: &[syn::Attribute],
    signature: &syn::Signature,
    body: &syn::Block,
) -> Result<(), DerivationError>
{
    let name = signature.ident.clone();
    let Some(attribute) = attrs.iter().find(|attribute| is_specification(attribute).0)
    else {
        if declares_clause(attrs).0 {
            derivation.unreached.push(Unreached {
                name,
                reason:
                    "the block states a specification that no specification attribute carries, so \
                         there is no declared predicate to derive"
                        .to_owned(),
            });
        }
        return Ok(());
    };
    let syn::Meta::List(_) = attribute.meta
    else {
        derivation.unreached.push(Unreached {
            name,
            reason: "the specification attribute declares no predicate".to_owned(),
        });
        return Ok(());
    };
    let entries =
        attribute.parse_args_with(Punctuated::<SpecEntry, syn::Token![,]>::parse_terminated)?;
    if entries.is_empty() {
        derivation.unreached.push(Unreached {
            name,
            reason: "the specification attribute declares no predicate".to_owned(),
        });
        return Ok(());
    }
    let predicate = predicate(&entries)?;
    let documentation: Vec<&syn::Attribute> = attrs
        .iter()
        .filter(|attribute| is_doc(attribute).0)
        .collect();
    let checked = quote! {
        #(#documentation)*
        #[verus_spec(__anodized_output => ensures (#predicate))]
        #signature
        #body
    };
    let relocated = match container {
        | Some(header) => quote! { #header { #checked } },
        | None => checked,
    };
    let unit = quote! {
        use vstd::prelude::*;

        verus! {
            #declarations
        }

        #relocated

        fn main() {}
    };
    derivation.targets.push(Target { name, unit });
    Ok(())
}

/// Assemble the derived postcondition from the attribute's own tokens.
///
/// # Specification
/// - requires: `entries` are one attribute's parsed argument list.
/// - ensures: emits a block that binds each `captures` name to its own
///   expression, binds the closure's output name to the verifier's return
///   binder, and ends in the closure's unchanged body; no predicate token is
///   authored here.
/// - provides: the `ret => ensures (predicate)` payload.
/// - fails: an argument list with no postcondition, with more than one
///   postcondition, or with a closure that does not bind exactly one plain
///   output name.
/// - panics: none.
///
/// # Errors
/// Reports the argument list the derivation does not support.
///
/// # Adequacy
/// - hypothesis: L2 agreement on the assembled block, pinned exactly for an
///   argument list carrying one capture and one postcondition, so a dropped
///   binding or a reordered `let` diverges; L3 pointwise over the four refusals
///   — no postcondition, two postconditions, a closure binding no name or two,
///   and a closure binding a pattern — each asserted as its exact message.
/// - witness: `tests::a_predicate_binds_captures_and_output`
/// - witness: `tests::an_underivable_list_is_refused`
fn predicate(
    entries: &Punctuated<SpecEntry, syn::Token![,]>
) -> Result<TokenStream, DerivationError>
{
    let mut bindings = TokenStream::new();
    let mut postcondition: Option<&syn::ExprClosure> = None;
    for entry in entries {
        match *entry {
            | SpecEntry::Capture {
                ref binder,
                ref value,
            } => bindings.extend(quote! { let #binder = #value; }),
            | SpecEntry::Ensures(ref closure) => {
                if postcondition.is_some() {
                    return Err(DerivationError::Rejected(
                        "one postcondition per attribute is derived".to_owned(),
                    ));
                }
                postcondition = Some(closure);
            },
        }
    }
    let closure = postcondition.ok_or_else(|| {
        DerivationError::Rejected("the attribute declares no postcondition".to_owned())
    })?;
    let mut inputs = closure.inputs.iter();
    let Some(first) = inputs.next()
    else {
        return Err(DerivationError::Rejected(
            "a postcondition closure binds exactly one plain output name".to_owned(),
        ));
    };
    if inputs.next().is_some() {
        return Err(DerivationError::Rejected(
            "a postcondition closure binds exactly one plain output name".to_owned(),
        ));
    }
    let syn::Pat::Ident(ref binder) = *first
    else {
        return Err(DerivationError::Rejected(
            "a postcondition closure binds a plain output name, not a pattern".to_owned(),
        ));
    };
    let output = &binder.ident;
    let body = &closure.body;
    Ok(quote! {
        {
            #bindings
            let #output = __anodized_output;
            #body
        }
    })
}

/// The witnesses the adapter's adequacy hypotheses name.
#[cfg(test)]
mod tests
{
    use std::path::Path;
    use std::process::Command;

    use proc_macro2::Span;
    use quote::ToTokens as _;
    use quote::quote;
    use syn::parse::Parser as _;
    use syn::punctuated::Punctuated;

    use super::Derivation;
    use super::DerivationError;
    use super::SpecEntry;
    use super::collect;
    use super::declarations;
    use super::declares_clause;
    use super::derive;
    use super::is_specification;
    use super::main;
    use super::predicate;
    use super::reason_site;
    use super::verdict;

    /// The environment marker selecting the child half of a `main` witness.
    const CHILD: &str = "VERUS_DERIVE_WITNESS_CHILD";

    /// The directory holding the stub verifiers the witnesses write.
    const STUBS: &str = "target/verus-stubs";

    /// The report a stub verifier accepting every unit produces.
    const REPORT: &str = concat!(
        "map: verified\n",
        "and_then: verified\n",
        "into_result: verified\n",
        "unavailable: verified\n",
        "delegate_ops: not reached — the specification is written in a ",
        "`macro_rules!` body, so every expansion carries it and no top-level ",
        "item does\n",
        "absent_reason: not reached — the block states a specification that no ",
        "specification attribute carries, so there is no declared predicate ",
        "to derive\n",
        "fmt: not reached — the specification attribute declares no predicate\n",
    );

    /// A formatter sink that records the attempted write and refuses it.
    #[repr(transparent)]
    struct Refusing
    {
        /// The text the formatter attempted to write.
        attempted: String,
    }

    impl core::fmt::Write for Refusing
    {
        /// Record the attempted text and refuse the write.
        ///
        /// # Specification
        /// - ensures: keeps the attempted text and refuses unconditionally, so
        ///   a `Display` implementation's own propagation is observable.
        /// - fails: always, with the formatter's own error.
        /// - panics: none.
        ///
        /// # Errors
        /// - `core::fmt::Error`: the sink refuses every write by construction.
        ///
        /// # Adequacy
        /// - hypothesis: L0 — the error type has one inhabitant, so the only
        ///   mutant is the success value, which the witness separates by the
        ///   propagated refusal it asserts.
        /// - witness: `tests::each_refusal_renders_its_half`
        fn write_str(
            &mut self,
            s: &str,
        ) -> core::fmt::Result
        {
            self.attempted.push_str(s);
            Err(core::fmt::Error)
        }
    }

    /// Each refusal renders its own half and carries its own report.
    #[test]
    fn each_refusal_renders_its_half()
    {
        let operational = DerivationError::Operational("no verifier".to_owned());
        let rejected = DerivationError::Rejected("an inline reason site".to_owned());
        let syntax = DerivationError::Syntax(syn::Error::new(Span::call_site(), "expected `,`"));
        assert_eq!(
            operational.to_string(),
            "operation failed: no verifier",
            "the operational half names itself and carries its report"
        );
        assert_eq!(
            rejected.to_string(),
            "unsupported source construct: an inline reason site",
            "the rejection half names itself and carries its report"
        );
        assert_eq!(
            syntax.to_string(),
            "expected `,`",
            "a parse failure carries the parser's own report unchanged"
        );
        let mut sink = Refusing {
            attempted: String::new(),
        };
        let refused = core::fmt::write(&mut sink, format_args!("{operational}"));
        assert_eq!(
            refused,
            Err(core::fmt::Error),
            "the formatter's own write failure is propagated"
        );
        assert_eq!(
            sink.attempted, "operation failed: ",
            "the refused write is the half's own prefix"
        );
    }

    /// A capture entry carries its binder and its expression.
    #[test]
    fn a_capture_entry_carries_its_binding()
    {
        let entry = syn::parse_str::<SpecEntry>("captures: original = count").unwrap();
        let relocated = match entry {
            | SpecEntry::Capture { binder, value } => {
                format!("{}|{}", binder, value.to_token_stream())
            },
            | SpecEntry::Ensures(closure) => format!("ensures|{}", closure.to_token_stream()),
        };
        assert_eq!(
            relocated, "original|count",
            "a capture entry relocates its binder and its expression"
        );
    }

    /// A postcondition entry carries its closure.
    #[test]
    fn a_postcondition_entry_carries_its_closure()
    {
        let entry = syn::parse_str::<SpecEntry>("ensures: |output| output == original").unwrap();
        let relocated = match entry {
            | SpecEntry::Capture { binder, .. } => format!("captures|{binder}"),
            | SpecEntry::Ensures(closure) => {
                format!(
                    "ensures|{}|{}",
                    closure.inputs.to_token_stream(),
                    closure.body.to_token_stream()
                )
            },
        };
        assert_eq!(
            relocated, "ensures|output|output == original",
            "a postcondition entry relocates its output name and its body"
        );
    }

    /// Any other entry label refuses the derivation at its own span.
    #[test]
    fn an_unknown_label_refuses_derivation()
    {
        let refusal = match syn::parse_str::<SpecEntry>("requires: count > 0") {
            | Ok(_) => "the label was accepted".to_owned(),
            | Err(error) => error.to_string(),
        };
        assert_eq!(
            refusal, "only `captures` and `ensures` entries are derived",
            "an unsupported label stops the derivation rather than being dropped"
        );
    }

    /// Exactly two attribute spellings are accepted.
    #[test]
    fn two_attribute_spellings_are_accepted()
    {
        for (attribute, accepted) in [
            ("#[spec]", true),
            ("#[quenchant::spec]", true),
            ("#[other::spec]", false),
            ("#[quenchant::spec::inner]", false),
            ("#[spec::inner]", false),
            ("#[anodized]", false),
            ("#[doc = \"a summary\"]", false),
        ] {
            let function =
                syn::parse_str::<syn::ItemFn>(&format!("{attribute} fn probe() {{}}")).unwrap();
            assert_eq!(
                is_specification(&function.attrs[0]).0,
                accepted,
                "{attribute} is classified by its whole path"
            );
        }
    }

    /// A specification bullet counts only under the specification heading.
    #[test]
    fn a_clause_counts_under_its_heading()
    {
        for (documentation, states) in [
            (
                "/// # Specification\n/// - requires: `count` is positive.\n",
                true,
            ),
            (
                "/// # Specification\n/// - ensures: returns the count.\n",
                true,
            ),
            ("/// # Specification\n/// trivial.\n", false),
            ("/// # Adequacy\n/// - ensures: returns the count.\n", false),
            (
                "/// # Specification\n/// # Errors\n/// - ensures: returns the count.\n",
                false,
            ),
            ("/// A summary alone.\n", false),
        ] {
            let function =
                syn::parse_str::<syn::ItemFn>(&format!("{documentation} fn probe() {{}}")).unwrap();
            assert_eq!(
                declares_clause(&function.attrs).0,
                states,
                "the heading a bullet stands under decides the answer: {documentation}"
            );
        }
    }

    /// Generated declarations expose usable types while keeping the reason
    /// vocabulary closed.
    #[test]
    fn relocated_nominals_preserve_type_and_seal_boundaries()
    {
        let file = syn::parse2::<syn::File>(quote! {
            #[derive(UnsupportedAttribute)]
            enum Ticket { One }
            #[derive(UnsupportedAttribute)]
            #[repr(transparent)]
            struct Wrapper(bool);
            reason_enum! {
                pub mod lookup {
                    #[derive(UnsupportedAttribute)]
                    pub enum Unavailable { Missing }
                }
            }
            nominal_type! { pub struct Ignored(bool); }
            fn ignored() { compile_error!("ordinary functions must not be relocated"); }
        })
        .unwrap();
        let declarations = declarations(&file).unwrap();
        let consumer = quote! {
            #declarations
            fn demand_reason<Reason: lookup::Reason>() {}
            pub fn exercise() -> bool {
                let _ticket = Ticket::One;
                let _reason = lookup::Unavailable::Missing;
                let Wrapper(flag) = Wrapper(true);
                demand_reason::<lookup::Unavailable>();
                flag
            }
        };
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "quenchant-relocation-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("consumer.rs");
        let mut compiler = Command::new("rustc");
        compiler
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "--edition=2024",
                "--crate-type=lib",
                "--emit=metadata",
                "-Dwarnings",
            ])
            .arg(&source)
            .arg("-o")
            .arg(directory.join("consumer.rmeta"));
        std::fs::write(&source, consumer.to_string()).unwrap();
        let admitted = compiler.output().unwrap();
        let escaped = quote! {
            #consumer
            pub struct Foreign;
            impl lookup::Reason for Foreign {}
        };
        std::fs::write(&source, escaped.to_string()).unwrap();
        let refused = compiler.output().unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        assert!(
            admitted.status.success(),
            "{}",
            String::from_utf8_lossy(&admitted.stderr)
        );
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("error[E0277]"));
    }

    /// A site that is not one module carrying one enum is refused.
    #[test]
    fn an_unsupported_site_is_refused()
    {
        for (site, refusal) in [
            (
                quote! { pub mod lookup; },
                "unsupported source construct: a reason site declares its enum inline",
            ),
            (
                quote! { pub mod lookup { pub struct NotAnEnum; } },
                "unsupported source construct: a reason site carries exactly one enum",
            ),
            (
                quote! { pub mod lookup { pub enum One { A } pub enum Two { B } } },
                "unsupported source construct: a reason site carries exactly one enum",
            ),
        ] {
            assert_eq!(
                reason_site(&site).unwrap_err().to_string(),
                refusal,
                "an unsupported site shape is reported rather than translated"
            );
        }
    }

    /// A derived predicate binds its captures and its output name.
    #[test]
    fn a_predicate_binds_captures_and_output()
    {
        let entries = Punctuated::<SpecEntry, syn::Token![,]>::parse_terminated
            .parse_str("captures: original = count, ensures: |output| output == original")
            .unwrap();
        let expected = quote! {
            {
                let original = count;
                let output = __anodized_output;
                output == original
            }
        };
        assert_eq!(
            predicate(&entries).unwrap().to_string(),
            expected.to_string(),
            "the capture, the output binding and the closure body are relocated in order"
        );
    }

    /// An argument list carrying no derivable postcondition is refused.
    #[test]
    fn an_underivable_list_is_refused()
    {
        for (list, refusal) in [
            (
                "captures: original = count",
                "unsupported source construct: the attribute declares no postcondition",
            ),
            (
                "ensures: |first| first, ensures: |second| second",
                "unsupported source construct: one postcondition per attribute is derived",
            ),
            (
                "ensures: || true",
                "unsupported source construct: a postcondition closure binds exactly one plain \
                 output name",
            ),
            (
                "ensures: |first, second| first",
                "unsupported source construct: a postcondition closure binds exactly one plain \
                 output name",
            ),
            (
                "ensures: |(first, second)| first",
                "unsupported source construct: a postcondition closure binds a plain output name, \
                 not a pattern",
            ),
        ] {
            let entries = Punctuated::<SpecEntry, syn::Token![,]>::parse_terminated
                .parse_str(list)
                .unwrap();
            assert_eq!(
                predicate(&entries).unwrap_err().to_string(),
                refusal,
                "an underivable argument list is reported rather than assembled: {list}"
            );
        }
    }

    /// A method's unit carries its impl header and the declarations.
    #[test]
    fn a_method_unit_carries_its_header()
    {
        let mut derivation = Derivation {
            targets: Vec::new(),
            unreached: Vec::new(),
        };
        let relocated = quote! { enum Ticket { One } };
        let container = quote! { impl Holder };
        let method = syn::parse2::<syn::ImplItemFn>(quote! {
            /// Provide the only ticket.
            ///
            /// # Specification
            /// - ensures: returns the only ticket.
            #[quenchant::spec(ensures: |output| matches!(output, Ticket::One))]
            fn ticket(&self) -> Ticket
            {
                Ticket::One
            }
        })
        .unwrap();
        collect(
            &mut derivation,
            &relocated,
            Some(&container),
            &method.attrs,
            &method.sig,
            &method.block,
        )
        .unwrap();
        assert!(
            derivation.unreached.is_empty(),
            "a method whose attribute declares a predicate is not named unreached"
        );
        let names: Vec<String> = derivation
            .targets
            .iter()
            .map(|target| target.name.to_string())
            .collect();
        assert_eq!(
            names,
            ["ticket"],
            "the verdict reports the method's own name"
        );
        let expected = quote! {
            use vstd::prelude::*;

            verus! {
                enum Ticket { One }
            }

            impl Holder
            {
                /// Provide the only ticket.
                ///
                /// # Specification
                /// - ensures: returns the only ticket.
                #[verus_spec(__anodized_output => ensures ({
                    let output = __anodized_output;
                    matches!(output, Ticket::One)
                }))]
                fn ticket(&self) -> Ticket
                {
                    Ticket::One
                }
            }

            fn main() {}
        };
        assert_eq!(
            derivation.targets[0].unit.to_string(),
            expected.to_string(),
            "the unit carries the declarations, the impl header and the derived attribute"
        );
    }

    /// A specification with no derivable predicate is named, never skipped.
    #[test]
    fn a_specification_without_a_predicate_is_named()
    {
        let relocated = quote! { enum Ticket { One } };
        for (function, record) in [
            (
                quote! {
                    /// A bare attribute.
                    #[quenchant::spec]
                    fn bare() {}
                },
                "bare: the specification attribute declares no predicate",
            ),
            (
                quote! {
                    /// An empty argument list.
                    #[spec()]
                    fn empty() {}
                },
                "empty: the specification attribute declares no predicate",
            ),
            (
                quote! {
                    /// A specification with no attribute.
                    ///
                    /// # Specification
                    /// - ensures: returns nothing.
                    fn stated() {}
                },
                "stated: the block states a specification that no specification attribute carries, so \
                 there is no declared predicate to derive",
            ),
        ] {
            let mut derivation = Derivation {
                targets: Vec::new(),
                unreached: Vec::new(),
            };
            let parsed = syn::parse2::<syn::ItemFn>(function).unwrap();
            collect(
                &mut derivation,
                &relocated,
                None,
                &parsed.attrs,
                &parsed.sig,
                &parsed.block,
            )
            .unwrap();
            assert!(
                derivation.targets.is_empty(),
                "a function with no derivable predicate reaches no verifier: {record}"
            );
            let named: Vec<String> = derivation
                .unreached
                .iter()
                .map(|construct| format!("{}: {}", construct.name, construct.reason))
                .collect();
            assert_eq!(
                named,
                [record],
                "the construct is named with the reason the derivation stops"
            );
        }
        let mut derivation = Derivation {
            targets: Vec::new(),
            unreached: Vec::new(),
        };
        let quiet = syn::parse2::<syn::ItemFn>(quote! {
            /// A summary alone.
            fn quiet() {}
        })
        .unwrap();
        collect(
            &mut derivation,
            &relocated,
            None,
            &quiet.attrs,
            &quiet.sig,
            &quiet.block,
        )
        .unwrap();
        assert!(
            derivation.targets.is_empty() && derivation.unreached.is_empty(),
            "a function stating no specification records nothing"
        );
    }

    /// The derivation names every population it reads off one file.
    #[test]
    fn derivation_names_every_population()
    {
        let file = syn::parse2::<syn::File>(quote! {
            /// A derivable function.
            ///
            /// # Specification
            /// - ensures: returns the count.
            #[quenchant::spec(ensures: |output| output == count)]
            fn derivable(count: usize) -> usize
            {
                count
            }

            /// A bare attribute.
            #[quenchant::spec]
            fn bare() {}

            /// A specification with no attribute.
            ///
            /// # Specification
            /// - requires: nothing.
            fn stated() {}

            /// A summary alone.
            fn quiet() {}

            impl Holder
            {
                /// Provide the only ticket.
                ///
                /// # Specification
                /// - ensures: returns the only ticket.
                #[spec(ensures: |output| matches!(output, Ticket::One))]
                fn ticket(&self) -> Ticket
                {
                    Ticket::One
                }
            }

            macro_rules! delegated
            {
                () => {
                    /// A generated method.
                    ///
                    /// # Specification
                    /// - ensures: returns nothing.
                    fn generated() {}
                };
            }
        })
        .unwrap();
        let derivation = derive(&file, &quote! { enum Ticket { One } }).unwrap();
        let targets: Vec<String> = derivation
            .targets
            .iter()
            .map(|target| target.name.to_string())
            .collect();
        assert_eq!(
            targets,
            ["derivable", "ticket"],
            "every function whose attribute declares a postcondition reaches the verifier"
        );
        let named: Vec<String> = derivation
            .unreached
            .iter()
            .map(|construct| format!("{}: {}", construct.name, construct.reason))
            .collect();
        assert_eq!(
            named,
            [
                "bare: the specification attribute declares no predicate",
                "stated: the block states a specification that no specification attribute carries, so \
                 there is no declared predicate to derive",
                "delegated: the specification is written in a `macro_rules!` body, so every expansion \
                 carries it and no top-level item does",
            ],
            "every specification the derivation does not carry is named with its reason"
        );
    }

    /// Every answer a stub verifier gives is reported verbatim.
    #[test]
    fn a_verifier_answer_is_reported_verbatim()
    {
        let directory = Path::new(STUBS);
        std::fs::create_dir_all(directory).unwrap();
        let unit = directory.join("verdict-unit.rs");
        let name = syn::Ident::new("probe", Span::call_site());
        for (stub, script, answer) in [
            ("accepting-verdict", "exit 0\n", "probe: verified\n"),
            (
                "refusing-verdict",
                "echo 'error: the stub refuses this unit' >&2\nexit 1\n",
                "probe: not reached — error: the stub refuses this unit\n",
            ),
            (
                "silent-verdict",
                "exit 1\n",
                "probe: not reached — the verifier reported no error line\n",
            ),
        ] {
            let verifier = directory.join(stub);
            std::fs::write(&verifier, format!("#!/bin/sh\n{script}")).unwrap();
            assert!(
                Command::new("chmod")
                    .arg("+x")
                    .arg(&verifier)
                    .status()
                    .unwrap()
                    .success(),
                "the stub verifier is executable"
            );
            assert_eq!(
                verdict(&verifier, &name, &unit).unwrap(),
                answer,
                "the verifier's own answer is reported, never this adapter's opinion of it"
            );
        }
    }

    /// A verifier that cannot run at all is an operational failure.
    #[test]
    fn an_unlaunchable_verifier_is_operational()
    {
        let name = syn::Ident::new("probe", Span::call_site());
        let refusal = verdict(
            &Path::new(STUBS).join("absent-verifier"),
            &name,
            &Path::new(STUBS).join("verdict-unit.rs"),
        )
        .unwrap_err();
        assert_eq!(
            refusal.to_string(),
            "operation failed: No such file or directory (os error 2)",
            "a verifier that cannot be launched carries the system's own report"
        );
    }

    /// A run with no commissioned verifier reports the absent binary.
    #[test]
    fn main_reports_an_absent_verifier()
    {
        if std::env::var_os(CHILD).is_some() {
            let refusal = main().unwrap_err();
            assert_eq!(
                refusal.to_string(),
                "operation failed: set VERUS_BIN to the commissioned verifier before deriving",
                "an absent commissioned verifier is the reported operational failure"
            );
            return;
        }
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::main_reports_an_absent_verifier")
            .env(CHILD, "1")
            .env_remove("VERUS_BIN")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "the child half asserts the absent-verifier report: {}",
            String::from_utf8_lossy(&child.stdout)
        );
    }

    /// A run reports one line per specification-bearing construct.
    #[test]
    fn main_reports_every_construct()
    {
        if std::env::var_os(CHILD).is_some() {
            main().unwrap();
            return;
        }
        let directory = Path::new(STUBS);
        std::fs::create_dir_all(directory).unwrap();
        let verifier = directory.join("accepting-report");
        std::fs::write(&verifier, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(
            Command::new("chmod")
                .arg("+x")
                .arg(&verifier)
                .status()
                .unwrap()
                .success(),
            "the stub verifier is executable"
        );
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("tests::main_reports_every_construct")
            .env(CHILD, "1")
            .env("VERUS_BIN", &verifier)
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success(),
            "the child half derives and checks every unit: {report}"
        );
        assert!(
            report.contains(REPORT),
            "the report names every specification-bearing construct once: {report}"
        );
    }
}
