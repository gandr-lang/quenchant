//! The mode plane: fallback arms over the checking judgement's own scrutinees.
//!
//! A bidirectional judgement is a set of directed rules, and each rule's
//! direction is forced by the syntactic class of the term it applies to. A rule
//! that reads its mode off a *value* instead — a direction carried at runtime,
//! or the expected type it is checking against — turns "no rule applies here"
//! into "synthesise, and hope the conversion agrees". The observable signature
//! of that defect is a single fallback arm, and every term it swallows is one
//! the judgement had no rule for.
//!
//! [`MODE_DISPATCH_WILDCARD`] denies the shape rather than counting it. Inside
//! the crate that declares the judgement, a wildcard or bare-binding arm in a
//! match over one of the judgement's scrutinees is denied, guard or no guard:
//! an arm that does not name the case it handles is the fallback whatever else
//! it also tests.
//!
//! # The crate declares its own scrutinees
//!
//! The gate reads a `# Judgement` rustdoc section, in the fixed grammar the
//! module documents beside the lint declaration. Two bullets exist, and each
//! sits on the item that owns what it declares: `- direction:` on the type
//! definition whose values are the judgement's modes, and `- expected:` on the
//! function whose named parameter carries the type a term is checked against.
//!
//! Declaring on the item rather than in a configuration file is what keeps the
//! declaration correct as the code moves: the mark travels with the definition,
//! and removing it is an edit to the definition that a reviewer reads.
//!
//! # Why the expected plane names a parameter, not a type
//!
//! The expected type of a checker is the *core language's* type vocabulary, and
//! the crate that owns that vocabulary is not the crate that declares the
//! judgement. A gate keyed on the type would therefore need a mark on a crate
//! the judgement never edits, and would fire on every unrelated match over a
//! core type besides. Keying on the declared parameter keeps the mark and the
//! rule both inside the judgement's own text, and asks the question the
//! discipline actually asks: did this arm reach its answer by inspecting what
//! the term is being checked against?
//!
//! # A section that declares nothing is denied
//!
//! Every way of writing the section without declaring anything is a defect of
//! its own — a bullet with no value, an `- expected:` naming no parameter of
//! the function, a section on an item that carries no scrutinee. Each reads as
//! an opted-in gate and gates nothing, which is the failure a silent
//! opt-in mechanism has and a loud one does not.
//!
//! The reach is therefore every item, not only the two that can carry a
//! scrutinee: an associated const, an associated type, a body-less required
//! trait method and a foreign function each draw the misplacement denial. A
//! declaration read nowhere would be exactly the silence this section exists to
//! refuse, reached by the route of never looking.

use alloc::collections::VecDeque;

use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// Grammar defects retain first-observed precedence during declaration reading.
    mod declaration_grammar {
        /// Why the parsed prefix carries no grammar defect.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Accepted {
            /// No processed bullet violates the declaration grammar.
            NoDefectObserved,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A declaration's grammar and attachment site must agree.
    mod declaration_site {
        /// Why no declaration-site diagnostic is required.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Accepted {
            /// The declaration is grammatical, correctly placed, and its names bind.
            MatchesDeclarationSite,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Parameter names are single quoted tokens, not descriptive text.
    mod name_syntax {
        /// Why a value cannot be used as an exact parameter name.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// No opening backtick starts the trimmed value.
            OpeningBacktickAbsent,
            /// No closing backtick ends the value.
            ClosingBacktickAbsent,
            /// The quoted value contains no name.
            EmptyName,
            /// Another backtick interrupts the name.
            InteriorBacktick,
            /// Whitespace prevents the value from denoting one binding name.
            Whitespace,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Destructuring patterns are not a parameter's single named binding.
    mod parameter_lookup {
        /// Why an exact parameter lookup supplies no HIR binding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// No plain parameter binding has the requested spelling.
            Unbound,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Only recognized authored roles activate the dispatch rule.
    mod role_lookup {
        /// Why this scrutinee supplies no judgment role to inspect.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// Neither the declared direction nor a declared expected parameter is recognized.
            NoRecognizedRole,
        }
    }
}

use clippy_utils::diagnostics::span_lint;
use clippy_utils::diagnostics::span_lint_and_then;
use rustc_hir::Body;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::FnDecl;
use rustc_hir::ForeignItem;
use rustc_hir::HirId;
use rustc_hir::ImplItem;
use rustc_hir::ImplItemKind;
use rustc_hir::Item;
use rustc_hir::ItemKind;
use rustc_hir::MatchSource;
use rustc_hir::Pat;
use rustc_hir::PatKind;
use rustc_hir::QPath;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::FnKind;
use rustc_hir::intravisit::Visitor;
use rustc_hir::intravisit::walk_expr;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::hir::nested_filter;
use rustc_middle::ty as rustc_ty;
use rustc_middle::ty::TyCtxt;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::rustdoc::indented_rustdoc_lines;
use crate::rustdoc::section_lines;
use crate::semantic::DeclaredExpectedParameter;
use crate::semantic::DiagnosticText;
use crate::semantic::DirectionDeclared;
use crate::semantic::ParameterName;
use crate::semantic::RustdocLine;
use crate::semantic::ScrutineeIsDirection;
use crate::semantic::ScrutineeMentionsExpected;
use crate::semantic::SectionHeading;

declare_lint! {
    /// ### What it does
    ///
    /// Denies a wildcard or bare-binding arm in a match whose scrutinee is the
    /// checking judgement's direction, or a parameter declared to carry the
    /// type a term is checked against.
    ///
    /// ### Why is this bad?
    ///
    /// A bidirectional judgement forces each rule's direction by the syntactic
    /// class of the term, so every arm of a match over a mode names the case it
    /// handles. A fallback arm answers for the cases the judgement has no rule
    /// for — it turns an absent rule into a silent synthesis, and the terms it
    /// swallows are exactly the ones nobody wrote a rule for. Counting the arms
    /// in prose does not survive review fatigue; denying the shape does.
    ///
    /// A guard changes nothing: an arm that does not name its case is the
    /// fallback whatever else it tests.
    ///
    /// ### How a crate declares its judgement
    ///
    /// The gate reads a `# Judgement` rustdoc section. `- direction:` sits on
    /// the type definition whose values are the modes, and states what the
    /// direction is; `- expected:` sits on a judgement face and names, in
    /// backticks, the one parameter carrying the expected type.
    ///
    /// ```rust
    /// /// The direction of the checking judgement.
    /// ///
    /// /// # Judgement
    /// /// - direction: the two modes, forced by the term's syntactic class.
    /// enum Direction {
    ///     Synthesises,
    ///     Checks,
    /// }
    ///
    /// /// Check a value against the type it is offered at.
    /// ///
    /// /// # Judgement
    /// /// - expected: `expected`
    /// fn check_value(term: Term, expected: TypeHead) -> Verdict {
    ///     Verdict::Accepted
    /// }
    /// ```
    ///
    /// A section declaring neither bullet, a bullet with no value, an
    /// `- expected:` naming no parameter of its function, and a section on an
    /// item that carries no scrutinee are each denied: a declaration that names
    /// nothing gates nothing, and reads as though it did.
    ///
    /// ### Scope
    ///
    /// A declaration *marks* a scrutinee only on a type definition or on a
    /// function with a body, which is where the judgement's scrutinees are; on
    /// every other item — an associated const, an associated type, a required
    /// trait method, a foreign function — it is read and denied as misplaced
    /// rather than passed over, so an opted-in gate never goes quiet.
    ///
    /// The gate is **crate-local by construction**: the direction's `DefId`
    /// must resolve local and the declaration is read off local rustdoc, so a
    /// wildcard match on the judgement's direction written in a *consuming*
    /// crate is not denied. That is the intended scope — the constraint is the
    /// declaring crate's own specification, and code downstream of the judgement is
    /// not writing the judgement.
    ///
    /// A `match` written inside a macro expansion is not reported. The case
    /// that motivates it is `matches!(direction, ..)`, which carries a fallback
    /// arm the author never wrote and asks a boolean question rather than
    /// choosing a mode; the exemption is wider than that case, and a
    /// crate-local `macro_rules!` hiding a fallback arm rides it too. The
    /// narrower rule would need to tell an author's own expansion from a
    /// dependency's, and reporting into a macro body names a span the author of
    /// the *call* cannot act on.
    ///
    /// ### Blind spots
    ///
    /// This gate reads match arms, and `if let`, `let .. else` and `while let`
    /// have none: their else branch is a fallback the HIR spells as a
    /// conditional rather than as an arm, and it is not reported.
    /// `ui/mode_dispatch.rs::if_let_fallback` is the fixture that demonstrates
    /// the miss, and its absence from `ui/mode_dispatch.stderr` is the record.
    ///
    /// The direction plane is a type-identity test on the *whole* scrutinee, so
    /// a match dispatching on a pair — `match (direction, term) { .., _ => .. }`
    /// — is not gated: the tuple's type is not the declared type, references
    /// peeled or not. The expected plane does not reach it either, since its
    /// search is for a declared expected parameter and never for the direction.
    /// `ui/mode_dispatch.rs::tupled_scrutinee` is that fixture, and its silence
    /// in the same `.stderr` is that record.
    ///
    /// The judgement's own shape is what narrows both: the four faces are
    /// separate functions taking separate terms, so a mode chosen anywhere is a
    /// mode chosen over a scrutinee this gate can see.
    ///
    /// ### Example
    ///
    /// ```rust
    /// match direction {
    ///     Direction::Checks => check(term, expected),
    ///     _ => synthesise(term),
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// match direction {
    ///     Direction::Checks => check(term, expected),
    ///     Direction::Synthesises => synthesise(term),
    /// }
    /// ```
    pub MODE_DISPATCH_WILDCARD,
    Deny,
    "the checking judgement's modes are named one arm at a time, never by a fallback"
}

impl_lint_pass!(WorkflowJudgement => [MODE_DISPATCH_WILDCARD]);

/// Late lint pass denying fallback arms over the judgement's scrutinees.
pub struct WorkflowJudgement;

impl<'tcx> LateLintPass<'tcx> for WorkflowJudgement
{
    /// Read a non-function item's declaration, at its own name.
    ///
    /// # Specification
    /// - ensures: reports the declaration's misplacement defect for every item
    ///   but a function, treating a type definition as the one site that may
    ///   carry `- direction:`; a function item is left to `check_fn`.
    /// - panics: none.
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        // A function item is an item and a body both; its declaration is read
        // once, at `check_fn`, where the parameters it names are in reach.
        if matches!(item.kind, ItemKind::Fn { .. }) {
            return;
        }
        let site = if matches!(
            item.kind,
            ItemKind::Struct(..) | ItemKind::Enum(..) | ItemKind::Union(..)
        ) {
            DeclarationSite::TypeDefinition
        }
        else {
            DeclarationSite::Elsewhere
        };
        let span = item.kind.ident().map_or(item.span, |ident| ident.span);
        report_declaration_defect(cx, item.owner_id.def_id, span, site);
    }

    /// Read a function's declaration, at its own name.
    ///
    /// # Specification
    /// - ensures: reports the declaration's defect for every function with a
    ///   body, resolving `- expected:` against the parameters that body binds;
    ///   a closure carries no declaration and is skipped.
    /// - panics: none.
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        fn_kind: FnKind<'tcx>,
        _fn_decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    )
    {
        if matches!(fn_kind, FnKind::Closure) {
            return;
        }
        let span = cx.tcx.def_ident_span(def_id).unwrap_or(span);
        report_declaration_defect(cx, def_id, span, DeclarationSite::Function(body));
    }

    /// Read an associated item's declaration, at its own name.
    ///
    /// # Specification
    /// - ensures: reports the misplacement defect for every associated item but
    ///   a method, which reaches `check_fn` with its parameters in scope.
    /// - panics: none.
    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        impl_item: &'tcx ImplItem<'tcx>,
    )
    {
        // An associated function has a body, so `check_fn` reads its
        // declaration where the parameters it names are in reach. Everything
        // else in an impl carries no scrutinee at all.
        if matches!(impl_item.kind, ImplItemKind::Fn(..)) {
            return;
        }
        report_declaration_defect(
            cx,
            impl_item.owner_id.def_id,
            impl_item.ident.span,
            DeclarationSite::Elsewhere,
        );
    }

    /// Read a trait item's declaration, at its own name.
    ///
    /// # Specification
    /// - ensures: reports the misplacement defect for every trait item but a
    ///   provided method, which reaches `check_fn`; a required method's
    ///   parameters bind nothing a match can dispatch on.
    /// - panics: none.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx TraitItem<'tcx>,
    )
    {
        // A provided method has a body and reaches `check_fn`. A required one
        // has none: its parameters bind nothing a match could dispatch on, and
        // an implementation's parameters are its own, so a declaration written
        // here gates nothing and belongs on the implementation.
        if matches!(trait_item.kind, TraitItemKind::Fn(_, TraitFn::Provided(_))) {
            return;
        }
        report_declaration_defect(
            cx,
            trait_item.owner_id.def_id,
            trait_item.ident.span,
            DeclarationSite::Elsewhere,
        );
    }

    /// Read a foreign item's declaration, at its own name.
    ///
    /// # Specification
    /// - ensures: reports the misplacement defect on every foreign declaration,
    ///   which holds no scrutinee at all.
    /// - panics: none.
    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    )
    {
        report_declaration_defect(
            cx,
            item.owner_id.def_id,
            item.ident.span,
            DeclarationSite::Elsewhere,
        );
    }

    /// Deny a fallback arm in a match over a declared scrutinee.
    ///
    /// # Specification
    /// - ensures: reports every arm naming no case of an ordinary match whose
    ///   scrutinee is the declared direction or mentions the declared expected
    ///   parameter; a match inside an expansion is left alone.
    /// - panics: none.
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if expr.span.from_expansion() {
            return;
        }
        let ExprKind::Match(scrutinee, arms, MatchSource::Normal) = expr.kind
        else {
            return;
        };
        let Maybe::Present(role) = scrutinee_role(cx, scrutinee)
        else {
            return;
        };
        for arm in arms {
            for span in fallback_spans(arm.pat) {
                span_lint_and_then(cx, MODE_DISPATCH_WILDCARD, span, role.message().0, |diag| {
                    diag.span_note(scrutinee.span, role.note().0);
                    diag.help(FALLBACK_HELP);
                });
            }
        }
    }
}

/// The heading that opens a judgement declaration.
const HEADING: &str = "# Judgement";

/// The bullet declaring the type whose values are the judgement's modes.
const DIRECTION_BULLET: &str = "- direction:";

/// The bullet naming the parameter that carries the expected type.
const EXPECTED_BULLET: &str = "- expected:";

/// The help attached to every fallback-arm diagnostic.
const FALLBACK_HELP: &str = concat!(
    "name every case in an arm of its own; a term the judgement has no rule for is a refusal, and ",
    "a fallback arm answers for it instead",
);

/// Which of the judgement's scrutinees a match dispatches on.
enum ScrutineeRole
{
    /// The scrutinee's type is the declared checking direction.
    Direction,
    /// The scrutinee mentions a parameter declared to carry the expected type.
    ExpectedType,
}

impl ScrutineeRole
{
    /// Return the diagnostic text for a fallback arm over this scrutinee.
    ///
    /// # Specification
    /// trivial.
    fn message(&self) -> DiagnosticText<'static>
    {
        DiagnosticText(match *self {
            | Self::Direction => {
                "fallback arm in a match on the checking judgement's direction: a directed rule \
                 names the direction it applies to, so a mode reached through a fallback is a rule \
                 nobody wrote"
            },
            | Self::ExpectedType => {
                "fallback arm in a match on the expected type: no arm of the judgement chooses its \
                 mode by inspecting the type a term is checked against"
            },
        })
    }

    /// Return the note pointing at the scrutinee this role was decided from.
    ///
    /// # Specification
    /// trivial.
    fn note(&self) -> DiagnosticText<'static>
    {
        DiagnosticText(match *self {
            | Self::Direction => "this scrutinee's type declares itself the judgement's direction",
            | Self::ExpectedType => {
                "this scrutinee mentions the parameter declared to carry the expected type"
            },
        })
    }
}

/// The kind of item a `# Judgement` section was written on.
#[derive(Clone, Copy)]
enum DeclarationSite<'body>
{
    /// A type definition, whose values can be the judgement's modes.
    TypeDefinition,
    /// A function, whose parameters can carry the expected type.
    Function(&'body Body<'body>),
    /// Anything else, which carries no scrutinee of the judgement.
    Elsewhere,
}

/// What one item's `# Judgement` section declares.
struct Declaration
{
    /// Whether the item declares itself the judgement's checking direction.
    direction: DirectionDeclared,
    /// The parameter names declared to carry the expected type.
    expected: Vec<String>,
    /// The first way the section fails the fixed grammar, if it does.
    defect: Maybe<JudgementDefect, declaration_grammar::Accepted>,
}

/// Why a `# Judgement` section declares nothing the gate can act on.
enum JudgementDefect
{
    /// The section carries neither of the two bullets.
    DeclaresNothing,
    /// The `- direction:` bullet states no value.
    DirectionUnexplained,
    /// An `- expected:` bullet names no single backticked parameter.
    ExpectedMalformed,
    /// An `- expected:` bullet names something no parameter binds.
    ExpectedUnbound(String),
    /// `- direction:` sits on an item that is not a type definition.
    DirectionOffType,
    /// `- expected:` sits on an item that is not a function.
    ExpectedOffFunction,
    /// The section sits on an item carrying no scrutinee at all.
    SectionMisplaced,
}

impl JudgementDefect
{
    /// Return the diagnostic text for this defect.
    ///
    /// # Specification
    /// trivial.
    fn message(&self) -> String
    {
        match *self {
            | Self::DeclaresNothing => {
                "`# Judgement` section declares nothing: state `- direction:` on the type whose \
                 values are the judgement's modes, or `- expected: `name`` on the parameter \
                 carrying the type a term is checked against"
                    .to_owned()
            },
            | Self::DirectionUnexplained => {
                "`- direction:` states no value: say what the direction is, so the declaration \
                 reads as a claim rather than as a marker"
                    .to_owned()
            },
            | Self::ExpectedMalformed => {
                "`- expected:` names exactly one parameter in backticks, as `- expected: \
                 `expected``"
                    .to_owned()
            },
            | Self::ExpectedUnbound(ref name) => format!(
                "`- expected:` names `{name}`, which no parameter of this function binds; a \
                 declaration that names nothing gates nothing"
            ),
            | Self::DirectionOffType => {
                "`- direction:` declares the type whose values are the judgement's modes, so it \
                 belongs on that type's definition"
                    .to_owned()
            },
            | Self::ExpectedOffFunction => {
                "`- expected:` names a parameter carrying the expected type, so it belongs on the \
                 function that takes it"
                    .to_owned()
            },
            | Self::SectionMisplaced => {
                "`# Judgement` section is read on a type definition and on a function with a body; \
                 on any other item it declares nothing"
                    .to_owned()
            },
        }
    }
}

/// Report the defect in one item's `# Judgement` section, if it has one.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item, `span` is its own name
///   span, and `site` states what kind of item it is.
/// - ensures: reports the section's defect exactly when the item carries a `#
///   Judgement` section that declares nothing usable at that site.
/// - panics: none.
fn report_declaration_defect(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    span: Span,
    site: DeclarationSite<'_>,
)
{
    let Maybe::Present(declaration) = declaration_of(cx, def_id)
    else {
        return;
    };
    let Maybe::Present(defect) = site_defect(declaration, site)
    else {
        return;
    };
    span_lint(cx, MODE_DISPATCH_WILDCARD, span, defect.message());
}

/// Return the defect in a declaration read at `site`, if any.
///
/// # Specification
/// - requires: `declaration` was read from the item `site` describes.
/// - ensures: returns the section's own grammar defect where it has one, and
///   otherwise the mismatch between what the section declares and what the item
///   can carry: a direction off a type definition, an expected parameter off a
///   function, a name no parameter binds, or a section on an item holding
///   neither.
/// - provides: everything [`MODE_DISPATCH_WILDCARD`] reports away from a match
///   expression.
/// - provides: `declaration_site::Accepted::MatchesDeclarationSite` means the
///   grammar, item kind, and parameter bindings all admit the declaration.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates each site from each bullet
///   one pairing at a time: both bullets on the item that owns them, each on
///   the item that does not, a named parameter present and absent, and a
///   section on each item carrying no scrutinee — a free const, an associated
///   const, an associated type, and a required trait method — beside the
///   provided method and the associated function that do carry one.
/// - witness: `tests::ui`
fn site_defect(
    declaration: Declaration,
    site: DeclarationSite<'_>,
) -> Maybe<JudgementDefect, declaration_site::Accepted>
{
    if let Maybe::Present(defect) = declaration.defect {
        return Maybe::Present(defect);
    }
    match site {
        | DeclarationSite::TypeDefinition => {
            if declaration.expected.is_empty() {
                Maybe::Absent(declaration_site::Accepted::MatchesDeclarationSite)
            }
            else {
                Maybe::Present(JudgementDefect::ExpectedOffFunction)
            }
        },
        | DeclarationSite::Function(body) => {
            if declaration.direction.0 {
                return Maybe::Present(JudgementDefect::DirectionOffType);
            }
            let unbound = declaration.expected.into_iter().find(|name| {
                matches!(
                    parameter_binding(body, ParameterName::from(name.as_str())),
                    Maybe::Absent(_)
                )
            });
            match unbound {
                | Some(name) => Maybe::Present(JudgementDefect::ExpectedUnbound(name)),
                | None => Maybe::Absent(declaration_site::Accepted::MatchesDeclarationSite),
            }
        },
        | DeclarationSite::Elsewhere => Maybe::Present(JudgementDefect::SectionMisplaced),
    }
}

/// Read the `# Judgement` section attached to `def_id`, if it carries one.
///
/// # Specification
/// - ensures: returns the parsed declaration exactly when the item's rustdoc
///   carries a `# Judgement` heading.
/// - provides: `crate::rustdoc::section_lookup::Missing::HeadingAbsent` records
///   absence of that exact authored heading.
/// - panics: none.
fn declaration_of(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
) -> Maybe<Declaration, crate::rustdoc::section_lookup::Missing>
{
    let lines = indented_rustdoc_lines(cx, def_id);
    declaration(&lines)
}

/// Parse the `# Judgement` section out of one item's rustdoc lines.
///
/// # Specification
/// - requires: `lines` are the item's rustdoc lines with their indentation.
/// - ensures: returns `Absent` when no `# Judgement` heading is present.
///   Otherwise the direction is declared exactly when a `- direction:` bullet
///   is present, the expected names are the backticked values of the `-
///   expected:` bullets, and `defect` carries the first grammar failure — a
///   valueless direction, an expected value that is not one backticked name, or
///   a section stating neither bullet.
/// - provides: the declaration [`site_defect`] validates and the gate reads.
/// - provides: `crate::rustdoc::section_lookup::Missing::HeadingAbsent` means
///   no declaration section exists. Within a present declaration,
///   `declaration_grammar::Accepted::NoDefectObserved` means no processed
///   bullet or final empty-declaration check found a grammar defect.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a section declaring each bullet, one
///   declaring neither, a direction with an empty value, a wrapped direction
///   value, and an expected value that is prose rather than a name.
/// - witness: `judgement::tests::a_section_stating_neither_bullet_declares_nothing`
/// - witness: `judgement::tests::a_direction_bullet_states_what_the_direction_is`
/// - witness: `judgement::tests::an_expected_bullet_names_one_backticked_parameter`
/// - witness: `judgement::tests::a_wrapped_direction_value_stays_one_bullet`
fn declaration(lines: &[String]) -> Maybe<Declaration, crate::rustdoc::section_lookup::Missing>
{
    let bullets = match section_lines(lines, SectionHeading::from(HEADING)) {
        | Maybe::Present(bullets) => bullets,
        | Maybe::Absent(reason) => return Maybe::Absent(reason),
    };
    let mut direction = DirectionDeclared(false);
    let mut expected: Vec<String> = Vec::new();
    let mut defect = Maybe::Absent(declaration_grammar::Accepted::NoDefectObserved);
    for bullet in &bullets {
        if let Some(value) = bullet.strip_prefix(DIRECTION_BULLET) {
            direction = DirectionDeclared(true);
            if value.trim().is_empty() && matches!(defect, Maybe::Absent(_)) {
                defect = Maybe::Present(JudgementDefect::DirectionUnexplained);
            }
            continue;
        }
        let Some(value) = bullet.strip_prefix(EXPECTED_BULLET)
        else {
            continue;
        };
        let Maybe::Present(name) = backticked_name(RustdocLine::from(value))
        else {
            if matches!(defect, Maybe::Absent(_)) {
                defect = Maybe::Present(JudgementDefect::ExpectedMalformed);
            }
            continue;
        };
        expected.push(name.0.to_owned());
    }
    if !direction.0 && expected.is_empty() && matches!(defect, Maybe::Absent(_)) {
        defect = Maybe::Present(JudgementDefect::DeclaresNothing);
    }
    Maybe::Present(Declaration {
        direction,
        expected,
        defect,
    })
}

/// Return the one backticked name a bullet value states.
///
/// # Specification
/// - requires: `value` is a folded bullet's value, backticks and all.
/// - ensures: returns the name exactly when the value is one nonempty
///   backtick-delimited run holding neither a further backtick nor whitespace;
///   prose, a bare name, and two names on one bullet all yield nothing.
/// - provides: the exactness a parameter lookup depends on, since the name is
///   matched against the parameter's spelling verbatim.
/// - provides: `name_syntax::Missing` separates `OpeningBacktickAbsent`,
///   `ClosingBacktickAbsent`, `EmptyName`, `InteriorBacktick`, and `Whitespace`
///   according to the first unmet part of the exact-name grammar.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate an exact name, an unquoted name, an
///   empty pair of backticks, a name followed by prose, and two names on one
///   bullet.
/// - witness: `judgement::tests::an_expected_bullet_names_one_backticked_parameter`
fn backticked_name(value: RustdocLine<'_>) -> Maybe<RustdocLine<'_>, name_syntax::Missing>
{
    let Some(rest) = value.0.trim().strip_prefix('`')
    else {
        return Maybe::Absent(name_syntax::Missing::OpeningBacktickAbsent);
    };
    let Some(name) = rest.strip_suffix('`')
    else {
        return Maybe::Absent(name_syntax::Missing::ClosingBacktickAbsent);
    };
    if name.is_empty() {
        return Maybe::Absent(name_syntax::Missing::EmptyName);
    }
    if name.contains('`') {
        return Maybe::Absent(name_syntax::Missing::InteriorBacktick);
    }
    if name.contains(char::is_whitespace) {
        return Maybe::Absent(name_syntax::Missing::Whitespace);
    }
    Maybe::Present(RustdocLine::from(name))
}

/// Return the binding of the parameter `name` names, if the body has one.
///
/// # Specification
/// - requires: `body` is the body of the function the declaration sits on.
/// - ensures: returns the binding's HIR id exactly when some parameter is a
///   plain binding pattern spelled `name`. A destructuring parameter binds no
///   single name and is therefore not found.
/// - provides: the resolution behind both the unbound-name defect and the
///   expected plane's identity test.
/// - provides: `parameter_lookup::Missing::Unbound` means no plain parameter
///   binding matches the exact name; destructuring does not invent one.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates a named parameter, a name no
///   parameter binds, and a match on a parameter the declaration does not name.
/// - witness: `tests::ui`
fn parameter_binding(
    body: &Body<'_>,
    name: ParameterName<'_>,
) -> Maybe<HirId, parameter_lookup::Missing>
{
    for param in body.params {
        if let PatKind::Binding(_, hir_id, ident, None) = param.pat.kind
            && ident.as_str() == name.0
        {
            return Maybe::Present(hir_id);
        }
    }
    Maybe::Absent(parameter_lookup::Missing::Unbound)
}

/// Return which of the judgement's scrutinees a match dispatches on, if either.
///
/// # Specification
/// - requires: `scrutinee` is the scrutinee expression of a match written in
///   the source rather than produced by an expansion.
/// - ensures: reports [`ScrutineeRole::Direction`] when the scrutinee's type,
///   references peeled, is a crate-local type declaring itself the direction;
///   otherwise [`ScrutineeRole::ExpectedType`] when the expression mentions a
///   parameter its function declares expected; otherwise nothing.
/// - provides: the gate's trigger, and the role its diagnostic names.
/// - provides: `role_lookup::Missing::NoRecognizedRole` means neither
///   direction-type inspection nor expected-parameter tracking recognized a
///   role; it does not infer the absence of declarations outside those views.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates a match on the direction, a
///   match on an undeclared type, a match on a declared expected parameter, a
///   match on the same parameter beneath a call, and a match on another
///   parameter of the same function.
/// - witness: `tests::ui`
fn scrutinee_role<'tcx>(
    cx: &LateContext<'tcx>,
    scrutinee: &'tcx Expr<'tcx>,
) -> Maybe<ScrutineeRole, role_lookup::Missing>
{
    if scrutinee_is_direction(cx, scrutinee).0 {
        return Maybe::Present(ScrutineeRole::Direction);
    }
    if scrutinee_mentions_expected(cx, scrutinee).0 {
        return Maybe::Present(ScrutineeRole::ExpectedType);
    }
    Maybe::Absent(role_lookup::Missing::NoRecognizedRole)
}

/// Return whether a scrutinee's type declares itself the judgement's direction.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the scrutinee's type, its
///   references peeled, is a crate-local ADT whose own rustdoc carries a
///   defect-free `- direction:` declaration.
/// - panics: none.
fn scrutinee_is_direction<'tcx>(
    cx: &LateContext<'tcx>,
    scrutinee: &'tcx Expr<'tcx>,
) -> ScrutineeIsDirection
{
    let Some(scrutinee_ty) = cx.typeck_results().expr_ty_opt(scrutinee)
    else {
        return ScrutineeIsDirection(false);
    };
    let rustc_ty::Adt(adt, _) = *scrutinee_ty.peel_refs().kind()
    else {
        return ScrutineeIsDirection(false);
    };
    let Some(def_id) = adt.did().as_local()
    else {
        return ScrutineeIsDirection(false);
    };
    ScrutineeIsDirection(matches!(
        declaration_of(cx, def_id),
        Maybe::Present(declaration)
            if matches!(declaration.defect, Maybe::Absent(_)) && declaration.direction.0
    ))
}

/// Return whether a scrutinee mentions a declared expected parameter.
///
/// # Specification
/// - ensures: answers affirmatively exactly when some path in the scrutinee,
///   closure bodies included, resolves to a parameter its function declares
///   expected.
/// - panics: none.
fn scrutinee_mentions_expected<'tcx>(
    cx: &LateContext<'tcx>,
    scrutinee: &'tcx Expr<'tcx>,
) -> ScrutineeMentionsExpected
{
    let mut finder = ExpectedMention {
        cx,
        found: ScrutineeMentionsExpected(false),
    };
    finder.visit_expr(scrutinee);
    finder.found
}

/// Return whether `binding` is a parameter its function declares expected.
///
/// # Specification
/// - requires: `binding` is the HIR id a `Res::Local` resolved to.
/// - ensures: answers affirmatively exactly when the body owning the binding
///   carries a defect-free `# Judgement` declaration naming a parameter, and
///   that parameter is this very binding. A closure that captures the parameter
///   answers the same way, because the capture resolves to the parameter's own
///   binding.
/// - provides: the identity test behind the expected plane.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates the declared parameter, a
///   local of the same function that is not it, and a parameter of a function
///   declaring nothing.
/// - witness: `tests::ui`
fn declared_expected_parameter(
    cx: &LateContext<'_>,
    binding: HirId,
) -> DeclaredExpectedParameter
{
    let owner = cx.tcx.hir_enclosing_body_owner(binding);
    let Maybe::Present(declaration) = declaration_of(cx, owner)
    else {
        return DeclaredExpectedParameter(false);
    };
    if matches!(declaration.defect, Maybe::Present(_)) {
        return DeclaredExpectedParameter(false);
    }
    let Some(body) = cx.tcx.hir_maybe_body_owned_by(owner)
    else {
        return DeclaredExpectedParameter(false);
    };
    DeclaredExpectedParameter(declaration.expected.iter().any(|name| {
        parameter_binding(body, ParameterName::from(name.as_str())) == Maybe::Present(binding)
    }))
}

/// Return the spans of the fallback alternatives in one arm's pattern.
///
/// # Specification
/// - requires: `pattern` is an arm's whole pattern.
/// - ensures: returns the span of every top-level alternative that names no
///   case — a wildcard, or a binding with no subpattern beneath it. An
///   or-pattern is opened so that a fallback hidden among named alternatives is
///   found, and a binding *with* a subpattern names its case and is not
///   returned.
/// - provides: the denial set of [`MODE_DISPATCH_WILDCARD`] at a match.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates a wildcard arm, a
///   bare-binding arm, a wildcard inside an or-pattern, a binding carrying a
///   subpattern, and an exhaustive match naming every variant.
/// - witness: `tests::ui`
fn fallback_spans(pattern: &Pat<'_>) -> Vec<Span>
{
    let mut pending: VecDeque<&Pat<'_>> = VecDeque::new();
    pending.push_back(pattern);
    let mut spans: Vec<Span> = Vec::new();
    while let Some(pattern) = pending.pop_front() {
        match pattern.kind {
            | PatKind::Wild | PatKind::Binding(_, _, _, None) => spans.push(pattern.span),
            | PatKind::Or(alternatives) => pending.extend(alternatives.iter()),
            | _ => {},
        }
    }
    spans
}

/// HIR visitor answering whether an expression mentions a declared parameter.
struct ExpectedMention<'context, 'tcx>
{
    /// The late lint context, used to read the declaration of the body that
    /// owns each binding the expression resolves to.
    cx: &'context LateContext<'tcx>,
    /// Whether such a mention has been found.
    found: ScrutineeMentionsExpected,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names; the \
              implementation keeps descriptive ones"
)]
impl<'tcx> Visitor<'tcx> for ExpectedMention<'_, 'tcx>
{
    type MaybeTyCtxt = TyCtxt<'tcx>;
    type NestedFilter = nested_filter::OnlyBodies;

    /// Hand rustc's context to the walk.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Ask whether this expression is a declared parameter, then descend.
    ///
    /// The nested filter enters bodies, so a closure written inside the
    /// scrutinee is searched too: it captures the parameter by the parameter's
    /// own binding, and reading the expected type through a closure is reading
    /// it.
    ///
    /// # Specification
    /// - ensures: sets the found flag on the first path resolving to a declared
    ///   expected parameter and stops descending once it is set.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is the only supported traversal of
    ///   the borrowed HIR arena, and its `walk_expr`/`visit_expr` pair is
    ///   mutually recursive by construction.
    /// - measure: the height of the remaining HIR subtree below `expr`.
    /// - boundedness: HIR expression trees are finite and their depth is capped
    ///   by the parser's own nesting limit, which rejects deeper input before a
    ///   lint pass ever runs.
    /// - input recursion: none.
    fn visit_expr(
        &mut self,
        expr: &'tcx Expr<'_>,
    )
    {
        if self.found.0 {
            return;
        }
        if let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind
            && let Res::Local(binding) = path.res
            && declared_expected_parameter(self.cx, binding).0
        {
            self.found = ScrutineeMentionsExpected(true);
            return;
        }
        walk_expr(self, expr);
    }
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::backticked_name;
    use super::declaration;
    use crate::semantic::RustdocLine;

    /// Build a doc block the way a `///` comment reaches the pass: one leading
    /// space, and whatever further indentation the author wrote.
    ///
    /// # Specification
    /// trivial.
    fn block(lines: &[RustdocLine<'_>]) -> Vec<String>
    {
        lines.iter().map(|line| format!(" {}", line.0)).collect()
    }

    /// The heading line, spelled here rather than in a doc comment: this crate
    /// runs under its own gate, and a heading written in prose would be read as
    /// a declaration.
    ///
    /// # Specification
    /// trivial.
    fn heading() -> RustdocLine<'static>
    {
        RustdocLine("# Judgement")
    }

    #[test]
    fn a_section_stating_neither_bullet_declares_nothing()
    {
        let doc = block(&[
            heading(),
            RustdocLine("- mode: whichever the term asks for."),
        ]);
        let declared = match declaration(&doc) {
            | Maybe::Present(value) => value,
            | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
        };
        assert!(
            matches!(declared.defect, Maybe::Present(_)),
            "a section carrying neither bullet has declared nothing"
        );
        assert!(
            !declared.direction.0 && declared.expected.is_empty(),
            "and it marks no scrutinee either"
        );
        assert!(
            matches!(
                declaration(&block(&[RustdocLine("Summary only.")])),
                Maybe::Absent(_)
            ),
            "an item with no section at all is not a declaration"
        );
    }

    #[test]
    fn a_direction_bullet_states_what_the_direction_is()
    {
        let stated = block(&[heading(), RustdocLine("- direction: the two modes.")]);
        let declared = match declaration(&stated) {
            | Maybe::Present(value) => value,
            | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
        };
        assert!(
            declared.direction.0 && matches!(declared.defect, Maybe::Absent(_)),
            "a direction with a value declares the type"
        );

        let bare = block(&[heading(), RustdocLine("- direction:")]);
        let declared = match declaration(&bare) {
            | Maybe::Present(value) => value,
            | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
        };
        assert!(
            matches!(declared.defect, Maybe::Present(_)),
            "and a valueless bullet is a marker rather than a claim"
        );
    }

    #[test]
    fn an_expected_bullet_names_one_backticked_parameter()
    {
        let named = block(&[heading(), RustdocLine("- expected: `expected`")]);
        let declared = match declaration(&named) {
            | Maybe::Present(value) => value,
            | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
        };
        assert_eq!(
            declared.expected.first().map(String::as_str),
            Some("expected"),
            "the backticked name is the parameter the gate looks for"
        );
        assert!(
            matches!(declared.defect, Maybe::Absent(_)),
            "and one exact name is the whole of the grammar"
        );

        for value in [
            "- expected: expected",
            "- expected: ``",
            "- expected: `expected` and the other one",
            "- expected: `first` `second`",
        ] {
            let doc = block(&[heading(), RustdocLine(value)]);
            let declared = match declaration(&doc) {
                | Maybe::Present(value) => value,
                | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
            };
            assert!(
                matches!(declared.defect, Maybe::Present(_)),
                "and nothing else names a parameter the lookup can resolve: {value}"
            );
        }

        assert!(
            matches!(
                backticked_name(RustdocLine(" `expected`")),
                Maybe::Present(_)
            ),
            "the value is read with its surrounding space trimmed"
        );
    }

    #[test]
    fn a_wrapped_direction_value_stays_one_bullet()
    {
        let doc = block(&[
            heading(),
            RustdocLine("- direction: the value continues"),
            RustdocLine("  onto a second line."),
            RustdocLine("- expected: `expected`"),
        ]);
        let declared = match declaration(&doc) {
            | Maybe::Present(value) => value,
            | Maybe::Absent(reason) => panic!("{}: {reason:?}", "the section is present"),
        };
        assert!(
            declared.direction.0 && matches!(declared.defect, Maybe::Absent(_)),
            "a continuation line folds upward rather than opening a bullet of its own"
        );
        assert_eq!(
            declared.expected.len(),
            1_usize,
            "and the bullet after it is still read"
        );
    }
}
