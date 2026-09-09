//! Reusable Dylint gates for Rust workflow boundaries.
//!
//! Seven rules ship here. Four are the type and call planes:
//! `#[repr(transparent)]` on single-field wrappers, semantic-wrapper
//! (non-primitive) function and method signatures, the outright ban on
//! recursive call cycles, and its type-plane closure — the ban on ADTs that own
//! their way back to themselves.
//!
//! Specification attributes use `anodized`. The former `specifications`
//! adoption lints guarded competing predicate attributes, bypassable early
//! exits, and injected predicate documentation. Those premises disappeared with
//! the pinned `#[spec]` expansion; their registrations and analysis were
//! removed.
//!
//! Three read rustdoc off the items the crate authors. `specification_present`
//! requires a `# Specification` block on every function and method, and denies
//! a block whose `trivial` marker sits beside a clause; an item counts as the
//! crate's own only where both its name and its declaration are text the author
//! wrote, so a method a macro from another crate generates under the author's
//! identifier is exempt, and the reversal is a consumer whose generated
//! siblings warrant documentation. The `# Adequacy` block
//! is shape-checked in place, its companion resolution gate (G0) running beside
//! the lint crate in `quenchant-gates`; the `# Judgement` block marks the
//! checking judgement's own scrutinees, and a match over one of them is denied
//! a fallback arm.
//!
//! The lint library is a `cdylib` built under a pinned nightly because a lint
//! pass links rustc's internal libraries; see the crate `README.md` for the pin
//! and its rationale.

#![feature(rustc_private)]
#![expect(
    unstable_features,
    reason = "rustc_private is the lint driver's substrate"
)]

extern crate alloc;

extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

mod adequacy;

mod callgraph;

mod graph;

mod judgement;

mod ownership;

mod rustdoc;

mod semantic;

mod signature;

mod specification;

#[cfg(test)]
mod specification_tests;

mod termination;

use std::collections::HashMap;

use clippy_utils::diagnostics::span_lint;
use clippy_utils::diagnostics::span_lint_hir;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use clippy_utils::trait_ref_of_method;
use rustc_hir::Body;
use rustc_hir::CRATE_HIR_ID;
use rustc_hir::FnDecl;
use rustc_hir::ForeignItem;
use rustc_hir::ForeignItemKind;
use rustc_hir::HirId;
use rustc_hir::Item;
use rustc_hir::ItemKind;
use rustc_hir::OwnerId;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::attrs::ReprAttr;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::find_attr;
use rustc_hir::intravisit::FnKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::Level;
use rustc_lint::LintStore;
use rustc_middle::lint::LintLevelSource;
use rustc_session::Session;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::adequacy::ADEQUACY_BLOCK_GRAMMAR;
use crate::adequacy::WorkflowAdequacy;
use crate::callgraph::CallEdge;
use crate::callgraph::FunctionNode;
use crate::callgraph::local_call_edges;
use crate::callgraph::parameter_binding_ids;
use crate::callgraph::recursive_sccs;
use crate::judgement::MODE_DISPATCH_WILDCARD;
use crate::judgement::WorkflowJudgement;
use crate::ownership::AdtNode;
use crate::ownership::NonOwningAllowList;
use crate::ownership::non_owning_allow_list;
use crate::ownership::owning_cycles;
use crate::semantic::DiagnosticText;
use crate::semantic::ExpectationOnItem;
use crate::semantic::NonLocalTraitImpl;
use crate::semantic::TransparentReprDeclared;
use crate::signature::check_fn_decl;
use crate::specification::SPECIFICATION_PRESENT;
use crate::specification::WorkflowSpecification;
use crate::termination::termination_defect;

dylint_linting::dylint_library!();

declare_lint! {
    /// ### What it does
    ///
    /// Requires every single-field named or tuple struct to declare
    /// `#[repr(transparent)]`.
    ///
    /// ### Why is this bad?
    ///
    /// The policy uses single-field structs as semantic domain wrappers. Making
    /// their transparent representation explicit preserves the intended
    /// ABI/layout specification while the wrapper stays nominal at the type
    /// boundary.
    ///
    /// ### Example
    ///
    /// ```rust
    /// struct UserId(u64);
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// #[repr(transparent)]
    /// struct UserId(u64);
    /// ```
    pub SINGLE_FIELD_STRUCT_NEEDS_TRANSPARENT_REPR,
    Deny,
    "single-field structs must declare #[repr(transparent)]"
}

declare_lint! {
    /// ### What it does
    ///
    /// Rejects function and method signatures that expose Rust primitives in
    /// workspace-owned APIs, including primitives below structural type layers,
    /// selected transparent containers, and type aliases.
    ///
    /// ### Why is this bad?
    ///
    /// Bare primitives erase semantic roles at crate boundaries. Nominal domain
    /// wrappers keep distinct meanings distinct for humans, tools, and agents.
    /// This lint establishes only that the signature reaches a local nominal
    /// transparent boundary. The wrapper's field visibility, conversion traits,
    /// and documentation remain the responsibility of Clippy and review.
    ///
    /// ### Example
    ///
    /// ```rust
    /// fn parse(offset: usize) -> Option<bool> { Some(true) }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// #[repr(transparent)]
    /// struct ByteOffset(usize);
    ///
    /// #[repr(transparent)]
    /// struct ParseSucceeded(bool);
    ///
    /// fn parse(offset: ByteOffset) -> Option<ParseSucceeded> { Some(ParseSucceeded(true)) }
    /// ```
    pub PRIMITIVE_SIGNATURE,
    Deny,
    "function signatures must use semantic wrappers instead of Rust primitives"
}

declare_lint! {
    /// ### What it does
    ///
    /// Denies every function that participates in a crate-local recursive call
    /// cycle.
    ///
    /// ### Why is this bad?
    ///
    /// Rust guarantees no tail-call optimization, so recursion whose depth
    /// scales with input is a latent stack overflow on real data. The policy
    /// requires the explicit worklist, heap frame stack, or iterative loop instead.
    ///
    /// ### The exception
    ///
    /// An owner-approved exception is `#[expect(recursion_forbidden, reason =
    /// "...")]` on the item itself, and the item must also carry a complete
    /// `# Termination` rustdoc section with the fixed bullet grammar
    /// `- reason:`, `- measure:`, `- boundedness:`, `- input recursion:`.
    /// Lint levels are inherited, but approval is not: an expectation on an
    /// enclosing module or on the crate root approves nothing inside it.
    /// A claim of `- input recursion: none.` is rejected when a call inside the
    /// cycle passes data derived from the function's own parameters; prose
    /// written after the claim does not weaken it.
    ///
    /// ### Blind spots
    ///
    /// This is a call-plane gate over crate-local HIR call edges, and it is a
    /// floor rather than a proof. A cycle whose edge carries no such edge is
    /// not denied: derived-trait recursion routes through non-local `std`
    /// generics, compiler-generated drop glue has no HIR function, and a callee
    /// reached through a *function pointer* has had its definition id erased by
    /// the coercion. A function *item* held in a binding keeps its `FnDef` type
    /// and is resolved, and a call written inside a closure counts as a call by
    /// the function that builds the closure.
    ///
    /// ### Example
    ///
    /// ```rust
    /// fn depth(node: Node) -> Depth { depth(node.child()) }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// fn depth(node: Node) -> Depth {
    ///     let mut pending = vec![node];
    ///     let mut depth = Depth::ZERO;
    ///     while let Some(node) = pending.pop() {
    ///         depth = depth.next();
    ///         pending.extend(node.children());
    ///     }
    ///     depth
    /// }
    /// ```
    pub RECURSION_FORBIDDEN,
    Deny,
    "recursive call cycles are forbidden; write the iterative form"
}

declare_lint! {
    /// ### What it does
    ///
    /// Denies every locally defined ADT that owns its way back to itself,
    /// directly or through other types.
    ///
    /// ### Why is this bad?
    ///
    /// Pointer-linked recursive data defeats destruction and duplication
    /// totality at once: drop glue and derived `Clone` both recurse over the
    /// pointee, with a depth that scales with the data. Neither is a HIR
    /// function, so the call-plane gate cannot see either. The sanctioned
    /// recursive data shape is flat and id-addressed — children are indices
    /// into an arena or interning table.
    ///
    /// ### What forms an edge
    ///
    /// The question is asked of the field's *type*, not of its spelling: a
    /// `Box` whose pointee returns only through an arena index closes no cycle,
    /// and a `Vec<Self>` closes one without naming a pointer at all. Array,
    /// slice and tuple components are owned; the type arguments of an ADT are
    /// owned at every parameter that reaches an owned field position, which for
    /// an ADT outside the crate means every parameter. References, raw
    /// pointers, function pointers, closures and trait objects own nothing, and
    /// `PhantomData` and `Weak` are non-owning by name.
    ///
    /// ### The allow-list
    ///
    /// A generic from another crate that owns none of its arguments is declared
    /// in the workspace's `dylint.toml`, and each entry states the evidence:
    ///
    /// ```toml
    /// [quenchant-dylints]
    /// non_owning_generics = [
    ///   { path = "std::ptr::NonNull", justification = "holds a raw pointer; its drop never touches the pointee" },
    /// ]
    /// ```
    ///
    /// An entry stating no justification is refused, so the type it names stays
    /// conservatively owning.
    ///
    /// ### Blind spots
    ///
    /// A cycle closed through a trait object, a closure capture, or a raw
    /// pointer is not denied: none of the three names the type it reaches.
    ///
    /// ### Example
    ///
    /// ```rust
    /// enum Expr {
    ///     Lit(Literal),
    ///     Add(Box<Expr>, Box<Expr>),
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// #[repr(transparent)]
    /// struct ExprId(u32);
    ///
    /// enum Expr {
    ///     Lit(Literal),
    ///     Add(ExprId, ExprId),
    /// }
    ///
    /// struct ExprArena {
    ///     nodes: Vec<Expr>,
    /// }
    /// ```
    pub RECURSIVE_OWNED_POINTER,
    Deny,
    "types must not own their way back to themselves; use the flat id-addressed form"
}

impl_lint_pass!(WorkflowBoundaries => [
    SINGLE_FIELD_STRUCT_NEEDS_TRANSPARENT_REPR,
    PRIMITIVE_SIGNATURE,
    RECURSION_FORBIDDEN,
    RECURSIVE_OWNED_POINTER,
]);

/// Register the workflow Dylint passes.
///
/// # Specification
/// - requires: the driver calls this once per compilation, before any pass
///   runs.
/// - ensures: reads the workspace configuration, registers every lint this
///   library declares, and registers one late pass per lint group.
/// - provides: the entry point dylint's driver resolves by symbol name.
/// - panics: none.
#[expect(
    clippy::no_mangle_with_rust_abi,
    reason = "dylint's driver loads `register_lints` by exact symbol name and passes rustc-internal \
              types (`Session`, `LintStore`), so a C ABI is impossible by design"
)]
#[unsafe(no_mangle)]
pub fn register_lints(
    sess: &Session,
    lint_store: &mut LintStore,
)
{
    dylint_linting::init_config(sess);
    let allow_list = non_owning_allow_list();
    lint_store.register_lints(&[
        SINGLE_FIELD_STRUCT_NEEDS_TRANSPARENT_REPR,
        PRIMITIVE_SIGNATURE,
        RECURSION_FORBIDDEN,
        RECURSIVE_OWNED_POINTER,
        ADEQUACY_BLOCK_GRAMMAR,
        MODE_DISPATCH_WILDCARD,
        SPECIFICATION_PRESENT,
    ]);
    lint_store.register_late_pass(Box::new(move |_| {
        Box::new(WorkflowBoundaries::new(allow_list.clone()))
    }));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowAdequacy)));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowJudgement)));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowSpecification)));
}

/// The diagnostic emitted at every member of a recursive call cycle.
const RECURSION_MESSAGE: &str = concat!(
    "function participates in a recursive call cycle; recursion is forbidden — write the ",
    "iterative form (worklist, explicit frame stack), or take the owner-approved exception ",
    "(`#[expect(recursion_forbidden, reason = \"...\")]` plus a complete `# Termination` section)",
);

/// The diagnostic emitted when the exception was inherited, not written here.
const INHERITED_EXPECT_MESSAGE: &str = concat!(
    "recursion exception must be attached to this item: an ",
    "`#[expect(recursion_forbidden, ..)]` on an enclosing module or on the crate root approves ",
    "nothing inside it",
);

/// The diagnostic emitted when an exception attribute states no reason.
const MISSING_REASON_MESSAGE: &str = concat!(
    "recursion exception must state `reason = \"...\"` on its ",
    "`#[expect(recursion_forbidden, ..)]` attribute",
);

/// Whether an item claims the owner-approved recursion exception, and how well.
enum ExceptionState
{
    /// Neither the item nor anything enclosing it expects the lint.
    Absent,
    /// The item carries the expectation with a `reason`.
    Approved,
    /// The item carries the expectation without a `reason`.
    Unreasoned,
    /// The expectation was inherited from an enclosing module or the crate
    /// root rather than written on the item.
    Inherited,
}

/// The diagnostic emitted at every ADT on an ownership cycle.
const OWNING_CYCLE_MESSAGE: &str = concat!(
    "type owns its way back to itself; pointer-linked recursive data is forbidden — the drop glue ",
    "and the derived traversals both recurse over the pointee, and neither is a function the ",
    "call-plane gate can see",
);

/// The help attached to every ownership-cycle diagnostic.
const OWNING_CYCLE_HELP: &str = concat!(
    "use the flat id-addressed form: a child is an index into an arena or interning table, never a ",
    "node linked through `Box`, `Rc`, `Arc`, `Vec`, or any other owning carrier. Outside a ",
    "recursive position `Arc` is the one permitted shared-ownership pointer, for whole values at ",
    "boundaries",
);

/// Late lint pass implementing the workflow's Rust boundary gates.
struct WorkflowBoundaries
{
    /// Crate-local function metadata by definition id, filled by `check_fn`.
    functions: HashMap<LocalDefId, FunctionNode>,
    /// Crate-local callsites by caller definition id, filled by `check_fn`.
    edges: HashMap<LocalDefId, Vec<CallEdge>>,
    /// Crate-local ADT metadata by definition id, filled by `check_item`.
    adts: HashMap<LocalDefId, AdtNode>,
    /// External generics the workspace configuration declares non-owning.
    allow_list: NonOwningAllowList,
}

impl WorkflowBoundaries
{
    /// Build the pass around the configured non-owning allow-list.
    ///
    /// # Specification
    /// trivial.
    fn new(allow_list: NonOwningAllowList) -> Self
    {
        Self {
            functions: HashMap::new(),
            edges: HashMap::new(),
            adts: HashMap::new(),
            allow_list,
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for WorkflowBoundaries
{
    /// Deny an untransparent single-field struct, and record every ADT.
    ///
    /// # Specification
    /// - ensures: reports a struct of exactly one field that declares no
    ///   transparent representation, and records every struct, enum and union
    ///   for the ownership graph built at crate end.
    /// - panics: none.
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        if let ItemKind::Struct(_, _, ref variant_data) = item.kind
            && variant_data.fields().len() == 1_usize
            && !has_transparent_repr(cx, item).0
        {
            span_lint(
                cx,
                SINGLE_FIELD_STRUCT_NEEDS_TRANSPARENT_REPR,
                item.kind.ident().map_or(item.span, |ident| ident.span),
                "single-field struct must declare #[repr(transparent)]",
            );
        }

        if matches!(
            item.kind,
            ItemKind::Struct(..) | ItemKind::Enum(..) | ItemKind::Union(..)
        ) {
            let def_id = item.owner_id.def_id;
            self.adts.insert(def_id, AdtNode {
                hir_id: item.hir_id(),
                span: item.kind.ident().map_or(item.span, |ident| ident.span),
                path: cx.tcx.def_path_str(def_id.to_def_id()),
            });
        }
    }

    /// Check a signature, and record the function and its callsites.
    ///
    /// # Specification
    /// - ensures: checks the declaration unless the function implements a
    ///   non-local trait, and records the function's node and outgoing call
    ///   edges for the recursion search at crate end; a closure is neither
    ///   checked nor recorded.
    /// - panics: none.
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        fn_kind: FnKind<'tcx>,
        fn_decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        span: Span,
        def_id: LocalDefId,
    )
    {
        if matches!(fn_kind, FnKind::Closure) {
            return;
        }

        if !implements_non_local_trait(cx, def_id).0 {
            let semantic_sig = cx
                .tcx
                .fn_sig(def_id)
                .instantiate_identity()
                .skip_norm_wip()
                .skip_binder();
            check_fn_decl(cx, fn_decl, semantic_sig);
        }

        self.functions.insert(def_id, FunctionNode {
            span,
            path: cx.tcx.def_path_str(def_id.to_def_id()),
            body_hir_id: body.value.hir_id,
            input_bindings: parameter_binding_ids(body),
        });
        self.edges.insert(def_id, local_call_edges(cx, body.value));
    }

    /// Check a required trait method's signature.
    ///
    /// # Specification
    /// - ensures: checks the declaration of a method declared without a body; a
    ///   provided one reaches `check_fn` instead.
    /// - panics: none.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx TraitItem<'tcx>,
    )
    {
        if let TraitItemKind::Fn(fn_sig, TraitFn::Required(_)) = trait_item.kind {
            let semantic_sig = cx
                .tcx
                .fn_sig(trait_item.owner_id.def_id)
                .instantiate_identity()
                .skip_norm_wip()
                .skip_binder();
            check_fn_decl(cx, fn_sig.decl, semantic_sig);
        }
    }

    /// Check a foreign function's signature.
    ///
    /// # Specification
    /// - ensures: checks the declaration of a foreign function, and ignores
    ///   every other foreign item kind.
    /// - panics: none.
    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    )
    {
        if let ForeignItemKind::Fn(fn_sig, ..) = item.kind {
            let semantic_sig = cx
                .tcx
                .fn_sig(item.owner_id.def_id)
                .instantiate_identity()
                .skip_norm_wip()
                .skip_binder();
            check_fn_decl(cx, fn_sig.decl, semantic_sig);
        }
    }

    /// Report every whole-crate denial once the walk has finished.
    ///
    /// # Specification
    /// - requires: the item walk has recorded every crate-local function, call
    ///   edge and ADT.
    /// - ensures: reports each refused allow-list entry, each ADT on an
    ///   ownership cycle with the cycle rendered, and each member of a
    ///   recursive call component, following an approved exception with its
    ///   termination defect where the specification does not hold.
    /// - panics: none.
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        for defect in self.allow_list.defects() {
            span_lint(
                cx,
                RECURSIVE_OWNED_POINTER,
                cx.tcx.def_span(CRATE_HIR_ID.owner.def_id),
                defect.clone(),
            );
        }
        for denial in owning_cycles(cx, &self.adts, &self.allow_list) {
            let Some(node) = self.adts.get(&denial.adt)
            else {
                continue;
            };
            let cycle = denial.cycle;
            span_lint_hir_and_then(
                cx,
                RECURSIVE_OWNED_POINTER,
                node.hir_id,
                node.span,
                OWNING_CYCLE_MESSAGE,
                |diag| {
                    diag.note(format!("ownership cycle: {cycle}"));
                    diag.help(OWNING_CYCLE_HELP);
                },
            );
        }

        let components = recursive_sccs(&self.functions, &self.edges);
        let mut denials: Vec<(LocalDefId, &[LocalDefId])> = Vec::new();
        for component in &components {
            for def_id in component {
                denials.push((*def_id, component.as_slice()));
            }
        }
        denials
            .sort_by_key(|&(def_id, _)| self.functions.get(&def_id).map(|node| node.path.clone()));

        for (def_id, component) in denials {
            let Some(node) = self.functions.get(&def_id)
            else {
                continue;
            };
            let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
            let report_span = reportable_span(cx, def_id, node.span);

            // Emitted unconditionally: with no exception attribute this is the
            // denial, and with one it is what fulfils the expectation so rustc
            // suppresses it instead of reporting an unfulfilled expectation.
            span_lint_hir(
                cx,
                RECURSION_FORBIDDEN,
                hir_id,
                report_span,
                RECURSION_MESSAGE,
            );

            let message = match exception_state(cx, hir_id) {
                | ExceptionState::Absent => continue,
                | ExceptionState::Inherited => DiagnosticText::from(INHERITED_EXPECT_MESSAGE),
                | ExceptionState::Unreasoned => DiagnosticText::from(MISSING_REASON_MESSAGE),
                | ExceptionState::Approved => {
                    match termination_defect(cx, def_id, component, &self.functions, &self.edges) {
                        | quenchant_shape::shape::Maybe::Present(defect) => defect.message(),
                        | quenchant_shape::shape::Maybe::Absent(_) => continue,
                    }
                },
            };

            // Reported at the crate root so the item's own expectation cannot
            // swallow the report that its exception is unjustified.
            span_lint_hir(
                cx,
                RECURSION_FORBIDDEN,
                CRATE_HIR_ID,
                report_span,
                message.0,
            );
        }
    }
}

/// Return a span for `def_id` that a diagnostic can actually be seen at.
///
/// # Specification
/// - requires: `def_id` is a crate-local function, and `item_span` is its
///   whole-item span.
/// - ensures: returns `item_span` unless that span sits inside a macro rustc
///   treats as external, in which case it returns the function's own name span,
///   falling back to `item_span` if the name has none.
/// - provides: the reporting span of [`RECURSION_FORBIDDEN`], the one gate here
///   that reports at a whole item rather than at a piece of the author's own
///   syntax.
/// - panics: none.
///
/// # Why the fallback exists
///
/// rustc discards any lint whose primary span sits in an external macro, and
/// `SyntaxContext::in_external_macro` counts **every** attribute macro as
/// external — local crate or not. An attribute macro that rewrites a function
/// body therefore owns the item span, so a diagnostic reported there is dropped
/// silently. The function's own name is a pass-through token that keeps the
/// author's context, so a diagnostic anchored there survives `#[spec]`.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates an ordinary recursive function
///   (reported at the whole item) from a specification-expanded one (reported
///   at its name), both in a denied and in an owner-approved position.
/// - witness: `tests::ui`
/// - witness: `tests::ui_specifications`
fn reportable_span(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    item_span: Span,
) -> Span
{
    if !item_span.in_external_macro(cx.tcx.sess.source_map()) {
        return item_span;
    }
    cx.tcx.def_ident_span(def_id).unwrap_or(item_span)
}

/// Return whether `item` already declares `#[repr(transparent)]`.
///
/// # Specification
/// - ensures: answers affirmatively exactly when one of the item's own `repr`
///   attributes declares the transparent representation.
/// - panics: none.
fn has_transparent_repr(
    cx: &LateContext<'_>,
    item: &Item<'_>,
) -> TransparentReprDeclared
{
    let attrs = cx.tcx.hir_attrs(item.hir_id());
    TransparentReprDeclared(
        find_attr!(attrs, Repr { reprs, .. } if reprs.iter().any(|&(repr, _)| repr == ReprAttr::ReprTransparent)),
    )
}

/// Return whether `def_id` is a method implementing a non-local trait.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the function implements a
///   trait whose definition is outside this crate.
/// - provides: the one signature exception the primitive rule admits.
/// - panics: none.
fn implements_non_local_trait(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
) -> NonLocalTraitImpl
{
    NonLocalTraitImpl(
        trait_ref_of_method(cx, OwnerId { def_id }).is_some_and(|trait_ref| {
            trait_ref
                .trait_def_id()
                .is_some_and(|trait_def_id| !trait_def_id.is_local())
        }),
    )
}

/// Return whether the item at `hir_id` claims the recursion exception.
///
/// # Specification
/// - requires: `hir_id` identifies a crate-local function item.
/// - ensures: reports [`ExceptionState::Approved`] exactly when an `expect`
///   attribute carrying a `reason` sits on the item itself. Lint levels are
///   inherited, so the nearest level for [`RECURSION_FORBIDDEN`] may come from
///   an enclosing module or the crate root; that is reported as
///   [`ExceptionState::Inherited`] rather than treated as approval.
/// - provides: the gate on whether the `# Termination` specification is checked
///   at all.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates an item-level expectation, an
///   expectation on the enclosing module, an expectation without a reason, and
///   no expectation at all.
/// - witness: `tests::ui`
fn exception_state(
    cx: &LateContext<'_>,
    hir_id: HirId,
) -> ExceptionState
{
    let level_and_source = cx.tcx.lint_level_spec_at_node(RECURSION_FORBIDDEN, hir_id);
    if !matches!(level_and_source.level(), Level::Expect) {
        return ExceptionState::Absent;
    }
    let LintLevelSource::Node { reason, span, .. } = level_and_source.src
    else {
        return ExceptionState::Inherited;
    };
    if !expectation_on_item(cx, hir_id, span).0 {
        return ExceptionState::Inherited;
    }
    if reason.is_none() {
        return ExceptionState::Unreasoned;
    }
    ExceptionState::Approved
}

/// Return whether the lint-level attribute at `span` is one of the item's own.
///
/// # Specification
/// - requires: `span` is the source of a resolved lint level, and `hir_id`
///   identifies the item the level was resolved at.
/// - ensures: answers affirmatively exactly when one of the item's own
///   attributes encloses `span`, which distinguishes an attribute written on
///   the item from one inherited through the HIR ancestry.
/// - provides: the item-level requirement of the recursion exception.
/// - panics: none.
fn expectation_on_item(
    cx: &LateContext<'_>,
    hir_id: HirId,
    span: Span,
) -> ExpectationOnItem
{
    ExpectationOnItem(
        cx.tcx
            .hir_attrs(hir_id)
            .iter()
            .any(|attr| attr.span().contains(span)),
    )
}

#[cfg(test)]
mod tests
{
    /// The allow-list the `ui_config` fixture is read under: one justified
    /// entry, which is applied, and one entry stating no justification, which
    /// is refused.
    const ALLOW_LIST: &str = r#"
[quenchant-dylints]
non_owning_generics = [
  { path = "std::ptr::NonNull", justification = "holds a raw pointer; its drop never touches the pointee" },
  { path = "std::vec::Vec" },
]
"#;

    #[test]
    fn ui()
    {
        dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
    }

    #[test]
    fn ui_config()
    {
        dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui_config")
            .dylint_toml(ALLOW_LIST)
            .run();
    }

    #[test]
    fn ui_specifications()
    {
        dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui_specifications")
            .rustc_flags(fixture_extern_flags())
            .run();
    }

    /// Cargo's selected build, rather than cache order, identifies UI
    /// dependencies.
    ///
    /// Ordinary and instrumented facade artifacts can share a compiler and a
    /// target directory. The UI matrix needs the instrumented unit
    /// specifically; a compiler stamp alone cannot identify that feature
    /// interpretation.
    ///
    /// # Specification
    /// - ensures: commissions both fixture packages with the facade's
    ///   `anodized` feature enabled, in an isolated target directory, and
    ///   returns the exact artifact paths and dependency directories reported
    ///   by that Cargo build.
    /// - ensures: the fixture edition and `anodized` cfg match that
    ///   interpretation.
    /// - panics: a failed build, invalid Cargo output, or missing selected
    ///   artifact fails setup instead of selecting a different cached
    ///   configuration.
    ///
    /// # Adequacy
    /// - hypothesis: L3 the UI matrix exercises real specification expansion
    ///   even when ordinary and instrumented artifacts coexist in the workspace
    ///   cache.
    /// - witness: `tests::ui_specifications`
    fn fixture_extern_flags() -> Vec<String>
    {
        let test_binary =
            std::env::current_exe().expect("the running test binary has a path on disk");
        let target = test_binary
            .parent()
            .expect("the test binary lives in Cargo's deps directory")
            .join("ui-specification-deps");
        let output = std::process::Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "build",
                "--locked",
                "--message-format=json",
                "-p",
                "quenchant-anodized",
                "-p",
                "quenchant-fixture-macros",
                "--features",
                "quenchant-anodized/anodized",
                "--target-dir",
            ])
            .arg(target)
            .output()
            .expect("Cargo can build the UI fixture dependencies");
        assert!(
            output.status.success(),
            "UI dependency build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut anodized = None;
        let mut fixture_macros = None;
        let mut directories = alloc::collections::BTreeSet::new();
        for message in
            serde_json::Deserializer::from_slice(&output.stdout).into_iter::<serde_json::Value>()
        {
            let message = message.expect("Cargo emits valid JSON messages");
            if message.get("reason").and_then(serde_json::Value::as_str)
                != Some("compiler-artifact")
            {
                continue;
            }
            let target = message
                .get("target")
                .and_then(|target| target.get("name"))
                .and_then(serde_json::Value::as_str);
            let instrumented = target == Some("quenchant_anodized")
                && message
                    .get("features")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|features| features.iter().any(|feature| feature == "anodized"));
            let filenames = message
                .get("filenames")
                .and_then(serde_json::Value::as_array)
                .expect("compiler artifacts report their filenames");
            for filename in filenames {
                let path =
                    std::path::Path::new(filename.as_str().expect("an artifact filename is text"));
                let extension = path.extension().and_then(std::ffi::OsStr::to_str);
                if !matches!(extension, Some("rlib" | "rmeta"))
                    && extension != Some(std::env::consts::DLL_EXTENSION)
                {
                    continue;
                }
                let parent = path.parent().expect("Cargo artifact paths have a parent");
                if !directories.contains(parent) {
                    directories.insert(parent.to_path_buf());
                }
                match (target, extension) {
                    | (Some("quenchant_anodized"), Some("rlib")) if instrumented => {
                        anodized = Some(path.to_path_buf());
                    },
                    | (Some("quenchant_fixture_macros"), Some(extension))
                        if extension == std::env::consts::DLL_EXTENSION =>
                    {
                        fixture_macros = Some(path.to_path_buf());
                    },
                    | _ => {},
                }
            }
        }
        let anodized = anodized.expect("Cargo reported the instrumented facade rlib");
        let fixture_macros = fixture_macros.expect("Cargo reported the fixture macro library");
        let mut flags = vec!["--edition=2024".to_owned()];
        for directory in directories {
            flags.push("-L".to_owned());
            flags.push(format!("dependency={}", directory.display()));
        }
        flags.extend([
            "--extern".to_owned(),
            format!("quenchant={}", anodized.display()),
            "--extern".to_owned(),
            format!("quenchant_fixture_macros={}", fixture_macros.display()),
            "--cfg".to_owned(),
            r#"feature="anodized""#.to_owned(),
        ]);
        flags
    }
}
