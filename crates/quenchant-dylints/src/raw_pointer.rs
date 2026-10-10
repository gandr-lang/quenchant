//! Raw pointers are taken directly, never through a reference created only to
//! become one.
//!
//! A reference asserts alignment, a valid value and, for `&mut`, uniqueness
//! under the aliasing model. A pointer derived from it inherits those
//! assertions even when the memory is uninitialized, shared with a device, or
//! aliased by another raw pointer the new reference invalidates. `&raw const`
//! and `&raw mut` create the pointer with no reference.
//!
//! Clippy's `borrow_as_ptr` already refuses an explicit cast and the plain
//! implicit coercion of `&place` to a raw pointer of the same pointee; this
//! pass covers the forms it leaves: coercions that also unsize, coercions of a
//! reference value, slice pointer methods that autoref a place, and fresh
//! borrows handed to `ptr::from_ref` or `ptr::from_mut`.

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::is_expr_temporary_value;
use clippy_utils::source::snippet_with_context;
use clippy_utils::std_or_core;
use clippy_utils::sugg::Sugg;
use clippy_utils::sugg::has_enclosing_paren;
use rustc_errors::Applicability;
use rustc_hir::BorrowKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::Mutability;
use rustc_hir::Node;
use rustc_hir::QPath;
use rustc_hir::UnOp;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_middle::ty::adjustment::Adjust;
use rustc_middle::ty::adjustment::Adjustment;
use rustc_middle::ty::adjustment::AutoBorrow;
use rustc_middle::ty::adjustment::DerefAdjustKind;
use rustc_middle::ty::adjustment::PointerCoercion;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

declare_lint! {
    /// ### What it does
    ///
    /// A raw pointer must not be taken through a reference created only to become that pointer. The pass refuses a borrow coerced to a raw pointer through an unsizing step, a reference value coerced to a raw pointer, a slice's `as_ptr` or `as_mut_ptr` that borrows an owned place or a place behind a raw pointer, and a fresh borrow passed to `ptr::from_ref` or `ptr::from_mut`.
    ///
    /// ### Why is this bad?
    ///
    /// The intermediate reference asserts alignment, a valid value and, for `&mut`, uniqueness. Next to `unsafe` code the memory may be uninitialized, shared with a device, or aliased by another raw pointer that the new reference invalidates. `&raw const place` and `&raw mut place` create the pointer with no reference.
    ///
    /// ### Accepted forms
    ///
    /// A receiver that is already a reference value creates no new borrow: `slice.as_ptr()` where `slice: &[T]`, a place reached through a reference, and `ptr::from_ref(r)` for an existing reference `r`. The plain implicit coercion `let p: *mut T = &mut x;` is Clippy's `borrow_as_ptr`, which this pass leaves to it.
    ///
    /// ### Blind spots
    ///
    /// Macro-expanded code and borrows of temporaries are not inspected. Only slice `as_ptr` and `as_mut_ptr` are recognized among pointer-producing methods.
    ///
    /// ### Example
    ///
    /// ```rust
    /// let mut words = [0_u32; 4];
    /// let first = words.as_mut_ptr();
    /// ```
    ///
    /// A pointer with no intermediate reference:
    ///
    /// ```rust
    /// let mut words = [0_u32; 4];
    /// let first = (&raw mut words).cast::<u32>();
    /// ```
    pub RAW_POINTER_THROUGH_REFERENCE,
    Deny,
    "raw pointers are taken with `&raw`, not through an intermediate reference"
}

/// Stateless expression-level pointer-origin policy.
#[derive(Default)]
pub struct RawPointerThroughReference;

impl_lint_pass!(RawPointerThroughReference => [RAW_POINTER_THROUGH_REFERENCE]);

impl<'tcx> LateLintPass<'tcx> for RawPointerThroughReference
{
    /// Inspect each authored expression for the three refused origins.
    ///
    /// # Specification
    /// - ensures: reports a refused coercion, method receiver or pointer
    ///   constructor argument once, at the expression that creates the
    ///   reference.
    /// - ensures: expressions from macro expansions and desugarings are not
    ///   inspected.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 UI witnesses separate each refused form from the
    ///   accepted reference-value and reached-through-reference forms.
    /// - witness: `tests::ui_pointers`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if expr.span.from_expansion() {
            return;
        }
        check_coercion(cx, expr);
        match expr.kind {
            | ExprKind::MethodCall(_, receiver, &[], _) => check_pointer_method(cx, expr, receiver),
            | ExprKind::Call(callee, arguments) => {
                if let Some(argument) = arguments.first()
                    && arguments.len() == 1
                {
                    check_pointer_constructor(cx, expr, callee, argument);
                }
            },
            | _ => {},
        }
    }
}

/// Where a place's storage is reached from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin
{
    /// A local or static, possibly through owned boxes.
    Owned,
    /// The dereference of a raw pointer.
    RawPointer,
    /// An existing reference, or an overloaded dereference or index that
    /// returned one.
    Reference,
    /// Not a place this pass can rewrite.
    Value,
}

/// Classify the storage a place expression names.
///
/// # Specification
/// - ensures: overloaded dereferences and indexing classify as `Reference`.
/// - ensures: follows fields, built-in indexing, box dereferences and the
///   autoderef steps recorded on each projection base; the first reference met
///   yields `Reference`, a raw-pointer dereference `RawPointer`, and a local or
///   static `Owned`.
/// - ensures: overloaded dereferences and indexing yield `Reference`; any other
///   expression yields `Value`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 owned, boxed, raw-pointer and reference-reached receivers
///   are distinguished in the UI witnesses.
/// - witness: `tests::ui_pointers`
#[spec(ensures: |output| !matches!(place.kind, ExprKind::Unary(UnOp::Deref, _) | ExprKind::Index(..))
    || !cx.typeck_results().is_method_call(place) || output == Origin::Reference)]
fn place_origin<'tcx>(
    cx: &LateContext<'tcx>,
    mut place: &'tcx Expr<'tcx>,
) -> Origin
{
    let typeck = cx.typeck_results();
    loop {
        place = match place.kind {
            | ExprKind::Field(base, _) | ExprKind::Index(base, ..) => {
                if typeck.is_method_call(place) || adjusted_through(cx, base) == Reached::Reference
                {
                    return Origin::Reference;
                }
                base
            },
            | ExprKind::Unary(UnOp::Deref, base) => {
                if typeck.is_method_call(place) {
                    return Origin::Reference;
                }
                match *typeck.expr_ty(base).kind() {
                    | ty::Ref(..) => return Origin::Reference,
                    | ty::RawPtr(..) => return Origin::RawPointer,
                    | ty::Adt(definition, _) if definition.is_box() => base,
                    | _ => return Origin::Value,
                }
            },
            | ExprKind::Path(QPath::Resolved(None, path)) => {
                return match path.res {
                    | Res::Local(_) | Res::Def(DefKind::Static { .. }, _) => Origin::Owned,
                    | _ => Origin::Value,
                };
            },
            | _ => return Origin::Value,
        };
    }
}

/// How an expression's recorded adjustments reach the value they borrow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reached
{
    /// No adjustment dereferences a reference.
    Directly,
    /// An autoderef step is overloaded or starts from a reference.
    Reference,
}

/// Classify an expression's recorded autoderef steps.
///
/// # Specification
/// - ensures: `Reference` when an autoderef step is overloaded or starts from a
///   reference type; box dereferences count as `Directly`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — compiler fixtures contrast accepted and refused forms
///   through their observable diagnostics.
/// - witness: `tests::ui_pointers`
#[spec(ensures: |output| {
    let typeck = cx.typeck_results();
    let sources = core::iter::once(typeck.expr_ty(expr)).chain(typeck.expr_adjustments(expr).iter().map(|adjustment| adjustment.target));
    (output == Reached::Reference) == typeck.expr_adjustments(expr).iter().zip(sources).any(|(adjustment, source)| {
        matches!(adjustment.kind, Adjust::Deref(kind) if !matches!(kind, DerefAdjustKind::Builtin) || source.is_ref())
    })
})]
fn adjusted_through<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Reached
{
    let typeck = cx.typeck_results();
    let mut source = typeck.expr_ty(expr);
    for adjustment in typeck.expr_adjustments(expr) {
        if let Adjust::Deref(kind) = adjustment.kind
            && (!matches!(kind, DerefAdjustKind::Builtin) || source.is_ref())
        {
            return Reached::Reference;
        }
        source = adjustment.target;
    }
    Reached::Directly
}

/// Refuse a reference coerced to a raw pointer, except the plain form Clippy
/// reports.
///
/// # Specification
/// - ensures: a borrow of a place coerced through more than the deref and raw
///   borrow is reported with a `&raw` replacement; the exact two-step form is
///   left to `clippy::borrow_as_ptr`.
/// - ensures: a reference value coerced to a raw pointer is reported with a
///   `ptr::from_ref` or `ptr::from_mut` replacement.
/// - ensures: borrows of temporaries are not reported.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L3 unsizing borrows, argument and binding coercions of
///   reference values, and the plain form distinguish the reported cases.
/// - witness: `tests::ui_pointers`
fn check_coercion<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
)
{
    let adjustments = cx.typeck_results().expr_adjustments(expr);
    let RawBorrow::Coerced(target) = raw_borrow(adjustments)
    else {
        return;
    };
    let source = cx.typeck_results().expr_ty(expr);
    let ty::Ref(_, _, source_mutability) = *source.kind()
    else {
        return;
    };
    if let ExprKind::AddrOf(BorrowKind::Ref, _, place) = expr.kind {
        if is_expr_temporary_value(cx, place) || adjustments.len() == 2 {
            return;
        }
        emit_raw_borrow(cx, expr.span, expr, place, target);
        return;
    }
    let Some(std_or_core) = std_or_core(cx)
    else {
        return;
    };
    let mut applicability = Applicability::MachineApplicable;
    let value = Sugg::hir_with_context(cx, expr, expr.span.ctxt(), "_", &mut applicability);
    let replacement = match (source_mutability, target) {
        | (Mutability::Mut, Mutability::Mut) => format!("{std_or_core}::ptr::from_mut({value})"),
        | (Mutability::Mut, Mutability::Not) => {
            format!("{std_or_core}::ptr::from_mut({value}).cast_const()")
        },
        | (Mutability::Not, _) => format!("{std_or_core}::ptr::from_ref({value})"),
    };
    span_lint_and_then(
        cx,
        RAW_POINTER_THROUGH_REFERENCE,
        expr.span,
        "this reference becomes a raw pointer by coercion",
        |diagnostic| {
            diagnostic.span_suggestion_verbose(
                expr.span,
                "convert the reference explicitly",
                replacement,
                applicability,
            );
        },
    );
}

/// Whether adjustments coerce a reference to a raw pointer.
#[derive(Clone, Copy)]
enum RawBorrow
{
    /// A built-in deref followed by a raw borrow of this mutability.
    Coerced(Mutability),
    /// Any other adjustment sequence.
    Other,
}

/// Recognize a deref-then-raw-borrow coercion.
///
/// # Specification
/// - ensures: matches only a leading built-in deref followed by a raw borrow;
///   later unsizing steps are allowed.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — compiler fixtures contrast accepted and refused forms
///   through their observable diagnostics.
/// - witness: `tests::ui_pointers`
#[spec(ensures: |output| match *adjustments {
    [Adjustment { kind: Adjust::Deref(DerefAdjustKind::Builtin), .. }, Adjustment { kind: Adjust::Borrow(AutoBorrow::RawPtr(expected)), .. }, ..] => matches!(output, RawBorrow::Coerced(actual) if actual == expected),
    _ => matches!(output, RawBorrow::Other),
})]
fn raw_borrow(adjustments: &[Adjustment<'_>]) -> RawBorrow
{
    match *adjustments {
        | [
            Adjustment {
                kind: Adjust::Deref(DerefAdjustKind::Builtin),
                ..
            },
            Adjustment {
                kind: Adjust::Borrow(AutoBorrow::RawPtr(mutability)),
                ..
            },
            ..,
        ] => RawBorrow::Coerced(mutability),
        | _ => RawBorrow::Other,
    }
}

/// Refuse a slice pointer method whose receiver borrows an owned place or a
/// place behind a raw pointer.
///
/// # Specification
/// - ensures: `as_ptr` and `as_mut_ptr` of `[T]` are inspected; an autoref of
///   the receiver, or an explicit `&`/`&mut` receiver, is reported when the
///   borrowed place is owned or behind a raw pointer.
/// - ensures: a receiver that is a reference value, or a place reached through
///   one, is not reported.
/// - ensures: the suggestion takes `&raw` of the place and casts it to the
///   element type.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L3 arrays, boxed slices, raw-pointer places and explicit
///   borrows are reported; slice references, reborrows and reference-reached
///   fields are not.
/// - witness: `tests::ui_pointers`
fn check_pointer_method<'tcx>(
    cx: &LateContext<'tcx>,
    call: &'tcx Expr<'tcx>,
    receiver: &'tcx Expr<'tcx>,
)
{
    let typeck = cx.typeck_results();
    let Some(method) = typeck.type_dependent_def_id(call.hir_id)
    else {
        return;
    };
    let mutability = match cx.tcx.item_name(method).as_str() {
        | "as_ptr" => Mutability::Not,
        | "as_mut_ptr" => Mutability::Mut,
        | _ => return,
    };
    let Some(implementation) = cx.tcx.inherent_impl_of_assoc(method)
    else {
        return;
    };
    let ty::Slice(element) = *cx
        .tcx
        .type_of(implementation)
        .instantiate_identity()
        .skip_norm_wip()
        .kind()
    else {
        return;
    };
    // Method resolution may dereference the receiver, through owned boxes, to
    // reach its slice; the place the method borrows is then that many
    // dereferences of the receiver. Under an explicit borrow the first step
    // only undoes that borrow, and an overloaded step goes through a reference
    // `&raw` cannot avoid.
    let (place, derefs) = if let ExprKind::AddrOf(BorrowKind::Ref, _, place) = receiver.kind {
        let steps: Vec<DerefAdjustKind> = typeck
            .expr_adjustments(receiver)
            .iter()
            .map_while(|adjustment| match adjustment.kind {
                | Adjust::Deref(kind) => Some(kind),
                | _ => None,
            })
            .collect();
        if steps
            .iter()
            .skip(1)
            .any(|kind| !matches!(kind, DerefAdjustKind::Builtin))
        {
            return;
        }
        (place, steps.len().saturating_sub(1))
    }
    else if autoref(cx, receiver) == Receiver::Autoref {
        let derefs = typeck
            .expr_adjustments(receiver)
            .iter()
            .take_while(|adjustment| matches!(adjustment.kind, Adjust::Deref(_)))
            .count();
        (receiver, derefs)
    }
    else {
        return;
    };
    if !matches!(place_origin(cx, place), Origin::Owned | Origin::RawPointer) {
        return;
    }
    let mut applicability = Applicability::MachineApplicable;
    let snippet =
        snippet_with_context(cx, place.span, call.span.ctxt(), "..", &mut applicability).0;
    let inner = if has_enclosing_paren(&snippet) {
        snippet
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or(&snippet)
    }
    else {
        &snippet
    };
    let place_text = format!("{}{inner}", "*".repeat(derefs));
    let element = typeck
        .node_type(call.hir_id)
        .builtin_deref(true)
        .unwrap_or(element);
    span_lint_and_then(
        cx,
        RAW_POINTER_THROUGH_REFERENCE,
        call.span,
        "this method borrows the whole place before taking its pointer",
        |diagnostic| {
            diagnostic.span_suggestion_verbose(
                call.span,
                "take the pointer without a reference",
                format!(
                    "(&raw {} {place_text}).cast::<{element}>()",
                    mutability.ptr_str()
                ),
                applicability,
            );
        },
    );
}

/// How a method receiver reaches its reference.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Receiver
{
    /// The receiver place is borrowed by an autoref.
    Autoref,
    /// The receiver is a reference value, reborrowed or used as is.
    Reference,
}

/// Classify a method receiver's adjustments.
///
/// # Specification
/// - ensures: `Autoref` when the adjustments borrow the receiver as a reference
///   and no earlier step dereferences a reference.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — compiler fixtures contrast accepted and refused forms
///   through their observable diagnostics.
/// - witness: `tests::ui_pointers`
#[spec(ensures: |output| (output == Receiver::Autoref) == (adjusted_through(cx, receiver) == Reached::Directly
    && cx.typeck_results().expr_adjustments(receiver).iter().any(|adjustment| matches!(adjustment.kind, Adjust::Borrow(AutoBorrow::Ref(_))))))]
fn autoref<'tcx>(
    cx: &LateContext<'tcx>,
    receiver: &'tcx Expr<'tcx>,
) -> Receiver
{
    let borrows = cx
        .typeck_results()
        .expr_adjustments(receiver)
        .iter()
        .any(|adjustment| matches!(adjustment.kind, Adjust::Borrow(AutoBorrow::Ref(_))));
    if borrows && adjusted_through(cx, receiver) == Reached::Directly {
        Receiver::Autoref
    }
    else {
        Receiver::Reference
    }
}

/// Refuse a fresh borrow passed straight to `ptr::from_ref` or `ptr::from_mut`.
///
/// # Specification
/// - ensures: a call to `core::ptr::from_ref` or `core::ptr::from_mut`, under
///   any path, whose argument borrows a non-temporary place is reported with a
///   `&raw` replacement.
/// - ensures: when the constructor's type parameter coerces the borrow, the
///   replacement repeats the coercion: each builtin dereference after the one
///   undoing the borrow becomes a `*` of the place, and an unsizing step casts
///   to the call's pointer type, since `&raw` keeps the place's own type and
///   `cast` cannot unsize.
/// - ensures: a borrow coerced through an overloaded dereference is not
///   reported, since `&raw` cannot avoid the reference `Deref::deref` creates.
/// - ensures: the replacement is parenthesized where its parent expression
///   would otherwise bind to the place: a method receiver, a field or index
///   base, and, for a cast replacement, a unary or borrow operand.
/// - ensures: an argument that is an existing reference value is not reported.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L3 a fresh borrow and an existing reference passed to each
///   constructor distinguish the reported case.
/// - witness: `tests::ui_pointers`
fn check_pointer_constructor<'tcx>(
    cx: &LateContext<'tcx>,
    call: &'tcx Expr<'tcx>,
    callee: &'tcx Expr<'tcx>,
    argument: &'tcx Expr<'tcx>,
)
{
    let ExprKind::Path(ref path) = callee.kind
    else {
        return;
    };
    let Res::Def(DefKind::Fn, function) = cx.qpath_res(path, callee.hir_id)
    else {
        return;
    };
    let path = cx.get_def_path(function);
    let mut names = path.iter().map(rustc_span::Symbol::as_str);
    let mutability = match (names.next(), names.next(), names.next(), names.next()) {
        | (Some("core"), Some("ptr"), Some("from_ref"), None) => Mutability::Not,
        | (Some("core"), Some("ptr"), Some("from_mut"), None) => Mutability::Mut,
        | _ => return,
    };
    let ExprKind::AddrOf(BorrowKind::Ref, _, place) = argument.kind
    else {
        return;
    };
    if is_expr_temporary_value(cx, place) {
        return;
    }
    let typeck = cx.typeck_results();
    // The type parameter may coerce the borrow: dereferences reach the
    // pointee, then an unsizing step may follow. The first dereference only
    // undoes the explicit borrow; a cast can neither dereference nor reach
    // through an overloaded `Deref`.
    let adjustments = typeck.expr_adjustments(argument);
    let steps = adjustments
        .iter()
        .take_while(|adjustment| matches!(adjustment.kind, Adjust::Deref(_)));
    if steps
        .clone()
        .skip(1)
        .any(|adjustment| !matches!(adjustment.kind, Adjust::Deref(DerefAdjustKind::Builtin)))
    {
        return;
    }
    let derefs = steps.count().saturating_sub(1);
    let unsized_by_call = adjustments
        .iter()
        .any(|adjustment| matches!(adjustment.kind, Adjust::Pointer(PointerCoercion::Unsize)));
    let mut applicability = Applicability::MachineApplicable;
    let place_text = snippet_with_context(
        cx,
        place.span,
        argument.span.ctxt(),
        "..",
        &mut applicability,
    )
    .0;
    let mut replacement = format!(
        "&raw {} {}{place_text}",
        mutability.ptr_str(),
        "*".repeat(derefs)
    );
    if unsized_by_call {
        let pointer = typeck.expr_ty(call);
        // A named type may not be in scope at the call site as printed.
        let names_items = pointer.walk().any(|argument| {
            argument.as_type().is_some_and(|nested| {
                matches!(
                    nested.kind(),
                    ty::Adt(..) | ty::Dynamic(..) | ty::Param(..) | ty::Alias(..)
                )
            })
        });
        if names_items {
            applicability = Applicability::MaybeIncorrect;
        }
        replacement = format!("{replacement} as {pointer}");
    }
    let parenthesize = match cx.tcx.parent_hir_node(call.hir_id) {
        | Node::Expr(parent) => match parent.kind {
            | ExprKind::MethodCall(_, receiver, ..) => receiver.hir_id == call.hir_id,
            | ExprKind::Field(base, _) | ExprKind::Index(base, ..) => base.hir_id == call.hir_id,
            | ExprKind::Unary(..) | ExprKind::AddrOf(..) => unsized_by_call,
            | _ => false,
        },
        | _ => false,
    };
    if parenthesize {
        replacement = format!("({replacement})");
    }
    span_lint_and_then(
        cx,
        RAW_POINTER_THROUGH_REFERENCE,
        call.span,
        "this borrow only feeds a pointer constructor",
        |diagnostic| {
            diagnostic.span_suggestion_verbose(
                call.span,
                "take the pointer without a reference",
                replacement,
                applicability,
            );
        },
    );
}

/// Report a borrow coerced to a raw pointer, suggesting its `&raw` form.
///
/// # Specification
/// - ensures: one diagnostic at `span`, suggesting `&raw const` or `&raw mut`
///   of `place` in the borrow's own position, so the coercion still applies.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L2 — compiler fixtures contrast accepted and refused forms
///   through their observable diagnostics.
/// - witness: `tests::ui_pointers`
fn emit_raw_borrow<'tcx>(
    cx: &LateContext<'tcx>,
    span: Span,
    borrow: &'tcx Expr<'tcx>,
    place: &'tcx Expr<'tcx>,
    mutability: Mutability,
)
{
    let mut applicability = Applicability::MachineApplicable;
    let place_text =
        snippet_with_context(cx, place.span, borrow.span.ctxt(), "..", &mut applicability).0;
    span_lint_and_then(
        cx,
        RAW_POINTER_THROUGH_REFERENCE,
        span,
        "this borrow becomes a raw pointer",
        |diagnostic| {
            diagnostic.span_suggestion_verbose(
                span,
                "take the pointer without a reference",
                format!("&raw {} {place_text}", mutability.ptr_str()),
                applicability,
            );
        },
    );
}
