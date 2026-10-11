//! Executable predicates beside a clause-bearing specification block.
//!
//! A `# Specification` block that states clauses carries obligations a reader
//! checks by hand; `#[spec(...)]` restates the checkable ones as predicates an
//! enforcing build evaluates on every call. This gate requires the attribute,
//! with at least one executable clause, or an exemption clause in the block
//! stating why no runtime predicate expresses the obligation.
//!
//! The attribute does not survive expansion: the facade's disabled route
//! erases it and the enabled route rewrites the item, so stripped HIR is never
//! read for clauses. A pre-expansion pass reads each authored declaration's
//! attributes, nested trait and implementation markers included, and records
//! what its executable clauses state under the declaration's name span. The
//! late specification pass owns the authorship, test and derive boundaries and
//! looks that record up for the items it decides.
//!
//! Predicates are read as tokens. A literal `true`, an expression compared with
//! itself, a pattern every value of its subject's type matches and a predicate
//! repeated under the same clause are refused, because each holds on every
//! call. Whether a substantive predicate is the right one, or holds, is not
//! decided here.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_help;
use quenchant_shape::shape::Maybe;
use rustc_ast::AssocItem;
use rustc_ast::AssocItemKind;
use rustc_ast::AttrArgs;
use rustc_ast::AttrItemKind;
use rustc_ast::AttrKind;
use rustc_ast::Attribute;
use rustc_ast::ForeignItemKind;
use rustc_ast::Item;
use rustc_ast::ItemKind;
use rustc_ast::token::Delimiter;
use rustc_ast::token::Token;
use rustc_ast::token::TokenKind;
use rustc_ast::tokenstream::TokenStream;
use rustc_ast::tokenstream::TokenTree;
use rustc_lint::EarlyContext;
use rustc_lint::EarlyLintPass;
use rustc_lint::LateContext;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::BytePos;
use rustc_span::Span;
use rustc_span::Symbol;
use rustc_span::symbol::kw;

use crate::semantic::DiagnosticText;
use crate::semantic::RustdocLine;
use crate::semantic::SameTokens;

quenchant_shape::reason_enum! {
    /// A token run can state something beyond its subject's type.
    mod predicate_reading {
        /// Why a predicate is not refused as vacuous.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Substantive {
            /// None of the vacuous forms matches the predicate's tokens.
            NoVacuousForm,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A clause-bearing block can satisfy the executable obligation.
    mod executable_check {
        /// Why no diagnostic is needed.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Satisfied {
            /// The attribute states a substantive executable predicate.
            PredicateStated,
            /// The block states a reasoned exemption and no predicate contradicts it.
            Exempt,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A token can fail to name a specification clause.
    mod clause_reading {
        /// Why the token opens no clause.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAKey {
            /// The token is not an identifier followed by a colon.
            NotKeyShaped,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A token tree can be something other than an identifier.
    mod token_reading {
        /// Why no identifier is read.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotIdent {
            /// The tree is another token or a delimited group.
            OtherTree,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A pattern arm can admit fewer values than its type.
    mod pattern_reading {
        /// Why the arm is not a wildcard over one variant.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotWildcard {
            /// The arm is not a path, or its payload constrains the value.
            Refutable,
        }
    }
}

quenchant_shape::reason_enum! {
    /// An operand can be something other than a nullary method call.
    mod query_reading {
        /// Why the operand is not a discriminant query.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAQuery {
            /// The operand does not end in `.name()` after a receiver.
            OtherShape,
        }
    }
}

declare_lint! {
    /// ### What it does
    ///
    /// An authored function or method whose `# Specification` block carries clauses must carry `#[spec(...)]` with at least one executable `requires:`, `maintains:` or `ensures:` predicate, or state in the block why no runtime predicate expresses its obligation.
    ///
    /// The attribute is matched by the last segment of its path, so `#[spec]` and `#[anodized::spec]` both count, written directly, under `cfg_attr`, or as a nested marker inside a specified trait or implementation. A block whose whole body is `trivial.` owes nothing.
    ///
    /// ### The exemption
    ///
    /// The block's last clause before `- intension:` reads `- executable: none — <reason>`, the reason nonempty. It is refused when it is malformed, repeated, followed by another clause, or written beside an executable predicate it contradicts.
    ///
    /// ### Why is this bad?
    ///
    /// Prose clauses are read by hand and checked by nobody. An executable predicate is checked on every call of an enforcing build, and a stated exemption makes the decision not to check reviewable. A block with neither leaves that decision unrecorded.
    ///
    /// ### What it does not decide
    ///
    /// The predicates are read as tokens. It refuses a predicate that is the literal `true`, an expression compared with itself, a type check every value of its subject's type passes (`matches!(x, _)`, the two arms of `Result`, `Option` or `Maybe`, `is_ok() || is_err()`, `is_some() || is_none()`), and one repeated under the same clause. A predicate that restates a type invariant in other words, a predicate weaker than its prose clause, and whether any predicate holds are review's to judge. Whether the exemption's reason is true is review's too.
    ///
    /// Items a crate-local `macro_rules!` expands are read as carrying no attribute, because the pre-expansion pass sees the macro's tokens and not its items; such an item states the exemption in the template.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default. A crate opts in at its root with `#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_present))]` once its items satisfy it.
    ///
    /// ### Example
    ///
    /// ```rust
    /// /// # Specification
    /// /// - requires: `count` is positive.
    /// /// - panics: none.
    /// fn halve(count: Count) -> Count { count.halved() }
    /// ```
    ///
    /// An executable predicate, and an exemption:
    ///
    /// ```rust
    /// /// # Specification
    /// /// - requires: `count` is positive.
    /// /// - panics: none.
    /// #[spec(requires: count > Count::ZERO)]
    /// fn halve(count: Count) -> Count { count.halved() }
    ///
    /// /// # Specification
    /// /// - ensures: the arena is unchanged.
    /// /// - panics: none.
    /// /// - executable: none — the arena has no equality to compare entry and exit.
    /// fn inspect(arena: &Arena) -> Report { arena.report() }
    /// ```
    pub SPEC_ATTRIBUTE_PRESENT,
    Allow,
    "a clause-bearing # Specification block needs #[spec] with an executable predicate or a reasoned exemption"
}

impl_lint_pass!(AttributeCollector => [SPEC_ATTRIBUTE_PRESENT]);

declare_lint! {
    /// ### What it does
    ///
    /// Requires imported, single-segment spellings of the resolved anodized
    /// specification attributes, including the facade's `spec_helper`.
    ///
    /// ### Why is this bad?
    ///
    /// Multiple spellings obscure the shared specification surface. An import
    /// makes the dependency explicit for both `#[spec]` and `#[spec_helper]`.
    ///
    /// ### Limitations
    ///
    /// Only expanded, authored attributes are checked. Inactive configurations,
    /// macro-generated attributes, and nested markers consumed without their
    /// own macro expansion are not resolved invocations. Renaming a bare import
    /// is outside this lint's scope. Unrelated macros named `spec` are ignored.
    ///
    /// ### Activation
    ///
    /// Allow by default; select `spec_attribute_unqualified` at the crate root
    /// under `cfg_attr(dylint_lib = "quenchant_dylints", deny(...))` or with
    /// `-D spec_attribute_unqualified`.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// use anodized::spec;
    /// #[spec(requires: count > Count::ZERO)]
    /// fn consume(count: Count) { /* ... */ }
    /// ```
    pub SPEC_ATTRIBUTE_UNQUALIFIED,
    Allow,
    "specification attributes use an imported, single-segment path"
}

/// Authored qualified paths shared between collection and resolved expansion
/// checking. Removing a diagnosed invocation avoids duplicate reports for the
/// several declarations one attribute may produce.
#[derive(Clone, Default)]
#[repr(transparent)]
pub struct SpecificationSpelling(Arc<Mutex<BTreeMap<NameSpan, Span>>>);

impl_lint_pass!(SpecificationSpelling => [SPEC_ATTRIBUTE_UNQUALIFIED]);

impl EarlyLintPass for SpecificationSpelling
{
    /// Record authored path qualification before expansion consumes attributes.
    ///
    /// # Specification
    /// - ensures: stores multi-segment attribute paths and qualified paths in
    ///   conditional attributes; generated attributes are not recorded.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn check_attribute(
        &mut self,
        _cx: &EarlyContext<'_>,
        attribute: &Attribute,
    )
    {
        if attribute.span.from_expansion() {
            return;
        }
        let AttrKind::Normal(ref normal) = attribute.kind
        else {
            return;
        };
        if normal.item.path.segments.len() > 1_usize {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(NameSpan::from(attribute.span), normal.item.path.span);
        }
        if normal.item.path.segments.len() == 1_usize
            && normal
                .item
                .path
                .segments
                .first()
                .is_some_and(|segment| segment.ident.name == rustc_span::sym::cfg_attr)
            && let AttrItemKind::Unparsed(ref args) = normal.item.args
            && let AttrArgs::Delimited(ref delimited) = *args
        {
            self.collect_conditional(&delimited.tokens);
        }
    }
}

impl SpecificationSpelling
{
    /// Collect qualified paths nested in authored conditional attributes.
    ///
    /// # Specification
    /// - ensures: records each qualified applied path, including nested
    ///   `cfg_attr` lists, without treating paths in predicates or arguments as
    ///   attribute paths. Resolution later excludes inactive conditions.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the matrix distinguishes active direct and nested
    ///   conditions from an inactive condition and imported attributes.
    /// - witness: `tests::ui_spec_gates`
    fn collect_conditional(
        &self,
        arguments: &TokenStream,
    )
    {
        let mut pending = vec![arguments];
        while let Some(arguments) = pending.pop() {
            let trees: Vec<&TokenTree> = arguments.iter().collect();
            for attribute in split_commas(&trees).into_iter().skip(1_usize) {
                let path = match *attribute.as_slice() {
                    | [
                        ref path @ ..,
                        &TokenTree::Delimited(_, _, Delimiter::Parenthesis, ref tokens),
                    ] => {
                        if matches!(path, [tree] if matches!(ident_name(tree), Maybe::Present(name) if name == rustc_span::sym::cfg_attr))
                        {
                            pending.push(tokens);
                        }
                        path
                    },
                    | ref path => path,
                };
                if path.iter().any(|tree| {
                    matches!(
                        tree,
                        TokenTree::Token(
                            Token {
                                kind: TokenKind::PathSep,
                                ..
                            },
                            _
                        )
                    )
                }) {
                    self.0
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(NameSpan::from(trees_span(&attribute)), trees_span(path));
                }
            }
        }
    }

    /// Join a resolved specification expansion to its authored qualified path.
    ///
    /// # Specification
    /// - ensures: each recorded qualified invocation of the anodized macro or
    ///   quenchant facade is diagnosed once; unrelated macro identities and
    ///   unqualified paths are not diagnosed.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI matrix separates direct, absolute, aliased and
    ///   re-exported paths from bare imports and an unrelated macro named spec.
    /// - witness: `tests::ui_spec_gates`
    fn check_definition(
        &self,
        cx: &LateContext<'_>,
        definition: rustc_hir::def_id::LocalDefId,
    )
    {
        let mut expansion = cx.tcx.expn_that_defined(definition);
        while expansion != rustc_span::hygiene::ExpnId::root() {
            let data = expansion.expn_data();
            expansion = data.parent;
            let Some(macro_id) = data.macro_def_id
            else {
                continue;
            };
            if !cx.tcx.parent(macro_id).is_crate_root() {
                continue;
            }
            let Some(name) = cx.tcx.opt_item_name(macro_id)
            else {
                continue;
            };
            let (message, help) = match (cx.tcx.crate_name(macro_id.krate).as_str(), name.as_str())
            {
                | ("anodized_macros" | "quenchant_spec_macros", "spec") => (
                    "write #[spec(...)] with use anodized::spec; instead of a path-qualified specification attribute",
                    "import the specification macro in this scope, then use #[spec(...)]",
                ),
                | ("quenchant_spec_macros", "spec_helper" | "__retain_helper") => (
                    "write #[spec_helper] with use anodized::spec_helper; instead of a path-qualified specification attribute",
                    "import the specification helper macro in this scope, then use #[spec_helper]",
                ),
                | _ => continue,
            };
            let path = self
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&NameSpan::from(data.call_site));
            if let Some(path) = path {
                span_lint_and_help(cx, SPEC_ATTRIBUTE_UNQUALIFIED, path, message, None, help);
            }
        }
    }
}

impl<'tcx> rustc_lint::LateLintPass<'tcx> for SpecificationSpelling
{
    /// Check resolved specification spelling on a free item.
    ///
    /// # Specification
    /// - ensures: applies [`Self::check_definition`] in this item's lint scope.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures separate qualified specification
    ///   attributes from imported attributes in each declaration scope.
    /// - witness: `tests::ui_spec_gates`
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx rustc_hir::Item<'tcx>,
    )
    {
        self.check_definition(cx, item.owner_id.def_id);
    }

    /// Check resolved specification spelling on a trait member.
    ///
    /// # Specification
    /// - ensures: applies [`Self::check_definition`] in this member's lint
    ///   scope.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures separate qualified specification
    ///   attributes from imported attributes in each declaration scope.
    /// - witness: `tests::ui_spec_gates`
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx rustc_hir::TraitItem<'tcx>,
    )
    {
        self.check_definition(cx, item.owner_id.def_id);
    }

    /// Check resolved specification spelling on an implementation member.
    ///
    /// # Specification
    /// - ensures: applies [`Self::check_definition`] in this member's lint
    ///   scope.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures separate qualified specification
    ///   attributes from imported attributes in each declaration scope.
    /// - witness: `tests::ui_spec_gates`
    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx rustc_hir::ImplItem<'tcx>,
    )
    {
        self.check_definition(cx, item.owner_id.def_id);
    }
}

/// A declaration's authored name, located the same way before and after
/// expansion.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NameSpan
{
    /// The first byte of the name.
    lo: BytePos,
    /// One past the last byte of the name.
    hi: BytePos,
}

impl From<Span> for NameSpan
{
    /// The span's byte range is the key; its syntax context is not.
    ///
    /// # Specification
    /// trivial.
    fn from(span: Span) -> Self
    {
        Self {
            lo: span.lo(),
            hi: span.hi(),
        }
    }
}

/// Why a predicate holds on every call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Vacuity
{
    /// The predicate is the literal `true` or `!false`.
    LiteralTrue,
    /// The predicate compares one expression with itself.
    Reflexive,
    /// The predicate is a type check every value of its subject passes.
    TypeCheck,
    /// The predicate repeats an earlier one under the same clause.
    Repeated,
}

/// What the authored `spec` attributes on one declaration state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableAttribute
{
    /// No `spec` attribute is written on the declaration.
    Absent,
    /// A `spec` attribute states no `requires`, `maintains` or `ensures`
    /// clause.
    ClauseAbsent,
    /// Every executable predicate is substantive.
    Stated,
    /// The first vacuous predicate, at its authored span.
    Vacuous(Vacuity, Span),
}

/// Authored attribute records, written by the pre-expansion pass and read by
/// the late pass of the same compilation.
#[derive(Clone, Default)]
#[repr(transparent)]
pub struct AttributeIndex(Arc<Mutex<BTreeMap<NameSpan, ExecutableAttribute>>>);

impl AttributeIndex
{
    /// Record what a declaration's authored attributes state.
    ///
    /// # Specification
    /// - ensures: until another record replaces it, a later [`Self::lookup`] at
    ///   the same name span returns `attribute`; a poisoned lock is recovered,
    ///   since every write leaves the map whole.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn record(
        &self,
        name: Span,
        attribute: ExecutableAttribute,
    )
    {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(NameSpan::from(name), attribute);
    }

    /// Read what a declaration's authored attributes state.
    ///
    /// # Specification
    /// - requires: `name` is the declaration's authored name span.
    /// - ensures: returns the record the pre-expansion pass made at that span,
    ///   and [`ExecutableAttribute::Absent`] where it made none.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    #[must_use]
    pub fn lookup(
        &self,
        name: Span,
    ) -> ExecutableAttribute
    {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&NameSpan::from(name))
            .copied()
            .unwrap_or(ExecutableAttribute::Absent)
    }
}

/// Pre-expansion reader of the authored `spec` attributes.
#[repr(transparent)]
pub struct AttributeCollector
{
    /// Where the records are shared with the late specification pass.
    index: AttributeIndex,
}

impl AttributeCollector
{
    /// The collector writes into the index its late reader holds.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(index: AttributeIndex) -> Self
    {
        Self { index }
    }

    /// Record a declaration whose attributes carry a `spec` attribute.
    ///
    /// # Specification
    /// - ensures: records [`read_attributes`]'s answer under `name` unless the
    ///   answer is [`ExecutableAttribute::Absent`], which a lookup returns
    ///   anyway.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn collect(
        &self,
        name: Span,
        attrs: &[Attribute],
    )
    {
        let attribute = read_attributes(attrs);
        if attribute != ExecutableAttribute::Absent {
            self.index.record(name, attribute);
        }
    }
}

impl EarlyLintPass for AttributeCollector
{
    /// A free function's attributes, and each foreign function's in a foreign
    /// block, are read at their names.
    ///
    /// # Specification
    /// - ensures: collects a function item and every function of a foreign
    ///   block, and ignores every other item kind; the early pass has no hook
    ///   of its own for foreign items.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn check_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &Item,
    )
    {
        match item.kind {
            | ItemKind::Fn(ref function) => self.collect(function.ident.span, &item.attrs),
            | ItemKind::ForeignMod(ref foreign) => {
                for foreign_item in &foreign.items {
                    if let ForeignItemKind::Fn(ref function) = foreign_item.kind {
                        self.collect(function.ident.span, &foreign_item.attrs);
                    }
                }
            },
            | _ => {},
        }
    }

    /// A trait method's attributes, nested markers included, are read at its
    /// name.
    ///
    /// # Specification
    /// - ensures: collects a method and ignores every other associated item
    ///   kind.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn check_trait_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        if let AssocItemKind::Fn(ref function) = item.kind {
            self.collect(function.ident.span, &item.attrs);
        }
    }

    /// An implementation method's attributes are read at its name.
    ///
    /// # Specification
    /// - ensures: collects a method and ignores every other associated item
    ///   kind.
    /// - panics: none.
    /// - executable: none — the shared compiler-pass index can change before a
    ///   postcondition reacquires its lock.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the UI fixtures exercise authored attribute
    ///   collection and its late-pass lookup through the resulting diagnostics.
    /// - witness: `tests::ui_spec_gates`
    fn check_impl_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        if let AssocItemKind::Fn(ref function) = item.kind {
            self.collect(function.ident.span, &item.attrs);
        }
    }
}

/// The argument list of one authored `spec` attribute.
enum SpecArguments<'attr>
{
    /// The attribute is written without arguments.
    Bare,
    /// The tokens between the attribute's parentheses.
    Delimited(&'attr TokenStream),
}

/// One `key: value` clause of a `spec` attribute's argument list.
struct Clause<'stream>
{
    /// The clause's key.
    key: ClauseKey,
    /// The value's top-level token trees, without the separating comma.
    value: Vec<&'stream TokenTree>,
}

/// The keys the specification backend reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClauseKey
{
    /// A precondition, checked on entry.
    Requires,
    /// An invariant, checked on entry and on exit.
    Maintains,
    /// A postcondition, checked on exit.
    Ensures,
    /// A key stating no predicate of its own, such as `captures` or `binds`.
    Other,
}

/// One executable predicate and the clause it belongs to.
struct Predicate<'stream>
{
    /// The clause the predicate is written under.
    key: ClauseKey,
    /// The predicate's tokens, closure head and enclosing groups removed.
    trees: Vec<&'stream TokenTree>,
}

/// Whether a token tree is one token of the given kinds.
macro_rules! token_is {
    ($tree:expr, $($kind:pat_param)|+) => {
        matches!(*$tree, TokenTree::Token(Token { kind: $($kind)|+, .. }, _))
    };
}

/// Read what a declaration's authored `spec` attributes state.
///
/// # Specification
/// - requires: `attrs` are one declaration's outer attributes, read before
///   expansion.
/// - ensures: returns [`ExecutableAttribute::Absent`] exactly when no attribute
///   supplies specification arguments through [`spec_arguments`].
/// - ensures: returns [`ExecutableAttribute::ClauseAbsent`] when the attributes
///   state no `requires`, `maintains` or `ensures` predicate;
///   [`ExecutableAttribute::Vacuous`] at the first predicate, in source order,
///   that [`vacuity`] refuses or that repeats an earlier predicate of the same
///   clause; and [`ExecutableAttribute::Stated`] otherwise.
/// - provides: the record [`SPEC_ATTRIBUTE_PRESENT`] decides with.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates a function with no attribute, a
///   bare attribute, an attribute stating only captures, each executable key, a
///   qualified path, a `cfg_attr` route, a nested trait marker, and each
///   vacuous form one at a time.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| (output == ExecutableAttribute::Absent)
    == attrs.iter().all(|attr| spec_arguments(attr).is_empty()))]
fn read_attributes(attrs: &[Attribute]) -> ExecutableAttribute
{
    let mut found = false;
    let mut predicates: Vec<Predicate<'_>> = Vec::new();
    for attr in attrs {
        for arguments in spec_arguments(attr) {
            found = true;
            let SpecArguments::Delimited(tokens) = arguments
            else {
                continue;
            };
            for clause in clauses(tokens) {
                predicates.extend(clause_predicates(&clause));
            }
        }
    }
    if !found {
        return ExecutableAttribute::Absent;
    }
    if predicates.is_empty() {
        return ExecutableAttribute::ClauseAbsent;
    }
    for (position, predicate) in predicates.iter().enumerate() {
        if let Maybe::Present(kind) = vacuity(&predicate.trees) {
            return ExecutableAttribute::Vacuous(kind, trees_span(&predicate.trees));
        }
        let earlier = predicates.get(.. position).unwrap_or_default();
        if earlier.iter().any(|other| {
            other.key == predicate.key && same_tokens(&other.trees, &predicate.trees).0
        }) {
            return ExecutableAttribute::Vacuous(Vacuity::Repeated, trees_span(&predicate.trees));
        }
    }
    ExecutableAttribute::Stated
}

/// Find the `spec` attributes an authored attribute writes.
///
/// # Specification
/// - ensures: a documentation attribute supplies no specification arguments.
/// - ensures: returns the attribute's own arguments when its path's last
///   segment is `spec`; for a `cfg_attr`, the arguments of each attribute it
///   applies whose path ends in `spec`; and nothing for any other attribute.
///   The condition of a `cfg_attr` is not evaluated: the authored predicate is
///   an obligation whichever configuration checks it.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise bare, delimited, qualified
///   and conditional attributes and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| match attr.kind {
    AttrKind::DocComment(..) => output.is_empty(),
    AttrKind::Normal(_) => true,
})]
fn spec_arguments(attr: &Attribute) -> Vec<SpecArguments<'_>>
{
    let AttrKind::Normal(ref normal) = attr.kind
    else {
        return Vec::new();
    };
    let Some(segment) = normal.item.path.segments.last()
    else {
        return Vec::new();
    };
    let AttrItemKind::Unparsed(ref args) = normal.item.args
    else {
        return Vec::new();
    };
    if segment.ident.name.as_str() == "spec" {
        return match *args {
            | AttrArgs::Delimited(ref delimited) => {
                vec![SpecArguments::Delimited(&delimited.tokens)]
            },
            | AttrArgs::Empty | AttrArgs::Eq { .. } => vec![SpecArguments::Bare],
        };
    }
    let AttrArgs::Delimited(ref delimited) = *args
    else {
        return Vec::new();
    };
    if normal.item.path.segments.len() != 1_usize || segment.ident.name != rustc_span::sym::cfg_attr
    {
        return Vec::new();
    }
    let trees: Vec<&TokenTree> = delimited.tokens.iter().collect();
    let mut applied = Vec::new();
    for attribute in split_commas(&trees).into_iter().skip(1_usize) {
        let (arguments, path) = match *attribute.as_slice() {
            | [
                ref path @ ..,
                &TokenTree::Delimited(_, _, Delimiter::Parenthesis, ref tokens),
            ] => (SpecArguments::Delimited(tokens), path),
            | ref path => (SpecArguments::Bare, path),
        };
        let Some(&last) = path.last()
        else {
            continue;
        };
        if matches!(ident_name(last), Maybe::Present(name) if name.as_str() == "spec") {
            applied.push(arguments);
        }
    }
    applied
}

/// Split a `spec` argument list into its clauses.
///
/// # Specification
/// - ensures: a clause opens at an identifier followed by `:` at the start of
///   the list or after a top-level comma, and runs to the comma before the next
///   such identifier or to the end; a comma ending the list belongs to no
///   clause. Tokens before the first key belong to no clause.
/// - ensures: each returned key occurs as an identifier followed by `:` in the
///   input, and the clause count is at most the top-level comma count plus one.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise clause boundaries, nested
///   groups and trailing commas and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| output.len() <= arguments.iter().filter(|tree| token_is!(tree, TokenKind::Comma)).count().saturating_add(1_usize)
    && output.iter().all(|clause| arguments.iter().zip(arguments.iter().skip(1_usize)).any(|(tree, next)| {
        token_is!(next, TokenKind::Colon) && matches!(*tree,
            TokenTree::Token(Token { kind: TokenKind::Ident(name, _), .. }, _) if match clause.key {
                ClauseKey::Requires => name.as_str() == "requires",
                ClauseKey::Maintains => name.as_str() == "maintains",
                ClauseKey::Ensures => name.as_str() == "ensures",
                ClauseKey::Other => !matches!(name.as_str(), "requires" | "maintains" | "ensures"),
            })
    })))]
fn clauses(arguments: &TokenStream) -> Vec<Clause<'_>>
{
    let trees: Vec<&TokenTree> = arguments.iter().collect();
    let mut clauses: Vec<Clause<'_>> = Vec::new();
    let mut at_boundary = true;
    let mut skip_colon = false;
    for (position, tree) in trees.iter().copied().enumerate() {
        if skip_colon {
            skip_colon = false;
            continue;
        }
        let rest = trees
            .get(position.saturating_add(1_usize) ..)
            .unwrap_or_default();
        if at_boundary && let Maybe::Present(key) = clause_key(tree, rest) {
            clauses.push(Clause {
                key,
                value: Vec::new(),
            });
            at_boundary = false;
            skip_colon = true;
            continue;
        }
        if token_is!(tree, TokenKind::Comma) {
            let opens_clause = match rest.split_first() {
                | None => true,
                | Some((&next, after)) => matches!(clause_key(next, after), Maybe::Present(_)),
            };
            if opens_clause {
                at_boundary = true;
                continue;
            }
        }
        if let Some(clause) = clauses.last_mut() {
            clause.value.push(tree);
        }
    }
    clauses
}

/// Read a clause key at an identifier followed by a colon.
///
/// # Specification
/// - requires: `rest` is the token run after `tree`.
/// - ensures: returns the key `tree` names when it is an identifier and `rest`
///   opens with a single colon; `requires`, `maintains` and `ensures` are the
///   executable keys, and any other identifier is [`ClauseKey::Other`].
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise executable keys and
///   non-predicate keys and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn clause_key(
    tree: &TokenTree,
    rest: &[&TokenTree],
) -> Maybe<ClauseKey, clause_reading::NotAKey>
{
    let Maybe::Present(name) = ident_name(tree)
    else {
        return Maybe::Absent(clause_reading::NotAKey::NotKeyShaped);
    };
    if !rest
        .first()
        .is_some_and(|&colon| token_is!(colon, TokenKind::Colon))
    {
        return Maybe::Absent(clause_reading::NotAKey::NotKeyShaped);
    }
    Maybe::Present(match name.as_str() {
        | "requires" => ClauseKey::Requires,
        | "maintains" => ClauseKey::Maintains,
        | "ensures" => ClauseKey::Ensures,
        | _ => ClauseKey::Other,
    })
}

/// The predicates one clause states.
///
/// # Specification
/// - ensures: returns nothing for [`ClauseKey::Other`].
/// - ensures: otherwise a value that is one bracketed group is a list and each
///   element is read; an `ensures` element loses its closure head, and a
///   closure body that is one bracketed group is a list of predicates in turn.
///   Each predicate loses the parentheses and braces that enclose it whole.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise predicate lists, closure
///   bodies and non-predicate keys and compare accepted forms with refused
///   forms.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| clause.key != ClauseKey::Other || output.is_empty())]
fn clause_predicates<'stream>(clause: &Clause<'stream>) -> Vec<Predicate<'stream>>
{
    if clause.key == ClauseKey::Other {
        return Vec::new();
    }
    let mut predicates = Vec::new();
    for element in list_elements(&clause.value) {
        let bodies = if clause.key == ClauseKey::Ensures {
            list_elements(closure_body(&element))
        }
        else {
            vec![element]
        };
        for body in bodies {
            let trees = peeled(&body);
            if !trees.is_empty() {
                predicates.push(Predicate {
                    key: clause.key,
                    trees,
                });
            }
        }
    }
    predicates
}

/// Read a value as a list where it is one bracketed group.
///
/// # Specification
/// - ensures: returns the comma-separated elements of a value that is exactly
///   one bracketed group, and the value itself as one element otherwise.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise single predicates and
///   bracketed predicate lists and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn list_elements<'stream>(value: &[&'stream TokenTree]) -> Vec<Vec<&'stream TokenTree>>
{
    if let [&TokenTree::Delimited(_, _, Delimiter::Bracket, ref inner)] = *value {
        let trees: Vec<&TokenTree> = inner.iter().collect();
        return split_commas(&trees);
    }
    vec![value.to_vec()]
}

/// Remove a closure head from a postcondition.
///
/// # Specification
/// - ensures: returns the tokens after the closing `|` of a leading `|...|`
///   head, after a leading `||`, or after a leading `move` before either; any
///   other value is returned unchanged.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise postcondition closure
///   heads and bodies and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn closure_body<'value, 'stream>(
    value: &'value [&'stream TokenTree]
) -> &'value [&'stream TokenTree]
{
    let value = match value.split_first() {
        | Some((&first, rest)) if ident_name(first) == Maybe::Present(kw::Move) => rest,
        | _ => value,
    };
    let Some((&first, rest)) = value.split_first()
    else {
        return value;
    };
    if token_is!(first, TokenKind::OrOr) {
        return rest;
    }
    if !token_is!(first, TokenKind::Or) {
        return value;
    }
    rest.iter()
        .position(|&tree| token_is!(tree, TokenKind::Or))
        .and_then(|close| rest.get(close.saturating_add(1_usize) ..))
        .unwrap_or(value)
}

/// Remove the groups that enclose a predicate whole.
///
/// # Specification
/// - ensures: while the predicate is exactly one parenthesized or braced group,
///   replaces it with the group's contents; a bracketed group is a value and
///   stays.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise nested predicate
///   parentheses, braces and bracketed values and compare accepted forms with
///   refused forms.
/// - witness: `tests::ui_spec_gates`
fn peeled<'stream>(predicate: &[&'stream TokenTree]) -> Vec<&'stream TokenTree>
{
    let mut trees = predicate.to_vec();
    while let [&TokenTree::Delimited(_, _, Delimiter::Parenthesis | Delimiter::Brace, ref inner)] =
        *trees.as_slice()
    {
        let contents: Vec<&'stream TokenTree> = inner.iter().collect();
        trees = contents;
    }
    trees
}

/// Decide whether a predicate holds on every call by its form alone.
///
/// # Specification
/// - requires: `trees` is one predicate as [`clause_predicates`] returns it.
/// - ensures: returns [`Vacuity::LiteralTrue`] for `true` and `!false`,
///   [`Vacuity::Reflexive`] where [`reflexive`] does, and
///   [`Vacuity::TypeCheck`] where [`wildcard_match`] or
///   [`complementary_queries`] does; `Absent` otherwise.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix pairs each vacuous form with a substantive
///   predicate of the same shape: a literal beside a comparison, a reflexive
///   comparison beside one of two expressions, a wildcard and an exhaustive
///   pair beside a refutable pattern, and a complementary disjunction beside a
///   single query.
/// - witness: `tests::ui_spec_gates`
fn vacuity(trees: &[&TokenTree]) -> Maybe<Vacuity, predicate_reading::Substantive>
{
    match *trees {
        | [literal] if ident_name(literal) == Maybe::Present(kw::True) => {
            return Maybe::Present(Vacuity::LiteralTrue);
        },
        | [bang, literal]
            if token_is!(bang, TokenKind::Bang)
                && ident_name(literal) == Maybe::Present(kw::False) =>
        {
            return Maybe::Present(Vacuity::LiteralTrue);
        },
        | _ => {},
    }
    if let Maybe::Present(kind) = reflexive(trees) {
        return Maybe::Present(kind);
    }
    if let Maybe::Present(kind) = wildcard_match(trees) {
        return Maybe::Present(kind);
    }
    complementary_queries(trees)
}

/// Refuse a predicate comparing one expression with itself.
///
/// # Specification
/// - ensures: returns [`Vacuity::Reflexive`] exactly when the predicate's only
///   top-level comparison or logical operator is one `==`, `<=` or `>=` whose
///   two nonempty sides are token-identical.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise reflexive comparisons and
///   non-reflexive near misses and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn reflexive(trees: &[&TokenTree]) -> Maybe<Vacuity, predicate_reading::Substantive>
{
    let substantive = Maybe::Absent(predicate_reading::Substantive::NoVacuousForm);
    let mut operators = trees.iter().enumerate().filter(|&(_, &tree)| {
        token_is!(
            tree,
            TokenKind::EqEq
                | TokenKind::Le
                | TokenKind::Ge
                | TokenKind::Ne
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::AndAnd
                | TokenKind::OrOr
        )
    });
    let Some((position, &operator)) = operators.next()
    else {
        return substantive;
    };
    if operators.next().is_some()
        || !token_is!(operator, TokenKind::EqEq | TokenKind::Le | TokenKind::Ge)
    {
        return substantive;
    }
    let (Some(left), Some(right)) = (
        trees.get(.. position),
        trees.get(position.saturating_add(1_usize) ..),
    )
    else {
        return substantive;
    };
    if left.is_empty() || !same_tokens(left, right).0 {
        return substantive;
    }
    Maybe::Present(Vacuity::Reflexive)
}

/// Refuse a `matches!` predicate whose pattern admits every value.
///
/// # Specification
/// - ensures: a predicate not beginning with `matches!` is substantive.
/// - ensures: returns [`Vacuity::TypeCheck`] exactly when the predicate is
///   `matches!(subject, pattern)` and the pattern is `_` or the two arms of one
///   of `Result`, `Option` and `Maybe`, each with no payload or a `_` or `..`
///   payload, with no guard.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise wildcards, exhaustive
///   variants and guarded patterns and compare accepted forms with refused
///   forms.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| {
    let starts_matches = matches!(*trees, [name, bang, ..] if matches!(ident_name(name), Maybe::Present(called) if called.as_str() == "matches") && token_is!(bang, TokenKind::Bang));
    starts_matches || output == Maybe::Absent(predicate_reading::Substantive::NoVacuousForm)
})]
fn wildcard_match(trees: &[&TokenTree]) -> Maybe<Vacuity, predicate_reading::Substantive>
{
    let substantive = Maybe::Absent(predicate_reading::Substantive::NoVacuousForm);
    let [
        name,
        bang,
        &TokenTree::Delimited(_, _, Delimiter::Parenthesis, ref inner),
    ] = *trees
    else {
        return substantive;
    };
    let names_matches =
        matches!(ident_name(name), Maybe::Present(called) if called.as_str() == "matches");
    if !names_matches || !token_is!(bang, TokenKind::Bang) {
        return substantive;
    }
    let arguments: Vec<&TokenTree> = inner.iter().collect();
    let parts = split_commas(&arguments);
    let [_, ref pattern] = *parts.as_slice()
    else {
        return substantive;
    };
    if let [wildcard] = *pattern.as_slice()
        && ident_name(wildcard) == Maybe::Present(kw::Underscore)
    {
        return Maybe::Present(Vacuity::TypeCheck);
    }
    if pattern
        .iter()
        .any(|&tree| ident_name(tree) == Maybe::Present(kw::If))
    {
        return substantive;
    }
    let arms: Vec<Vec<&TokenTree>> = split_on(pattern, &TokenKind::Or)
        .into_iter()
        .filter(|arm| !arm.is_empty())
        .collect();
    let [ref first, ref second] = *arms.as_slice()
    else {
        return substantive;
    };
    let (Maybe::Present(first), Maybe::Present(second)) =
        (wildcard_variant(first), wildcard_variant(second))
    else {
        return substantive;
    };
    let complementary = matches!(
        (first.as_str(), second.as_str()),
        ("Ok", "Err")
            | ("Err", "Ok")
            | ("Some", "None")
            | ("None", "Some")
            | ("Present", "Absent")
            | ("Absent", "Present")
    );
    if !complementary {
        return substantive;
    }
    Maybe::Present(Vacuity::TypeCheck)
}

/// The variant name of a pattern arm whose payload admits every value.
///
/// # Specification
/// - ensures: returns the last path segment of an arm written as a path,
///   optionally followed by a parenthesized `_` or `..`; any other arm is
///   `Absent`.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise wildcard and refutable
///   variant payloads and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn wildcard_variant(arm: &[&TokenTree]) -> Maybe<Symbol, pattern_reading::NotWildcard>
{
    let refutable = Maybe::Absent(pattern_reading::NotWildcard::Refutable);
    let path = match *arm {
        | [
            ref path @ ..,
            &TokenTree::Delimited(_, _, Delimiter::Parenthesis, ref payload),
        ] => {
            let payload: Vec<&TokenTree> = payload.iter().collect();
            let admits_all = match *payload.as_slice() {
                | [only] => {
                    ident_name(only) == Maybe::Present(kw::Underscore)
                        || token_is!(only, TokenKind::DotDot)
                },
                | _ => false,
            };
            if !admits_all {
                return refutable;
            }
            path
        },
        | ref path => path,
    };
    let Some((&last, segments)) = path.split_last()
    else {
        return refutable;
    };
    let all_path = segments.iter().all(|&tree| {
        token_is!(tree, TokenKind::PathSep) || matches!(ident_name(tree), Maybe::Present(_))
    });
    if !all_path {
        return refutable;
    }
    match ident_name(last) {
        | Maybe::Present(name) => Maybe::Present(name),
        | Maybe::Absent(_) => refutable,
    }
}

/// A receiver and the nullary method a predicate operand calls on it.
struct DiscriminantQuery<'operand, 'stream>
{
    /// The tokens before the method call.
    receiver: &'operand [&'stream TokenTree],
    /// The method's name.
    query: Symbol,
}

/// Refuse a predicate asking both discriminant queries of one receiver.
///
/// # Specification
/// - ensures: returns [`Vacuity::TypeCheck`] exactly when the predicate is two
///   operands joined by one top-level `||`, each a receiver followed by
///   `.query()`, the receivers token-identical and the queries `is_ok` with
///   `is_err` or `is_some` with `is_none` in either order.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise complementary discriminant
///   queries and different receivers and compare accepted forms with refused
///   forms.
/// - witness: `tests::ui_spec_gates`
fn complementary_queries(trees: &[&TokenTree]) -> Maybe<Vacuity, predicate_reading::Substantive>
{
    let substantive = Maybe::Absent(predicate_reading::Substantive::NoVacuousForm);
    let operands = split_on(trees, &TokenKind::OrOr);
    let [ref first, ref second] = *operands.as_slice()
    else {
        return substantive;
    };
    let (Maybe::Present(first), Maybe::Present(second)) =
        (discriminant_query(first), discriminant_query(second))
    else {
        return substantive;
    };
    let complementary = matches!(
        (first.query.as_str(), second.query.as_str()),
        ("is_ok", "is_err") | ("is_err", "is_ok") | ("is_some", "is_none") | ("is_none", "is_some")
    );
    if !complementary || !same_tokens(first.receiver, second.receiver).0 {
        return substantive;
    }
    Maybe::Present(Vacuity::TypeCheck)
}

/// Split an operand into its receiver and a nullary method name.
///
/// # Specification
/// - ensures: returns the receiver and the method of an operand ending in
///   `.name()` with a nonempty receiver; any other operand is `Absent`.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise nullary queries and other
///   predicate operands and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn discriminant_query<'operand, 'stream>(
    operand: &'operand [&'stream TokenTree]
) -> Maybe<DiscriminantQuery<'operand, 'stream>, query_reading::NotAQuery>
{
    let not_a_query = Maybe::Absent(query_reading::NotAQuery::OtherShape);
    let [
        ref receiver @ ..,
        dot,
        name,
        &TokenTree::Delimited(_, _, Delimiter::Parenthesis, ref arguments),
    ] = *operand
    else {
        return not_a_query;
    };
    if receiver.is_empty() || !token_is!(dot, TokenKind::Dot) || !arguments.is_empty() {
        return not_a_query;
    }
    match ident_name(name) {
        | Maybe::Present(query) => Maybe::Present(DiscriminantQuery { receiver, query }),
        | Maybe::Absent(_) => not_a_query,
    }
}

/// Split token trees at each top-level occurrence of one token.
///
/// # Specification
/// - ensures: returns the runs between occurrences of `separator`, in order,
///   including empty runs; tokens inside groups are never separators.
/// - ensures: rejoining the runs with `separator` reproduces the input tokens.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise top-level separators and
///   grouped tokens and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
#[spec(ensures: |output| {
    let mut input = trees.iter().copied();
    !output.is_empty() && output.iter().enumerate().all(|(index, run)| {
        (index == 0_usize || input.next().is_some_and(|tree| matches!(*tree,
            TokenTree::Token(ref token, _) if token.kind == *separator)))
            && run.iter().all(|tree| input.next().is_some_and(|original| core::ptr::eq(*tree, original)))
    }) && input.next().is_none()
})]
fn split_on<'stream>(
    trees: &[&'stream TokenTree],
    separator: &TokenKind,
) -> Vec<Vec<&'stream TokenTree>>
{
    let mut runs: Vec<Vec<&TokenTree>> = vec![Vec::new()];
    for tree in trees.iter().copied() {
        if matches!(*tree, TokenTree::Token(ref token, _) if token.kind == *separator) {
            runs.push(Vec::new());
        }
        else if let Some(run) = runs.last_mut() {
            run.push(tree);
        }
    }
    runs
}

/// Split token trees at their top-level commas.
///
/// # Specification
/// - ensures: returns the runs between top-level commas, dropping the empty run
///   a trailing comma leaves.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise comma-separated lists and
///   trailing commas and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn split_commas<'stream>(trees: &[&'stream TokenTree]) -> Vec<Vec<&'stream TokenTree>>
{
    let mut runs = split_on(trees, &TokenKind::Comma);
    if runs.last().is_some_and(Vec::is_empty) {
        let _trailing = runs.pop();
    }
    runs
}

/// Compare two token runs without their spans.
///
/// # Specification
/// trivial.
fn same_tokens(
    left: &[&TokenTree],
    right: &[&TokenTree],
) -> SameTokens
{
    SameTokens(
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(left, right)| left.eq_unspanned(right)),
    )
}

/// The span covering a nonempty token run.
///
/// # Specification
/// - ensures: returns the span from the first tree's start to the last tree's
///   end, and the dummy span for an empty run.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise diagnostic spans over
///   predicate token runs and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn trees_span(trees: &[&TokenTree]) -> Span
{
    match (trees.first(), trees.last()) {
        | (Some(first), Some(last)) => first.span().to(last.span()),
        | _ => rustc_span::DUMMY_SP,
    }
}

/// The name of an identifier token.
///
/// # Specification
/// - ensures: returns the symbol of an identifier token, keywords and raw
///   identifiers included, and `Absent` for any other tree.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise keyword, identifier and
///   non-identifier token forms and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
fn ident_name(tree: &TokenTree) -> Maybe<Symbol, token_reading::NotIdent>
{
    match *tree {
        | TokenTree::Token(
            Token {
                kind: TokenKind::Ident(name, _),
                ..
            },
            _,
        ) => Maybe::Present(name),
        | _ => Maybe::Absent(token_reading::NotIdent::OtherTree),
    }
}

/// How a block states that no runtime predicate expresses its obligation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exemption
{
    /// The block carries no `- executable:` clause.
    Absent,
    /// The clause reads `none — <reason>` with a nonempty reason.
    Stated,
    /// The clause does not read `none — <reason>`.
    Unreasoned,
    /// A clause other than `- intension:` follows the exemption.
    NotLast,
    /// The block carries more than one `- executable:` clause.
    Duplicated,
}

/// The exemption clause's prefix.
const EXEMPTION: &str = "- executable:";

/// The one clause that may follow the exemption.
const INTENSION: &str = "- intension:";

/// Read the exemption clause of a `# Specification` block.
///
/// # Specification
/// - requires: `bullets` is the folded bullet list of the block, as
///   [`crate::rustdoc::section_lines`] returns it.
/// - ensures: returns [`Exemption::Duplicated`] for two or more `- executable:`
///   bullets; otherwise [`Exemption::NotLast`] when a bullet other than `-
///   intension:` follows the one; otherwise [`Exemption::Unreasoned`] when its
///   value is not `none`, an em dash and a nonempty reason; otherwise
///   [`Exemption::Stated`]; and [`Exemption::Absent`] for a block with no such
///   bullet.
/// - panics: none.
/// - executable: none — the function is its own specification; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the stated exemption, the exemption
///   followed by an intension, an exemption with no reason, one with another
///   separator, one followed by a clause, two exemptions, and a block with
///   none.
/// - witness: `executable::tests::the_exemption_reads_none_and_a_reason`
/// - witness: `executable::tests::only_an_intension_follows_the_exemption`
/// - witness: `executable::tests::the_exemption_is_stated_once`
/// - witness: `tests::ui_spec_gates`
fn read_exemption(bullets: &[String]) -> Exemption
{
    let mut seen = 0_usize;
    let mut stated = Exemption::Absent;
    let mut followed = false;
    for bullet in bullets {
        if let Some(value) = bullet.strip_prefix(EXEMPTION) {
            seen = seen.saturating_add(1_usize);
            stated = exemption_value(RustdocLine::from(value));
            continue;
        }
        if seen > 0_usize && !bullet.starts_with(INTENSION) {
            followed = true;
        }
    }
    match seen {
        | 0_usize => Exemption::Absent,
        | 1_usize if followed => Exemption::NotLast,
        | 1_usize => stated,
        | _ => Exemption::Duplicated,
    }
}

/// Read an exemption's value.
///
/// # Specification
/// - ensures: returns [`Exemption::Stated`] exactly when the value, trimmed,
///   opens with the word `none`, then an em dash, then a nonempty reason, and
///   [`Exemption::Unreasoned`] otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the exemption cases distinguish a stated reason from an
///   empty reason and an incorrect separator.
/// - witness: `executable::tests::the_exemption_reads_none_and_a_reason`
#[spec(ensures: |output| (output == Exemption::Stated) == value.0.trim().split_once('—')
    .is_some_and(|(prefix, reason)| prefix.trim_end() == "none" && !reason.trim().is_empty()))]
fn exemption_value(value: RustdocLine<'_>) -> Exemption
{
    let reasoned = value
        .0
        .trim()
        .strip_prefix("none")
        .and_then(|rest| rest.trim_start().strip_prefix('—'))
        .is_some_and(|reason| !reason.trim().is_empty());
    if reasoned {
        Exemption::Stated
    }
    else {
        Exemption::Unreasoned
    }
}

/// Why a clause-bearing block fails the executable obligation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutableDefect
{
    /// Neither an attribute nor an exemption is written.
    AttributeAbsent,
    /// The attribute states no executable clause, and no exemption is written.
    ClauseAbsent,
    /// A predicate holds on every call.
    Vacuous(Vacuity, Span),
    /// The exemption does not read `none — <reason>`.
    ExemptionUnreasoned,
    /// A clause other than `- intension:` follows the exemption.
    ExemptionNotLast,
    /// The exemption is written more than once.
    ExemptionDuplicated,
    /// The exemption contradicts an executable predicate on the same item.
    ExemptionBesideClause,
}

impl ExecutableDefect
{
    /// The defect selects its repair-directed diagnostic.
    ///
    /// # Specification
    /// trivial.
    fn message(self) -> DiagnosticText<'static>
    {
        DiagnosticText(match self {
            | Self::AttributeAbsent => {
                "this function's `# Specification` states clauses but it carries no `#[spec(...)]` \
                 attribute; an executable predicate checks a clause on every enforcing call, and an \
                 obligation no runtime predicate can express is exempted in the block"
            },
            | Self::ClauseAbsent => {
                "this function's `#[spec]` states no `requires:`, `maintains:` or `ensures:` \
                 predicate, so it checks nothing on any call"
            },
            | Self::Vacuous(Vacuity::LiteralTrue, _) => {
                "this predicate is a literal truth: it holds on every call, so it checks nothing \
                 the signature does not"
            },
            | Self::Vacuous(Vacuity::Reflexive, _) => {
                "this predicate compares an expression with itself: it holds on every call, so it \
                 checks nothing the signature does not"
            },
            | Self::Vacuous(Vacuity::TypeCheck, _) => {
                "this predicate holds for every value of its subject's type: it restates the \
                 signature rather than checking a clause"
            },
            | Self::Vacuous(Vacuity::Repeated, _) => {
                "this predicate repeats an earlier one under the same clause, so it checks nothing \
                 more"
            },
            | Self::ExemptionUnreasoned => {
                "`- executable:` must read `none — <reason>`: the exemption claims that no runtime \
                 predicate expresses the obligation, and the reason is what review weighs"
            },
            | Self::ExemptionNotLast => {
                "`- executable:` must be the block's last clause before `- intension:`"
            },
            | Self::ExemptionDuplicated => "`- executable:` may appear at most once",
            | Self::ExemptionBesideClause => {
                "`- executable: none` claims no runtime predicate expresses this function's \
                 obligation, but its `#[spec]` states one; keep the predicate and drop the \
                 exemption, or the reverse"
            },
        })
    }
}

/// Denial help exposing the two admitted ways to discharge the obligation.
const EXECUTABLE_SHAPES: &str = concat!(
    "state a substantive predicate with `#[spec(requires: ...)]`, `#[spec(maintains: ...)]` or ",
    "`#[spec(ensures: |output| ...)]`, or end the block's clauses, before any `- intension:`, ",
    "with `- executable: none — <why no runtime predicate expresses the obligation>`",
);

/// Combine the block's exemption with the attribute record.
///
/// # Specification
/// - ensures: a malformed, misplaced or repeated exemption is its own defect;
///   then a vacuous predicate is a defect whatever the exemption says; then a
///   stated exemption beside a substantive predicate is
///   [`ExecutableDefect::ExemptionBesideClause`]; then a stated exemption or a
///   substantive predicate satisfies the obligation; and otherwise the missing
///   attribute or its missing clause is the defect.
/// - ensures: an exempt result requires a stated exemption; a predicate-stated
///   result requires a stated attribute; neither admitted input combination
///   returns a defect.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests take every pair of exemption state and
///   attribute record and assert the exact outcome.
/// - witness: `executable::tests::every_exemption_and_record_pair_has_one_outcome`
#[spec(ensures: |output| match output {
    Maybe::Absent(executable_check::Satisfied::Exempt) => exemption == Exemption::Stated,
    Maybe::Absent(executable_check::Satisfied::PredicateStated) => attribute == ExecutableAttribute::Stated,
    Maybe::Present(_) => !matches!((exemption, attribute),
        (Exemption::Stated, ExecutableAttribute::Absent | ExecutableAttribute::ClauseAbsent)
        | (Exemption::Absent, ExecutableAttribute::Stated)),
})]
fn executable_defect(
    exemption: Exemption,
    attribute: ExecutableAttribute,
) -> Maybe<ExecutableDefect, executable_check::Satisfied>
{
    match (exemption, attribute) {
        | (Exemption::Duplicated, _) => Maybe::Present(ExecutableDefect::ExemptionDuplicated),
        | (Exemption::NotLast, _) => Maybe::Present(ExecutableDefect::ExemptionNotLast),
        | (Exemption::Unreasoned, _) => Maybe::Present(ExecutableDefect::ExemptionUnreasoned),
        | (_, ExecutableAttribute::Vacuous(kind, span)) => {
            Maybe::Present(ExecutableDefect::Vacuous(kind, span))
        },
        | (Exemption::Stated, ExecutableAttribute::Stated) => {
            Maybe::Present(ExecutableDefect::ExemptionBesideClause)
        },
        | (Exemption::Stated, ExecutableAttribute::Absent | ExecutableAttribute::ClauseAbsent) => {
            Maybe::Absent(executable_check::Satisfied::Exempt)
        },
        | (Exemption::Absent, ExecutableAttribute::Stated) => {
            Maybe::Absent(executable_check::Satisfied::PredicateStated)
        },
        | (Exemption::Absent, ExecutableAttribute::Absent) => {
            Maybe::Present(ExecutableDefect::AttributeAbsent)
        },
        | (Exemption::Absent, ExecutableAttribute::ClauseAbsent) => {
            Maybe::Present(ExecutableDefect::ClauseAbsent)
        },
    }
}

/// Report a clause-bearing block that neither checks nor exempts its
/// obligation.
///
/// # Specification
/// - requires: the item is inside the specification presence rule, `name` is
///   its authored name span, and `bullets` is the folded bullet list of its
///   clause-bearing `# Specification` block.
/// - ensures: reports the defect [`executable_defect`] finds, at the vacuous
///   predicate for a vacuous one and at `name` otherwise; reports nothing when
///   the obligation is satisfied.
/// - provides: the reporting half of [`SPEC_ATTRIBUTE_PRESENT`].
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L2 — the compiler fixtures exercise missing, exempt, vacuous
///   and substantive predicates and compare accepted forms with refused forms.
/// - witness: `tests::ui_spec_gates`
pub fn check(
    cx: &LateContext<'_>,
    name: Span,
    bullets: &[String],
    index: &AttributeIndex,
)
{
    let Maybe::Present(defect) = executable_defect(read_exemption(bullets), index.lookup(name))
    else {
        return;
    };
    let span = match defect {
        | ExecutableDefect::Vacuous(_, predicate) => predicate,
        | _ => name,
    };
    span_lint_and_help(
        cx,
        SPEC_ATTRIBUTE_PRESENT,
        span,
        defect.message().0,
        None,
        EXECUTABLE_SHAPES,
    );
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::ExecutableAttribute;
    use super::ExecutableDefect;
    use super::Exemption;
    use super::Vacuity;
    use super::executable_defect;
    use super::read_exemption;
    use crate::semantic::RustdocLine;

    /// Fixture bullets as the section reader folds them.
    ///
    /// # Specification
    /// trivial.
    fn bullets(lines: &[RustdocLine<'_>]) -> Vec<String>
    {
        lines.iter().map(|line| line.0.to_owned()).collect()
    }

    #[test]
    fn the_exemption_reads_none_and_a_reason()
    {
        assert_eq!(
            read_exemption(&bullets(&[
                RustdocLine("- ensures: the arena is unchanged."),
                RustdocLine("- panics: none."),
                RustdocLine("- executable: none — the arena has no equality to compare."),
            ])),
            Exemption::Stated,
            "`none`, an em dash and a reason is the exemption"
        );
        for value in [
            "- executable: none",
            "- executable: none —",
            "- executable: none —   ",
            "- executable: none - the hyphen is not the separator.",
            "- executable: none: the colon is not the separator.",
            "- executable: nothing — the word is `none`.",
        ] {
            assert_eq!(
                read_exemption(&bullets(&[
                    RustdocLine("- panics: none."),
                    RustdocLine(value)
                ])),
                Exemption::Unreasoned,
                "and anything else is refused: {value}"
            );
        }
        assert_eq!(
            read_exemption(&bullets(&[
                RustdocLine("- requires: `count` is positive."),
                RustdocLine("- panics: none.")
            ])),
            Exemption::Absent,
            "a block with no exemption states none"
        );
    }

    #[test]
    fn only_an_intension_follows_the_exemption()
    {
        assert_eq!(
            read_exemption(&bullets(&[
                RustdocLine("- panics: none."),
                RustdocLine("- executable: none — the cost is the obligation."),
                RustdocLine("- intension: one pass over the input."),
            ])),
            Exemption::Stated,
            "the intension is the one clause after the exemption"
        );
        assert_eq!(
            read_exemption(&bullets(&[
                RustdocLine("- executable: none — the cost is the obligation."),
                RustdocLine("- panics: none."),
            ])),
            Exemption::NotLast,
            "and any other clause after it is refused"
        );
    }

    #[test]
    fn the_exemption_is_stated_once()
    {
        assert_eq!(
            read_exemption(&bullets(&[
                RustdocLine("- panics: none."),
                RustdocLine("- executable: none — the first reason."),
                RustdocLine("- executable: none — the second reason."),
            ])),
            Exemption::Duplicated,
            "two exemptions are one too many"
        );
    }

    #[test]
    fn every_exemption_and_record_pair_has_one_outcome()
    {
        let span = rustc_span::DUMMY_SP;
        let records = [
            ExecutableAttribute::Absent,
            ExecutableAttribute::ClauseAbsent,
            ExecutableAttribute::Stated,
            ExecutableAttribute::Vacuous(Vacuity::LiteralTrue, span),
        ];
        for record in records {
            assert_eq!(
                executable_defect(Exemption::Duplicated, record),
                Maybe::Present(ExecutableDefect::ExemptionDuplicated),
                "a repeated exemption is refused whatever the attribute states: {record:?}"
            );
            assert_eq!(
                executable_defect(Exemption::NotLast, record),
                Maybe::Present(ExecutableDefect::ExemptionNotLast),
                "and so is a misplaced one: {record:?}"
            );
            assert_eq!(
                executable_defect(Exemption::Unreasoned, record),
                Maybe::Present(ExecutableDefect::ExemptionUnreasoned),
                "and an unreasoned one: {record:?}"
            );
        }
        let vacuous = ExecutableAttribute::Vacuous(Vacuity::TypeCheck, span);
        for exemption in [Exemption::Absent, Exemption::Stated] {
            assert_eq!(
                executable_defect(exemption, vacuous),
                Maybe::Present(ExecutableDefect::Vacuous(Vacuity::TypeCheck, span)),
                "a vacuous predicate is refused beside a stated exemption or none: {exemption:?}"
            );
        }
        assert_eq!(
            executable_defect(Exemption::Stated, ExecutableAttribute::Stated),
            Maybe::Present(ExecutableDefect::ExemptionBesideClause),
            "an exemption contradicts a substantive predicate"
        );
        for record in [
            ExecutableAttribute::Absent,
            ExecutableAttribute::ClauseAbsent,
        ] {
            assert!(
                matches!(
                    executable_defect(Exemption::Stated, record),
                    Maybe::Absent(_)
                ),
                "a stated exemption satisfies the obligation without a predicate: {record:?}"
            );
        }
        assert!(
            matches!(
                executable_defect(Exemption::Absent, ExecutableAttribute::Stated),
                Maybe::Absent(_)
            ),
            "a substantive predicate satisfies it without an exemption"
        );
        assert_eq!(
            executable_defect(Exemption::Absent, ExecutableAttribute::Absent),
            Maybe::Present(ExecutableDefect::AttributeAbsent),
            "neither is the missing attribute"
        );
        assert_eq!(
            executable_defect(Exemption::Absent, ExecutableAttribute::ClauseAbsent),
            Maybe::Present(ExecutableDefect::ClauseAbsent),
            "and an attribute with no executable clause is its missing clause"
        );
    }
}
