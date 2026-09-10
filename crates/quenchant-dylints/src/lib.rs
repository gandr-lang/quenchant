//! Compiler-visible policy boundaries, kept separate from verification
//! evidence.
//!
//! Layout and signature rules constrain the type-level approximation. Call and
//! ownership graphs expose different routes to recursion. Their findings are
//! useful refutations within those analyses, not general termination proofs.
//!
//! The remaining rules read authored rustdoc: specification presence, adequacy
//! grammar, and declared judgment scrutinees. A correctly shaped statement has
//! not thereby been satisfied. Runnable witness resolution belongs to
//! `quenchant-gates`, which can inspect the workspace inventory that a lint
//! pass cannot see.
//!
//! Authorship is determined from both the declaration and its name. A foreign
//! macro can manufacture a method while reusing an author's identifier; that
//! generated sibling must not acquire the author's documentation obligation by
//! token coincidence. This distinction never makes a generated assertion
//! failure safe to ignore.
//!
//! Specification fixtures use the public facade and published backend. The
//! `cdylib` itself links compiler internals, so its compiler, Clippy source,
//! and Dylint driver are one compatibility boundary.

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
mod arithmetic;
mod callgraph;
mod graph;
mod judgement;
mod option_signature;
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
    /// A one-field named or tuple struct must make its transparent layout explicit.
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
    /// Explicit layout:
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
    /// Workspace-authored signatures must reach a nominal boundary before exposing primitives, including beneath structural layers, inspected containers, and aliases.
    ///
    /// ### Why is this bad?
    ///
    /// Nominal domain types distinguish meanings that share a primitive representation. This check recognizes local transparent boundaries; field privacy, conversion discipline, and the adequacy of the domain meaning remain separate Clippy and review obligations.
    ///
    /// ### Example
    ///
    /// ```rust
    /// fn parse(offset: usize) -> Option<bool> { Some(true) }
    /// ```
    ///
    /// An interface retaining domain roles and absence evidence:
    ///
    /// ```rust
    /// #[repr(transparent)]
    /// struct ByteOffset(usize);
    ///
    /// #[repr(transparent)]
    /// struct ParseSucceeded(bool);
    ///
    /// quenchant_shape::reason_enum! {
    ///     mod parse_attempt {
    ///         pub enum Pending { MoreInputRequired }
    ///     }
    /// }
    ///
    /// trait Parser {
    ///     fn parse(&self, offset: ByteOffset)
    ///         -> quenchant_shape::shape::Maybe<ParseSucceeded, parse_attempt::Pending>;
    /// }
    /// ```
    pub PRIMITIVE_SIGNATURE,
    Deny,
    "function signatures must use semantic wrappers instead of Rust primitives"
}

declare_lint! {
    /// ### What it does
    ///
    /// Every local function in a recursive call component receives a denial.
    ///
    /// ### Why is this bad?
    ///
    /// Input-dependent call depth can exhaust the stack because Rust supplies no tail-call elimination guarantee. An explicit worklist, heap continuation stack, or loop makes that control state bounded independently of the host call stack.
    ///
    /// ### The exception
    ///
    /// An approved exception is item-local `#[expect(recursion_forbidden, reason = "...")]` plus a complete `# Termination` section containing ordered `reason`, `measure`, `boundedness`, and `input recursion` bullets. Enclosing lint levels do not confer approval. A no-input-recursion claim is refuted by a cycle edge carrying parameter-derived data, including when explanatory prose follows the claim.
    ///
    /// ### Blind spots
    ///
    /// Only recovered local HIR edges participate. Derived-trait calls through external generics, compiler-generated drop glue, and definition-erasing function-pointer coercions can hide cycles. A bound function item retains `FnDef` identity and remains resolvable. Calls inside closures count conservatively against the function constructing the closure.
    ///
    /// ### Example
    ///
    /// ```rust
    /// fn depth(node: Node) -> Depth { depth(node.child()) }
    /// ```
    ///
    /// Explicit iteration:
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
    /// Local ADTs are denied when owned fields can lead back to the same type.
    ///
    /// ### Why is this bad?
    ///
    /// Ownership-linked recursion can make destruction and derived cloning recurse with data depth even when no authored function does. Their generated behavior is outside the call graph. Flat arenas and interning tables make children identities rather than recursively owned nodes.
    ///
    /// ### What forms an edge
    ///
    /// Ownership follows instantiated field types. `Vec<Self>` can close a cycle; a box containing only an arena index does not. Arrays, slices, and tuples own their components. Local generic positions use inferred field ownership, while external positions are conservatively owning. References, raw pointers, function pointers, closures, trait objects, `PhantomData`, and `Weak` contribute no edge in this analysis.
    ///
    /// ### The allow-list
    ///
    /// External generics can be declared non-owning in workspace `dylint.toml` only with a justification:
    ///
    /// ```toml
    /// [quenchant-dylints]
    /// non_owning_generics = [
    ///   { path = "std::ptr::NonNull", justification = "holds a raw pointer; its drop never touches the pointee" },
    /// ]
    /// ```
    ///
    /// A missing justification leaves the external type conservatively owning and produces a configuration diagnostic.
    ///
    /// ### Blind spots
    ///
    /// Trait objects, closure captures, and raw pointers can hide a concrete return path and therefore escape this type-identity analysis.
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
    /// Flat identity-addressed storage:
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

/// The driver's symbol entry point installs the workspace's compiler-policy
/// passes.
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
        arithmetic::PRIMITIVE_ARITHMETIC,
        option_signature::OPTION_SIGNATURE,
    ]);
    lint_store.register_late_pass(Box::new(move |_| {
        Box::new(WorkflowBoundaries::new(allow_list.clone()))
    }));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowAdequacy)));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowJudgement)));
    lint_store.register_late_pass(Box::new(|_| Box::new(WorkflowSpecification)));
    lint_store.register_late_pass(Box::new(|_| Box::new(arithmetic::PrimitiveArithmetic)));
    lint_store.register_late_pass(Box::new(|_| Box::new(option_signature::OptionSignature)));
}

/// Every member of a recovered call cycle receives this denial.
const RECURSION_MESSAGE: &str = concat!(
    "function participates in a recursive call cycle; recursion is forbidden — write the ",
    "iterative form (worklist, explicit frame stack), or take the owner-approved exception ",
    "(`#[expect(recursion_forbidden, reason = \"...\")]` plus a complete `# Termination` section)",
);

/// Inherited lint state cannot supply item-local exception approval.
const INHERITED_EXPECT_MESSAGE: &str = concat!(
    "recursion exception must be attached to this item: an ",
    "`#[expect(recursion_forbidden, ..)]` on an enclosing module or on the crate root approves ",
    "nothing inside it",
);

/// An exception without a reason lacks its required justification.
const MISSING_REASON_MESSAGE: &str = concat!(
    "recursion exception must state `reason = \"...\"` on its ",
    "`#[expect(recursion_forbidden, ..)]` attribute",
);

/// Exception syntax distinguishes local justification from inherited lint
/// state.
enum ExceptionState
{
    /// No relevant expectation appears on this item or an enclosing scope.
    Absent,
    /// This item carries the expected lint and an explicit reason.
    Approved,
    /// This item names the expected lint without its reason.
    Unreasoned,
    /// An enclosing module or crate supplies the expectation instead of the
    /// item.
    Inherited,
}

/// Each cyclic type receives the ownership-recursion denial.
const OWNING_CYCLE_MESSAGE: &str = concat!(
    "type owns its way back to itself; pointer-linked recursive data is forbidden — the drop glue ",
    "and the derived traversals both recurse over the pointee, and neither is a function the ",
    "call-plane gate can see",
);

/// Ownership repair guidance replaces recursively owned children with
/// identities.
const OWNING_CYCLE_HELP: &str = concat!(
    "use the flat id-addressed form: a child is an index into an arena or interning table, never a ",
    "node linked through `Box`, `Rc`, `Arc`, `Vec`, or any other owning carrier. Outside a ",
    "recursive position `Arc` is the one permitted shared-ownership pointer, for whole values at ",
    "boundaries",
);

/// Per-crate collection and post-walk analysis share one boundary-policy state.
struct WorkflowBoundaries
{
    /// Function definitions collected before whole-crate cycle analysis.
    functions: HashMap<LocalDefId, FunctionNode>,
    /// Caller identity indexes the local edges and argument roots collected
    /// from bodies.
    edges: HashMap<LocalDefId, Vec<CallEdge>>,
    /// Type definitions retain their diagnostic identity until ownership
    /// analysis.
    adts: HashMap<LocalDefId, AdtNode>,
    /// Justified external ownership boundaries constrain generic traversal.
    allow_list: NonOwningAllowList,
}

impl WorkflowBoundaries
{
    /// Configuration admission precedes collection of the crate's functions and
    /// types.
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
    /// Layout checks and whole-crate type collection share the authored item
    /// boundary.
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

    /// Signature inspection retains the function and call data needed after
    /// traversal.
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

    /// Body-free trait methods still own their authored signature boundary.
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

    /// Foreign function declarations still expose an authored signature.
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

    /// Whole-crate findings are determined after all relevant declarations have
    /// been collected.
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

            // The denial also fulfills an item-local expectation. Omitting it for an
            // excepted item would instead leave that expectation unfulfilled.
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

            // Crate-root lint state prevents the item's expectation from hiding an invalid
            // exception.
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

/// Diagnostic anchoring must survive macro provenance filtering.
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
/// Rustc suppresses diagnostics whose primary span belongs to an external
/// macro, including attribute macros defined locally. Such a macro can own the
/// whole-item span while preserving the authored name token. Re-anchoring to
/// that name keeps the diagnostic visible across specification expansion.
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

/// An explicit transparent representation satisfies the one-field layout
/// requirement.
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

/// Foreign trait identity determines whether the trait owns the required
/// signature shape.
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

/// Item-local reasoned expectations are distinguished from missing and
/// inherited ones.
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

/// Attribute-span ownership determines whether exception syntax belongs to this
/// item.
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
    /// Configuration witnesses distinguish a justified non-owning boundary from
    /// an unjustified entry that must remain owning.
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
    fn ui_arithmetic()
    {
        let mut flags = fixture_extern_flags();
        flags.extend([
            "-Dunknown-lints".to_owned(),
            "-Dprimitive_arithmetic".to_owned(),
            "-Doption_signature".to_owned(),
            "-Aprimitive_signature".to_owned(),
            "-Aspecification_present".to_owned(),
            "-Adead_code".to_owned(),
        ]);
        dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui_arithmetic")
            .rustc_flags(flags)
            .run();
    }

    #[test]
    fn ui_options()
    {
        let mut flags = fixture_extern_flags();
        flags.extend([
            "-Dunknown-lints".to_owned(),
            "-Doption_signature".to_owned(),
            "-Aprimitive_signature".to_owned(),
            "-Aspecification_present".to_owned(),
            "-Adead_code".to_owned(),
        ]);
        dylint_testing::ui::Test::src_base(env!("CARGO_PKG_NAME"), "ui_options")
            .rustc_flags(flags)
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
    /// - ensures: commissions the fixture packages and public arithmetic/shape
    ///   libraries with the facade's `anodized` feature enabled, in an isolated
    ///   target directory, and returns the exact artifact paths and dependency
    ///   directories reported by that Cargo build.
    /// - ensures: the fixture edition and `anodized` cfg match that
    ///   interpretation.
    /// - panics: a failed build, invalid Cargo output, or missing selected
    ///   artifact fails setup instead of selecting a different cached
    ///   configuration.
    ///
    /// # Adequacy
    /// - hypothesis: L3 the UI matrix exercises real specification expansion
    ///   even when ordinary and instrumented artifacts coexist in the workspace
    ///   cache; arithmetic and absence controls compile against the selected
    ///   public library artifacts rather than nominal lookalikes.
    /// - witness: `tests::ui_specifications`
    /// - witness: `tests::ui_arithmetic`
    /// - witness: `tests::ui_options`
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
                "-p",
                "quenchant-arith",
                "-p",
                "quenchant-shape",
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
        let mut arithmetic = None;
        let mut shape = None;
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
                    | (Some("quenchant_arith"), Some("rlib")) => {
                        arithmetic = Some(path.to_path_buf());
                    },
                    | (Some("quenchant_shape"), Some("rlib")) => {
                        shape = Some(path.to_path_buf());
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
        let arithmetic = arithmetic.expect("Cargo reported the arithmetic library");
        let shape = shape.expect("Cargo reported the reason-bearing shape library");
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
            "--extern".to_owned(),
            format!("quenchant_arith={}", arithmetic.display()),
            "--extern".to_owned(),
            format!("quenchant_shape={}", shape.display()),
            "--cfg".to_owned(),
            r#"feature="anodized""#.to_owned(),
        ]);
        flags
    }
}
