//! Reading `- witness:` bullets out of a crate's rustdoc, and resolving them.
//!
//! The bullets are read from **parsed** declarations rather than from raw
//! source text: a `- witness:` line inside a doc code fence, a string literal
//! or an ordinary comment is not an obligation, and a text scan cannot tell
//! the difference.
//!
//! The `# Adequacy` section runs from its heading to the next heading of any
//! level, the same rule the dylint gate applies. A reader that instead consumed
//! to the end of the doc block would absorb whatever an attribute macro
//! appends after the author's prose.

use std::path::Path;
use std::path::PathBuf;

use quenchant_shape::shape::Maybe;

use crate::Finding;
use crate::GateError;
use crate::catalog::TestCatalog;
use crate::semantic::DeclarationOnly;
use crate::semantic::DocLine;
use crate::semantic::ErrorMessage;
use crate::semantic::FindingDetail;
use crate::semantic::LineNumber;
use crate::semantic::OpensHeading;
use crate::semantic::PackageName;
use crate::semantic::SourceText;
use crate::semantic::WitnessPath;

quenchant_shape::reason_enum! {
    /// Only a single quoted witness value can enter alias resolution.
    mod witness_syntax {
        /// Why this line supplies no exact witness path.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The line lacks the witness prefix and opening backtick.
            PrefixAbsent,
            /// The value does not end at a closing backtick.
            ClosingBacktickAbsent,
            /// Backticks enclose no path.
            EmptyPath,
            /// An interior backtick separates multiple values or trailing prose.
            InteriorBacktick,
        }
    }
}

/// The heading that opens the adequacy block.
const HEADING: &str = "# Adequacy";

/// One `- witness:` obligation, and where it was written.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WitnessClaim
{
    /// The file the bullet was written in.
    pub path: PathBuf,
    /// The one-based line of the bullet.
    pub line: LineNumber,
    /// The exact witness path, without its backticks.
    pub witness: String,
}

/// One rustdoc block, with the source line of every line it holds.
#[repr(transparent)]
#[derive(Clone, Debug, Default)]
struct DocBlock
{
    /// The block's lines, each with the source line it was written on.
    lines: Vec<(LineNumber, String)>,
}

/// Collect every witness obligation written in one Rust source file.
///
/// # Specification
/// - requires: `source` is the complete text of `path`, parseable as Rust.
/// - ensures: returns one claim per `- witness:` bullet inside an `# Adequacy`
///   section of a parsed declaration's rustdoc, in source order; a section
///   claiming the `- declaration-only:` exemption contributes none, since it
///   asserts there is no implementation to witness.
/// - provides: the obligations [`resolve`] checks against the test inventory.
/// - fails: [`GateError::Parse`] when the file is not parseable Rust.
/// - panics: none.
///
/// # Errors
/// [`GateError::Parse`] when `source` does not parse.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures separate a bullet in ordinary rustdoc, a
///   bullet inside a doc code fence, a bullet after the section's terminating
///   heading, and a declaration-only block, asserting the exact set of claims
///   each yields.
/// - witness: `gates::witnesses::a_bullet_outside_the_adequacy_section_is_not_an_obligation`
/// - witness: `gates::witnesses::a_declaration_only_block_carries_no_obligation`
/// - witness: `gates::witnesses::unparseable_source_is_an_operational_error`
#[inline]
pub fn witness_claims(
    path: &Path,
    source: SourceText<'_>,
) -> Result<Vec<WitnessClaim>, GateError>
{
    let file = syn::parse_file(source.0)
        .map_err(|error| GateError::parse(path, ErrorMessage::from(&error.to_string())))?;
    let mut claims = Vec::new();
    for block in doc_blocks(&file) {
        collect_block(path, &block, &mut claims);
    }
    claims.sort();
    Ok(claims)
}

/// Resolve every claim against the owning package's own test targets.
///
/// # Specification
/// - requires: `catalog` holds the inventory of the whole workspace, and
///   `package` owns every claim in `claims`.
/// - ensures: emits one finding per unresolved or ambiguous claim and nothing
///   for a claim that names exactly one test in one of `package`'s own targets;
///   a claim resolving only in another package is reported as unresolved and
///   names that package.
/// - provides: the G0 verdict for one crate.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures separate an exact hit, an absent path, a
///   path resolving in a sibling crate, a path naming the right test under the
///   wrong target, and a path exposed by two targets of one crate, asserting
///   the exact finding kind in each case.
/// - witness: `gates::witnesses::an_exact_in_crate_witness_resolves`
/// - witness: `gates::witnesses::an_absent_witness_is_unresolved`
/// - witness: `gates::witnesses::a_witness_owned_by_a_sibling_crate_names_the_sibling`
/// - witness: `gates::witnesses::a_wrong_target_witness_suggests_the_owning_target`
/// - witness: `gates::witnesses::a_witness_exposed_by_two_targets_is_ambiguous`
#[inline]
#[must_use]
pub fn resolve(
    package: PackageName<'_>,
    claims: &[WitnessClaim],
    catalog: &TestCatalog,
) -> Vec<Finding>
{
    let mut findings = Vec::new();
    for claim in claims {
        let witness = WitnessPath::from(&claim.witness);
        let Maybe::Present(targets) = catalog.targets(package, witness)
        else {
            let detail = unresolved_detail(package, witness, catalog);
            findings.push(Finding::new(
                "unresolved-witness".into(),
                package,
                &claim.path,
                claim.line,
                witness,
                FindingDetail::from(&detail),
            ));
            continue;
        };
        if targets.len() > 1_usize {
            let labels = targets.iter().cloned().collect::<Vec<_>>().join(", ");
            let detail = format!(
                "the path is exposed by more than one target of this crate ({labels}); name the \
                 one a reviewer should watch fail"
            );
            findings.push(Finding::new(
                "ambiguous-witness".into(),
                package,
                &claim.path,
                claim.line,
                witness,
                FindingDetail::from(&detail),
            ));
        }
    }
    findings
}

/// Explain an unresolved witness, naming a repair when the inventory offers
/// one.
///
/// # Specification
/// - requires: `catalog` holds no target of `package` exposing `witness`, which
///   is the condition [`resolve`] establishes before calling.
/// - ensures: prefers the near-miss repair over the foreign-package one, since
///   a path naming the right test under the wrong target is the commoner defect
///   and its repair is actionable; falls back to the bare absence message when
///   the inventory offers neither.
/// - provides: the prose half of an `unresolved-witness` finding.
/// - panics: none.
fn unresolved_detail(
    package: PackageName<'_>,
    witness: WitnessPath<'_>,
    catalog: &TestCatalog,
) -> String
{
    let near = catalog.near_misses(package, witness);
    if !near.is_empty() {
        return format!(
            "no target of this crate runs the path; the same test name is exposed as {}",
            near.join(", ")
        );
    }
    let foreign = catalog.foreign_packages(package, witness);
    if !foreign.is_empty() {
        return format!(
            "the path runs in {}, not in this crate; a witness must name a test in the item's own \
             crate",
            foreign.join(", ")
        );
    }
    String::from("no target of this crate runs the path; it is absent or renamed")
}

/// Add every witness obligation carried by one rustdoc block.
///
/// # Specification
/// - requires: `block` holds the lines of one parsed declaration's rustdoc,
///   each paired with the source line it was written on.
/// - ensures: appends one claim per `- witness:` bullet between the block's `#
///   Adequacy` heading and the next heading of any level, in source order; a
///   block with no such heading and a block claiming the `- declaration-only:`
///   exemption both append nothing, the latter because the exemption asserts
///   there is no implementation to witness.
/// - provides: the per-block half of [`witness_claims`].
/// - panics: none.
fn collect_block(
    path: &Path,
    block: &DocBlock,
    claims: &mut Vec<WitnessClaim>,
)
{
    let Some(start) = block
        .lines
        .iter()
        .position(|entry| entry.1.trim() == HEADING)
    else {
        return;
    };
    let mut collected = Vec::new();
    let mut declaration_only = DeclarationOnly(false);
    for &(line_number, ref text) in block.lines.iter().skip(start.saturating_add(1_usize)) {
        if opens_heading(DocLine::from(text)).0 {
            break;
        }
        if text.trim().starts_with("- declaration-only:") {
            declaration_only = DeclarationOnly(true);
            continue;
        }
        let Maybe::Present(witness) = exact_witness(DocLine::from(text))
        else {
            continue;
        };
        collected.push(WitnessClaim {
            path: path.to_path_buf(),
            line: line_number,
            witness: witness.0.to_owned(),
        });
    }
    if declaration_only.0 {
        return;
    }
    claims.append(&mut collected);
}

/// Return whether a rustdoc line opens a heading of any level.
///
/// # Specification
/// - requires: `line` is one rustdoc line.
/// - ensures: answers affirmatively for a leading run of one or more `#`
///   followed by a space, and negatively for a bare `#` run or for a `#` that
///   appears after other text.
/// - provides: the terminator of the adequacy section, matching the dylint
///   gate's own reader so the two agree on where the block ends.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate `# `, `## `, a bare `#`, and a `#`
///   after other text.
/// - witness: `gates::witnesses::any_heading_level_terminates_the_section`
#[inline]
#[must_use]
pub fn opens_heading(line: DocLine<'_>) -> OpensHeading
{
    let trimmed = line.0.trim();
    let rest = trimmed.trim_start_matches('#');
    OpensHeading(rest.len() < trimmed.len() && rest.starts_with(' '))
}

/// Extract the exact path named by a `- witness:` bullet.
///
/// # Specification
/// - requires: `line` is one rustdoc line.
/// - ensures: returns the path exactly when the line is a `- witness:` bullet
///   whose value is one nonempty backtick-delimited path; a bullet spelled any
///   other way yields nothing here and is denied by the dylint gate, which owns
///   the grammar.
/// - provides: the exact alias resolution compares against.
/// - provides: `witness_syntax::Missing` distinguishes `PrefixAbsent`,
///   `ClosingBacktickAbsent`, `EmptyPath`, and `InteriorBacktick`; these
///   nonmatches are left to the compiler-side grammar diagnostic.
/// - panics: none.
fn exact_witness(line: DocLine<'_>) -> Maybe<WitnessPath<'_>, witness_syntax::Missing>
{
    let Some(rest) = line.0.trim().strip_prefix("- witness: `")
    else {
        return Maybe::Absent(witness_syntax::Missing::PrefixAbsent);
    };
    let Some(witness) = rest.strip_suffix('`')
    else {
        return Maybe::Absent(witness_syntax::Missing::ClosingBacktickAbsent);
    };
    if witness.is_empty() {
        return Maybe::Absent(witness_syntax::Missing::EmptyPath);
    }
    if witness.contains('`') {
        return Maybe::Absent(witness_syntax::Missing::InteriorBacktick);
    }
    Maybe::Present(WitnessPath::from(witness))
}

/// Every rustdoc block in a parsed file, in an unspecified order.
///
/// # Specification
/// - requires: `file` is a parsed Rust source file.
/// - ensures: yields one block per documented declaration, including the file's
///   own inner documentation, items nested in modules, impl items, trait items
///   and foreign items.
/// - provides: the doc groups the adequacy reader walks.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the walk is a loop over an explicit worklist.
/// - measure: the number of syntax nodes still on the worklist.
/// - boundedness: every pop pushes only strict children of the popped node, and
///   a parsed file is a finite tree.
/// - input recursion: none.
fn doc_blocks(file: &syn::File) -> Vec<DocBlock>
{
    let mut blocks = Vec::new();
    push_block(&mut blocks, &file.attrs);
    let mut pending: Vec<&syn::Item> = file.items.iter().collect();
    while let Some(item) = pending.pop() {
        push_block(&mut blocks, item_attrs(item));
        match *item {
            | syn::Item::Mod(ref module) => {
                if let Some((_, ref items)) = module.content {
                    pending.extend(items.iter());
                }
            },
            | syn::Item::Impl(ref block) => {
                for impl_item in &block.items {
                    push_block(&mut blocks, impl_item_attrs(impl_item));
                }
            },
            | syn::Item::Trait(ref declaration) => {
                for trait_item in &declaration.items {
                    push_block(&mut blocks, trait_item_attrs(trait_item));
                }
            },
            | syn::Item::ForeignMod(ref block) => {
                for foreign_item in &block.items {
                    push_block(&mut blocks, foreign_item_attrs(foreign_item));
                }
            },
            | syn::Item::Struct(ref declaration) => {
                for field in &declaration.fields {
                    push_block(&mut blocks, &field.attrs);
                }
            },
            | syn::Item::Enum(ref declaration) => {
                for variant in &declaration.variants {
                    push_block(&mut blocks, &variant.attrs);
                }
            },
            | _ => {},
        }
    }
    blocks
}

/// Add the documentation block carried by one attribute list, when nonempty.
///
/// # Specification
/// - requires: `attrs` is one declaration's complete attribute list.
/// - ensures: concatenates the lines of every `#[doc = "…"]` attribute in
///   declaration order, pairing each with the source line it sits on, and
///   pushes the result only when it holds at least one line; a non-`doc`
///   attribute and a `doc` attribute whose value is not a string literal are
///   both skipped, so a macro-generated `#[doc(hidden)]` contributes nothing.
/// - provides: one entry of [`doc_blocks`]'s result.
/// - panics: none.
fn push_block(
    blocks: &mut Vec<DocBlock>,
    attrs: &[syn::Attribute],
)
{
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(ref name_value) = attr.meta
        else {
            continue;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(ref literal),
            ..
        }) = name_value.value
        else {
            continue;
        };
        let start = attr_start_line(attr);
        for (offset, line) in literal.value().lines().enumerate() {
            lines.push((
                LineNumber(start.0.saturating_add(offset)),
                String::from(line),
            ));
        }
    }
    if lines.is_empty() {
        return;
    }
    blocks.push(DocBlock { lines });
}

/// The one-based source line an attribute's first token sits on.
///
/// # Specification
/// - ensures: returns the line the attribute's span starts on, one-based as the
///   parser reports it.
/// - panics: none.
fn attr_start_line(attr: &syn::Attribute) -> LineNumber
{
    LineNumber(syn::spanned::Spanned::span(attr).start().line)
}

/// Return the attributes of an item.
///
/// # Specification
/// - ensures: returns the item's own attribute slice for every item kind that
///   carries one, and an empty slice for a kind the parser has added since.
/// - panics: none.
fn item_attrs(item: &syn::Item) -> &[syn::Attribute]
{
    match *item {
        | syn::Item::Const(ref inner) => &inner.attrs,
        | syn::Item::Enum(ref inner) => &inner.attrs,
        | syn::Item::ExternCrate(ref inner) => &inner.attrs,
        | syn::Item::Fn(ref inner) => &inner.attrs,
        | syn::Item::ForeignMod(ref inner) => &inner.attrs,
        | syn::Item::Impl(ref inner) => &inner.attrs,
        | syn::Item::Macro(ref inner) => &inner.attrs,
        | syn::Item::Mod(ref inner) => &inner.attrs,
        | syn::Item::Static(ref inner) => &inner.attrs,
        | syn::Item::Struct(ref inner) => &inner.attrs,
        | syn::Item::Trait(ref inner) => &inner.attrs,
        | syn::Item::TraitAlias(ref inner) => &inner.attrs,
        | syn::Item::Type(ref inner) => &inner.attrs,
        | syn::Item::Union(ref inner) => &inner.attrs,
        | syn::Item::Use(ref inner) => &inner.attrs,
        | _ => &[],
    }
}

/// Return the attributes of an impl item.
///
/// # Specification
/// - ensures: returns the impl item's own attribute slice for every kind that
///   carries one, and an empty slice for a kind the parser has added since.
/// - panics: none.
fn impl_item_attrs(item: &syn::ImplItem) -> &[syn::Attribute]
{
    match *item {
        | syn::ImplItem::Const(ref inner) => &inner.attrs,
        | syn::ImplItem::Fn(ref inner) => &inner.attrs,
        | syn::ImplItem::Macro(ref inner) => &inner.attrs,
        | syn::ImplItem::Type(ref inner) => &inner.attrs,
        | _ => &[],
    }
}

/// Return the attributes of a trait item.
///
/// # Specification
/// - ensures: returns the trait item's own attribute slice for every kind that
///   carries one, and an empty slice for a kind the parser has added since.
/// - panics: none.
fn trait_item_attrs(item: &syn::TraitItem) -> &[syn::Attribute]
{
    match *item {
        | syn::TraitItem::Const(ref inner) => &inner.attrs,
        | syn::TraitItem::Fn(ref inner) => &inner.attrs,
        | syn::TraitItem::Macro(ref inner) => &inner.attrs,
        | syn::TraitItem::Type(ref inner) => &inner.attrs,
        | _ => &[],
    }
}

/// Return the attributes of a foreign item.
///
/// # Specification
/// - ensures: returns the foreign item's own attribute slice for every kind
///   that carries one, and an empty slice for a kind the parser has added
///   since.
/// - panics: none.
fn foreign_item_attrs(item: &syn::ForeignItem) -> &[syn::Attribute]
{
    match *item {
        | syn::ForeignItem::Fn(ref inner) => &inner.attrs,
        | syn::ForeignItem::Macro(ref inner) => &inner.attrs,
        | syn::ForeignItem::Static(ref inner) => &inner.attrs,
        | syn::ForeignItem::Type(ref inner) => &inner.attrs,
        | _ => &[],
    }
}
