//! Primitive arithmetic is resolved by operand type and inherent-definition
//! identity.
//!
//! The producer's sealed integer implementation is the representation boundary;
//! unrelated producer functions and consumer nominal methods are not
//! exemptions.

use rustc_hir::BinOpKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::UnOp;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Symbol;

declare_lint! {
    /// Reject primitive integer operators and resolved inherent arithmetic families.
    /// Nominal operators and same-spelled nominal methods remain accepted.
    /// The local `Integer for Int<primitive>` producer implementation owns the
    /// representation-level operations; nested functions do not inherit it.
    pub PRIMITIVE_ARITHMETIC,
    Allow,
    "primitive arithmetic must use the nominal arithmetic surface"
}

/// Stateless expression-level arithmetic policy.
#[derive(Default)]
pub struct PrimitiveArithmetic;

impl_lint_pass!(PrimitiveArithmetic => [PRIMITIVE_ARITHMETIC]);

impl<'tcx> LateLintPass<'tcx> for PrimitiveArithmetic
{
    /// Report primitive arithmetic outside its implementing representation
    /// boundary.
    ///
    /// # Specification
    /// - ensures: reports arithmetic with primitive integer operands and
    ///   resolved primitive inherent arithmetic families and partial methods,
    ///   including function-item references.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 UI controls distinguish primitive identity, nominal
    ///   spelling, operator overloading, and the exact producer boundary.
    /// - witness: `tests::ui_arithmetic`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if cx
            .tcx
            .lint_level_spec_at_node(PRIMITIVE_ARITHMETIC, expr.hir_id)
            .level()
            == rustc_session::lint::Level::Allow
        {
            return;
        }
        let refused = match expr.kind {
            | ExprKind::Binary(op, left, right) => {
                matches!(
                    op.node,
                    BinOpKind::Add
                        | BinOpKind::Sub
                        | BinOpKind::Mul
                        | BinOpKind::Div
                        | BinOpKind::Rem
                        | BinOpKind::Shl
                        | BinOpKind::Shr
                ) && cx.typeck_results().expr_ty(left).peel_refs().is_integral()
                    && cx.typeck_results().expr_ty(right).peel_refs().is_integral()
            },
            | ExprKind::AssignOp(op, left, right) => {
                matches!(
                    BinOpKind::from(op.node),
                    BinOpKind::Add
                        | BinOpKind::Sub
                        | BinOpKind::Mul
                        | BinOpKind::Div
                        | BinOpKind::Rem
                        | BinOpKind::Shl
                        | BinOpKind::Shr
                ) && cx.typeck_results().expr_ty(left).peel_refs().is_integral()
                    && cx.typeck_results().expr_ty(right).peel_refs().is_integral()
            },
            | ExprKind::Unary(UnOp::Neg, operand) => cx
                .typeck_results()
                .expr_ty(operand)
                .peel_refs()
                .is_integral(),
            | ExprKind::MethodCall(_, receiver, ..) => {
                cx.typeck_results()
                    .expr_ty_adjusted(receiver)
                    .peel_refs()
                    .is_integral()
                    && cx
                        .typeck_results()
                        .type_dependent_def_id(expr.hir_id)
                        .is_some_and(|method| {
                            matches!(primitive_method(cx, method), ArithmeticIdentity::Primitive)
                        })
            },
            | ExprKind::Path(_) => {
                matches!(*cx.typeck_results().expr_ty(expr).kind(), ty::FnDef(method, _)
                    if matches!(primitive_method(cx, method), ArithmeticIdentity::Primitive))
            },
            | _ => false,
        };
        if refused && matches!(producer_boundary(cx, expr), ProducerBoundary::Outside) {
            clippy_utils::diagnostics::span_lint(
                cx,
                PRIMITIVE_ARITHMETIC,
                expr.span,
                "primitive arithmetic must use the nominal arithmetic surface",
            );
        }
    }
}

/// Resolved arithmetic ownership, independent of source spelling.
enum ArithmeticIdentity
{
    /// A primitive inherent method in the selected arithmetic families.
    Primitive,
    /// Another function or a non-arithmetic primitive method.
    Other,
}

/// Resolve arithmetic through the method's inherent implementation self type.
///
/// # Specification
/// - ensures: inherent integer arithmetic families and unprefixed partial
///   arithmetic methods are classified as primitive; extension traits and
///   nominal methods are not. Unsigned square root remains total.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 UI fixtures distinguish aliases, UFCS, references, and
///   nominal methods from genuine primitive inherent definitions.
/// - witness: `tests::ui_arithmetic`
fn primitive_method(
    cx: &LateContext<'_>,
    method: DefId,
) -> ArithmeticIdentity
{
    let Some(implementation) = cx.tcx.impl_of_assoc(method)
    else {
        return ArithmeticIdentity::Other;
    };
    if cx.tcx.impl_is_of_trait(implementation)
        || !cx
            .tcx
            .type_of(implementation)
            .instantiate_identity()
            .skip_norm_wip()
            .is_integral()
    {
        return ArithmeticIdentity::Other;
    }
    let name = cx.tcx.item_name(method);
    let name = name.as_str();
    // This inventory follows core's integer domains: unsigned square root is
    // total, while signed square root rejects negative inputs.
    if matches!(
        name,
        "pow"
            | "abs"
            | "div_euclid"
            | "rem_euclid"
            | "div_floor"
            | "div_ceil"
            | "next_multiple_of"
            | "next_power_of_two"
            | "ilog"
            | "ilog2"
            | "ilog10"
            | "funnel_shl"
            | "funnel_shr"
    ) || (name == "isqrt"
        && cx
            .tcx
            .type_of(implementation)
            .instantiate_identity()
            .skip_norm_wip()
            .is_signed())
    {
        return ArithmeticIdentity::Primitive;
    }
    let Some((family, operation)) = name.split_once('_')
    else {
        return ArithmeticIdentity::Other;
    };
    if matches!(
        family,
        "checked" | "strict" | "wrapping" | "saturating" | "overflowing" | "unchecked"
    ) && matches!(
        operation,
        "add"
            | "sub"
            | "mul"
            | "div"
            | "rem"
            | "neg"
            | "pow"
            | "abs"
            | "shl"
            | "shr"
            | "funnel_shl"
            | "funnel_shr"
            | "add_signed"
            | "sub_signed"
            | "add_unsigned"
            | "sub_unsigned"
            | "signed_diff"
            | "unsigned_diff"
            | "div_euclid"
            | "rem_euclid"
            | "next_multiple_of"
            | "next_power_of_two"
            | "isqrt"
            | "ilog"
            | "ilog2"
            | "ilog10"
    ) {
        ArithmeticIdentity::Primitive
    }
    else {
        ArithmeticIdentity::Other
    }
}

/// Whether the enclosing body implements the producer's representation
/// operations.
enum ProducerBoundary
{
    /// Both nominal and trait definitions belong to the exact producer module.
    IntegerImplementation,
    /// No qualifying enclosing method exists.
    Outside,
}

/// Admit only producer-owned integer trait implementations, not a crate or
/// module.
///
/// # Specification
/// - ensures: a closure inherits its enclosing method's boundary; a nested
///   function stops the search and cannot borrow that boundary.
/// - ensures: both the implemented trait and nominal self type are local and
///   have the canonical producer paths; the representation is a primitive
///   integer.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 UI producer and near-miss bodies distinguish exact
///   ownership from a same-named trait, unrelated helper, or nested function.
/// - witness: `tests::ui_arithmetic`
fn producer_boundary(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> ProducerBoundary
{
    let mut owner = expr.hir_id.owner.def_id.to_def_id();
    while matches!(cx.tcx.def_kind(owner), DefKind::Closure) {
        owner = cx.tcx.parent(owner);
    }
    let Some(implementation) = cx.tcx.impl_of_assoc(owner)
    else {
        return ProducerBoundary::Outside;
    };
    if !cx.tcx.impl_is_of_trait(implementation) {
        return ProducerBoundary::Outside;
    }
    let trait_ref = cx
        .tcx
        .impl_trait_ref(implementation)
        .instantiate_identity()
        .skip_norm_wip();
    let ty::Adt(adt, arguments) = *trait_ref.self_ty().kind()
    else {
        return ProducerBoundary::Outside;
    };
    if trait_ref.def_id.is_local()
        && adt.did().is_local()
        && matches!(
            producer_item(cx, trait_ref.def_id, Symbol::intern("Integer")),
            ProducerItem::Canonical
        )
        && matches!(
            producer_item(cx, adt.did(), Symbol::intern("Int")),
            ProducerItem::Canonical
        )
        && arguments.types().any(ty::Ty::is_integral)
    {
        ProducerBoundary::IntegerImplementation
    }
    else {
        ProducerBoundary::Outside
    }
}

/// Exact producer path recognition without source-text matching.
enum ProducerItem
{
    /// The definition is a direct item of the producer's root arithmetic
    /// module.
    Canonical,
    /// Its definition path differs.
    Other,
}

/// Match the compiler's crate, module, and item identities.
///
/// # Specification
/// - ensures: recognizes only `quenchant_arith::arith::<name>`.
/// - panics: none.
fn producer_item(
    cx: &LateContext<'_>,
    item: DefId,
    name: Symbol,
) -> ProducerItem
{
    let module = cx.tcx.parent(item);
    if cx.tcx.crate_name(item.krate) == Symbol::intern("quenchant_arith")
        && cx.tcx.opt_item_name(item) == Some(name)
        && cx.tcx.opt_item_name(module) == Some(Symbol::intern("arith"))
        && cx.tcx.parent(module).is_crate_root()
    {
        ProducerItem::Canonical
    }
    else {
        ProducerItem::Other
    }
}
