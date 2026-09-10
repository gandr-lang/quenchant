//! Explicit judgment scrutinees must retain explicit case analysis.
//!
//! `# Judgement` attaches a `direction` declaration to the mode type or an
//! `expected` declaration to the function parameter carrying the checked type.
//! Matching a declared scrutinee through a wildcard or bare binding hides the
//! case that supplied the result; a guard does not name that case.
//!
//! Expected-type tracking is parameter-specific. The core type vocabulary may
//! be owned by another crate, and unrelated matches over that same type are not
//! automatically judgment dispatch. Keeping the declaration on the local
//! parameter preserves this distinction as code is moved.
//!
//! A malformed, empty, or misplaced declaration must not read as an opted-in
//! check that governs nothing. The reader therefore visits items that cannot
//! legitimately carry a scrutinee as well as those that can, including required
//! trait methods and foreign declarations.
//!
//! This rule constrains dispatch shape. It neither invents missing judgment
//! rules nor proves the soundness or completeness of the declared system.

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
    /// Declared judgment scrutinees require named match cases: wildcard and bare-binding alternatives are denied for direction values and expected-type parameters.
    ///
    /// ### Why is this bad?
    ///
    /// Judgment rules distinguish the cases they admit. A fallback supplies an answer for unnamed cases, including ones with no authored rule. Requiring explicit alternatives keeps that decision visible in the program rather than relying on a prose case count.
    ///
    /// A guard does not name a case; guarded wildcard and bare-binding alternatives remain fallbacks.
    ///
    /// ### How a crate declares its judgement
    ///
    /// A `# Judgement` section assigns roles explicitly. A type's `- direction:` bullet explains its mode values; each function `- expected:` bullet names one expected-type parameter in backticks.
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
    /// Empty declarations, valueless bullets, unbound expected names, and sections attached to items without a scrutinee are rejected. Authored opt-in must identify a role the gate can inspect.
    ///
    /// ### Scope
    ///
    /// Type definitions and functions with bodies can own these roles. Other sites, including associated constants and types, required methods, and foreign functions, are inspected and diagnosed as misplaced rather than silently ignored.
    ///
    /// Direction identity and its declaration are resolved within the current crate. Downstream matches on an imported direction type are outside this gate's scope; the policy governs the declaring crate's own judgment implementation.
    ///
    /// Expansion-produced matches are excluded. This permits `matches!` to ask a boolean question without treating its generated fallback as authored dispatch, but also permits a local macro to hide a fallback. Narrowing that exemption requires distinguishing expansion ownership and identifying a source span the caller can repair.
    ///
    /// ### Blind spots
    ///
    /// Conditional forms such as `if let`, `let .. else`, and `while let` escape match-arm inspection. The accepted `if_let_fallback` case in `ui/mode_dispatch.rs`, with no corresponding `ui/mode_dispatch.stderr` diagnostic, records that limit.
    ///
    /// Direction recognition applies to the whole scrutinee after peeling references. Pairing a direction with a term changes that type and escapes recognition; expected-parameter tracking does not compensate for a direction-only tuple. The `tupled_scrutinee` case and its absent diagnostic in the same fixture record this boundary.
    ///
    /// Separate judgment entry points with direct role-bearing parameters keep dispatch within the observable fragment. Declaration metadata alone cannot extend that fragment.
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
    /// Named case analysis:
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

/// Compiler-side enforcement of explicit cases over declared judgment roles.
pub struct WorkflowJudgement;

impl<'tcx> LateLintPass<'tcx> for WorkflowJudgement
{
    /// Non-function declarations are checked where their item kind is known.
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
        // Function declarations are deferred to `check_fn`, where parameter bindings
        // are available, and are read only once.
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

    /// Function-body context makes declared expected names resolvable.
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

    /// Implementation members are checked at the declaration that owns their
    /// role.
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
        // Body-bearing methods reach `check_fn`; other implementation members have no
        // scrutinee to bind.
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

    /// Trait sites distinguish a provided body from a body-free declaration.
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
        // Provided methods reach `check_fn`. Required methods have no body-local
        // binding to inspect; an implementation owns different parameters and must
        // carry its own declaration.
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

    /// Foreign declarations cannot supply a body-local judgment scrutinee.
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

    /// Recognized judgment roles make unnamed alternatives reportable.
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

/// Exact section label used to discover authored judgment roles.
const HEADING: &str = "# Judgement";

/// Role label assigning mode meaning to a type declaration.
const DIRECTION_BULLET: &str = "- direction:";

/// Role label associating a function parameter with the expected type.
const EXPECTED_BULLET: &str = "- expected:";

/// Repair guidance requires explicit cases instead of an unnamed answer.
const FALLBACK_HELP: &str = concat!(
    "name every case in an arm of its own; a term the judgement has no rule for is a refusal, and ",
    "a fallback arm answers for it instead",
);

/// The recognized judgment role that makes a match subject to the rule.
enum ScrutineeRole
{
    /// Whole-scrutinee type identity recognizes the declared direction.
    Direction,
    /// Parameter provenance recognizes expected-type data in the scrutinee.
    ExpectedType,
}

impl ScrutineeRole
{
    /// Diagnostic wording identifies the role whose cases were hidden.
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

    /// The source note identifies the scrutinee that activated the rule.
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

/// Declaration kind constrains which judgment roles can be attached.
#[derive(Clone, Copy)]
enum DeclarationSite<'body>
{
    /// Values of this declaration can represent judgment direction.
    TypeDefinition,
    /// Body-local parameter bindings can represent expected-type inputs.
    Function(&'body Body<'body>),
    /// This site supplies neither a direction type nor inspectable parameters.
    Elsewhere,
}

/// Authored role declarations retain their names and first grammar defect.
struct Declaration
{
    /// Type-level direction opt-in remains separate from expected-name
    /// declarations.
    direction: DirectionDeclared,
    /// Authored expected names retain source order for binding validation.
    expected: Vec<String>,
    /// First-observed grammar failure takes precedence over subsequent defects.
    defect: Maybe<JudgementDefect, declaration_grammar::Accepted>,
}

/// Grammar, placement, or binding defects that prevent a usable judgment
/// declaration.
enum JudgementDefect
{
    /// Neither role-bearing bullet appears in the section.
    DeclaresNothing,
    /// Direction opt-in supplies no explanation of the mode.
    DirectionUnexplained,
    /// Expected-type metadata supplies no single quoted parameter name.
    ExpectedMalformed,
    /// The declared name has no matching parameter binding.
    ExpectedUnbound(String),
    /// A direction role is attached outside a type declaration.
    DirectionOffType,
    /// Expected-parameter metadata is attached outside a function.
    ExpectedOffFunction,
    /// The declaration site cannot carry either recognized role.
    SectionMisplaced,
}

impl JudgementDefect
{
    /// Each declaration defect identifies a different repair obligation.
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

/// An authored section is validated against the kind of item carrying it.
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

/// Grammar validity precedes role placement and parameter-binding checks.
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

/// Compiler-owned documentation supplies this item's local role declaration.
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

/// Section interpretation retains authored role names and grammar-failure
/// precedence.
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

/// Exact quoted names separate parameter references from descriptive prose.
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

/// Exact parameter spelling resolves only to a plain body-local binding.
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

/// Direction recognition precedes expected-parameter tracking for a match
/// scrutinee.
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

/// Local type identity connects the scrutinee to a valid direction declaration.
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

/// Expected-role recognition follows parameter references within the
/// expression.
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

/// A parameter's owning body determines whether its name was declared expected.
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

/// Unnamed pattern alternatives retain their own reportable source spans.
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

/// Expression traversal searches for references to declared expected
/// parameters.
struct ExpectedMention<'context, 'tcx>
{
    /// Compiler context connects each binding to the declaration of its owning
    /// body.
    cx: &'context LateContext<'tcx>,
    /// A recognized parameter reference completes the existence query.
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

    /// Nested-body traversal uses the same declaration-resolution context.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Parameter references remain detectable beneath enclosing expression
    /// forms.
    ///
    /// Body descent includes closures within the scrutinee. A captured expected
    /// parameter retains its binding identity, so reading through a closure
    /// remains visible to this search.
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

    /// Fixture fragments retain doc-comment prefix spacing and authored
    /// indentation.
    ///
    /// # Specification
    /// trivial.
    fn block(lines: &[RustdocLine<'_>]) -> Vec<String>
    {
        lines.iter().map(|line| format!(" {}", line.0)).collect()
    }

    /// The heading is fixture data, kept outside rustdoc so self-hosting does
    /// not interpret this helper as a judgment declaration.
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
