//! Standard absence is checked by type identity, including normalized aliases.
//!
//! Foreign methods are paired with their unsubstituted declarations: required
//! Option layers are accepted, while absence introduced by substitution is not.

use rustc_hir::Body;
use rustc_hir::FnDecl;
use rustc_hir::FnRetTy;
use rustc_hir::ForeignItem;
use rustc_hir::ForeignItemKind;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::FnKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;
use rustc_span::sym;

use crate::signature::normalize_middle_ty;

declare_lint! {
    /// Reject standard Option before an admitted nominal signature boundary.
    /// Foreign traits permit only the layers required by their original method
    /// declarations, not layers introduced by generic or associated substitution.
    pub OPTION_SIGNATURE,
    Allow,
    "crate-defined signatures must preserve the reason for absence"
}

/// Stateless signature-level absence policy.
#[derive(Default)]
pub struct OptionSignature;

impl_lint_pass!(OptionSignature => [OPTION_SIGNATURE]);

impl<'tcx> LateLintPass<'tcx> for OptionSignature
{
    /// Inspect body-bearing declarations without treating closures as APIs.
    ///
    /// # Specification
    /// - ensures: checks free functions, inherent methods, and trait methods;
    ///   closure signatures are inferred implementation details.
    /// - panics: none.
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: FnKind<'tcx>,
        declaration: &'tcx FnDecl<'tcx>,
        _: &'tcx Body<'tcx>,
        _: Span,
        owner: LocalDefId,
    )
    {
        if !matches!(kind, FnKind::Closure) {
            check_declaration(cx, declaration, owner);
        }
    }

    /// Required local trait methods own signatures even without bodies.
    ///
    /// # Specification
    /// trivial.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx TraitItem<'tcx>,
    )
    {
        if let TraitItemKind::Fn(signature, TraitFn::Required(_)) = item.kind {
            check_declaration(cx, signature.decl, item.owner_id.def_id);
        }
    }

    /// Foreign declarations remain crate-authored signature boundaries.
    ///
    /// # Specification
    /// trivial.
    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    )
    {
        if let ForeignItemKind::Fn(signature, ..) = item.kind {
            check_declaration(cx, signature.decl, item.owner_id.def_id);
        }
    }
}

/// The foreign declaration supplies a structural layer, never a substituted
/// one.
#[derive(Clone, Copy)]
enum Required<'tcx>
{
    /// This layer is authored locally or introduced by substitution.
    Authored,
    /// This layer occurs in the external method's identity-instantiated
    /// signature.
    Foreign(ty::Ty<'tcx>),
}

/// Type traversal reports a semantic absence exposure once per signature
/// position.
#[derive(Clone, Copy)]
enum Exposure
{
    /// A non-required Option is reachable before the nominal boundary.
    Option,
    /// No such layer was resolved in the supported type structure.
    NotFound,
}

/// Pair each signature position with its external declaration, when one exists.
///
/// # Specification
/// - ensures: reports each offending input or explicit output once; aliases
///   cannot hide the standard Option identity.
/// - ensures: a foreign method contributes only its unsubstituted type shape.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 UI witnesses separate direct, nested, aliased, async,
///   required foreign, and substitution-introduced absence.
/// - witness: `tests::ui_options`
fn check_declaration<'tcx>(
    cx: &LateContext<'tcx>,
    declaration: &'tcx FnDecl<'tcx>,
    owner: LocalDefId,
)
{
    if cx
        .tcx
        .lint_level_spec_at_node(OPTION_SIGNATURE, cx.tcx.local_def_id_to_hir_id(owner))
        .level()
        == rustc_session::lint::Level::Allow
    {
        return;
    }
    let actual = cx
        .tcx
        .fn_sig(owner)
        .instantiate_identity()
        .skip_norm_wip()
        .skip_binder();
    let foreign = cx
        .tcx
        .opt_associated_item(owner.to_def_id())
        .and_then(|item| item.trait_item_def_id())
        .filter(|item| !item.is_local())
        .map(|item| {
            cx.tcx
                .fn_sig(item)
                .instantiate_identity()
                .skip_norm_wip()
                .skip_binder()
        });
    for (index, (syntax, actual)) in declaration.inputs.iter().zip(actual.inputs()).enumerate() {
        let required = foreign
            .and_then(|signature| signature.inputs().get(index).copied())
            .map_or(Required::Authored, Required::Foreign);
        if matches!(exposure(cx, *actual, required), Exposure::Option) {
            emit(cx, syntax.span);
        }
    }
    if let FnRetTy::Return(syntax) = declaration.output {
        let required = foreign.map_or(Required::Authored, |signature| {
            Required::Foreign(signature.output())
        });
        if matches!(exposure(cx, actual.output(), required), Exposure::Option) {
            emit(cx, syntax.span);
        }
    }
}

/// Traverse normalized structure without opening nominal representation fields.
///
/// # Specification
/// - ensures: required outer Option layers do not exempt substituted children.
/// - ensures: local transparent ADTs and the canonical reason-bearing Maybe
///   terminate traversal; other ADT arguments and function signatures are
///   inspected.
/// - ensures: opaque bounds expose associated output types when rustc retains
///   them.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 UI contrasts aliases and structural nesting with nominal
///   leaves, foreign generic substitution, and future output bounds.
/// - witness: `tests::ui_options`
fn exposure<'tcx>(
    cx: &LateContext<'tcx>,
    actual: ty::Ty<'tcx>,
    required: Required<'tcx>,
) -> Exposure
{
    let mut pending = vec![(actual, required)];
    let mut visited = std::collections::HashSet::new();
    while let Some((actual, required)) = pending.pop() {
        let actual = normalize_middle_ty(cx, actual);
        let required_ty = match required {
            | Required::Foreign(required) => Some(required),
            | Required::Authored => None,
        };
        if !visited.insert((actual, required_ty)) {
            continue;
        }
        match *actual.kind() {
            | ty::Adt(adt, arguments) => {
                let is_option = cx.tcx.is_diagnostic_item(sym::Option, adt.did());
                let required_arguments = required_ty.and_then(|required| match *required.kind() {
                    | ty::Adt(required_adt, arguments) if required_adt.did() == adt.did() => {
                        Some(arguments)
                    },
                    | _ => None,
                });
                if is_option && required_arguments.is_none() {
                    return Exposure::Option;
                }
                if !is_option && matches!(nominal_boundary(cx, adt), NominalBoundary::Reached) {
                    continue;
                }
                push_arguments(
                    &mut pending,
                    arguments,
                    required_arguments
                        .map_or(RequiredArguments::Authored, RequiredArguments::Foreign),
                );
            },
            | ty::Ref(_, inner, _)
            | ty::RawPtr(inner, _)
            | ty::Array(inner, _)
            | ty::Slice(inner)
            | ty::Pat(inner, _) => {
                let required = required_ty
                    .and_then(|required| match *required.kind() {
                        | ty::Ref(_, inner, _)
                        | ty::RawPtr(inner, _)
                        | ty::Array(inner, _)
                        | ty::Slice(inner)
                        | ty::Pat(inner, _) => Some(inner),
                        | _ => None,
                    })
                    .map_or(Required::Authored, Required::Foreign);
                pending.push((inner, required));
            },
            | ty::Tuple(types) => {
                let required = required_ty.and_then(|required| match *required.kind() {
                    | ty::Tuple(types) => Some(types),
                    | _ => None,
                });
                for (index, actual) in types.iter().enumerate() {
                    let required = required
                        .and_then(|types| types.get(index).copied())
                        .map_or(Required::Authored, Required::Foreign);
                    pending.push((actual, required));
                }
            },
            | ty::FnPtr(signature, header) => {
                let signature = signature.with(header).skip_binder();
                let required = required_ty.and_then(|required| match *required.kind() {
                    | ty::FnPtr(signature, header) => Some(signature.with(header).skip_binder()),
                    | _ => None,
                });
                for (index, actual) in signature.inputs_and_output.iter().enumerate() {
                    let required = required
                        .and_then(|signature| signature.inputs_and_output.get(index).copied())
                        .map_or(Required::Authored, Required::Foreign);
                    pending.push((actual, required));
                }
            },
            | ty::Alias(ty::AliasTy {
                kind: ty::Opaque { def_id },
                args,
                ..
            }) => {
                for (clause, _) in cx
                    .tcx
                    .explicit_item_bounds(def_id)
                    .iter_instantiated_copied(cx.tcx, args)
                    .map(ty::Unnormalized::skip_norm_wip)
                {
                    if let Some(projection) = clause.as_projection_clause()
                        && let Some(output) = projection.skip_binder().term.as_type()
                    {
                        let required =
                            required_projection(cx, required, projection.skip_binder().def_id());
                        pending.push((output, required));
                    }
                    if let Some(bound) = clause.as_trait_clause() {
                        let bound = bound.skip_binder().trait_ref;
                        let required = required_trait(cx, required, bound.def_id);
                        push_arguments(&mut pending, bound.args, required);
                    }
                }
            },
            | ty::Dynamic(predicates, _) => {
                for predicate in predicates {
                    if let ty::ExistentialPredicate::Projection(projection) =
                        predicate.skip_binder()
                        && let Some(output) = projection.term.as_type()
                    {
                        let required = required_projection(cx, required, projection.def_id);
                        pending.push((output, required));
                    }
                    if let ty::ExistentialPredicate::Trait(bound) = predicate.skip_binder() {
                        let required = required_trait(cx, required, bound.def_id);
                        push_arguments(&mut pending, bound.args, required);
                    }
                }
            },
            | _ => {},
        }
    }
    Exposure::NotFound
}

/// Match a foreign bound by associated-item identity without substituting its
/// output.
///
/// # Specification
/// - ensures: only the same associated projection supplies an external required
///   shape; generic or associated substitutions retain their authored boundary.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 foreign dynamic and future outputs distinguish required
///   Option layers from layers introduced by a generic argument.
/// - witness: `tests::ui_options`
fn required_projection<'tcx>(
    cx: &LateContext<'tcx>,
    required: Required<'tcx>,
    associated: rustc_hir::def_id::DefId,
) -> Required<'tcx>
{
    let Required::Foreign(required) = required
    else {
        return Required::Authored;
    };
    match *required.kind() {
        | ty::Dynamic(predicates, _) => {
            for predicate in predicates {
                if let ty::ExistentialPredicate::Projection(projection) = predicate.skip_binder()
                    && projection.def_id == associated
                    && let Some(output) = projection.term.as_type()
                {
                    return Required::Foreign(output);
                }
            }
        },
        | ty::Alias(ty::AliasTy {
            kind: ty::Opaque { def_id } | ty::Projection { def_id },
            args,
            ..
        }) => {
            for (clause, _) in cx
                .tcx
                .explicit_item_bounds(def_id)
                .iter_instantiated_copied(cx.tcx, args)
                .map(ty::Unnormalized::skip_norm_wip)
            {
                if let Some(projection) = clause.as_projection_clause()
                    && projection.skip_binder().def_id() == associated
                    && let Some(output) = projection.skip_binder().term.as_type()
                {
                    return Required::Foreign(output);
                }
            }
        },
        | _ => {},
    }
    Required::Authored
}

/// Generic arguments retained from an unsubstituted foreign bound.
#[derive(Clone, Copy)]
enum RequiredArguments<'tcx>
{
    /// No corresponding foreign type constructor exists.
    Authored,
    /// The foreign constructor supplies these argument positions.
    Foreign(ty::GenericArgsRef<'tcx>),
}

/// Pair generic arguments by position within the same resolved constructor.
///
/// # Specification
/// - ensures: only type arguments enter the traversal; absent counterpart
///   positions remain authored rather than becoming foreign exemptions.
/// - panics: none.
fn push_arguments<'tcx>(
    pending: &mut Vec<(ty::Ty<'tcx>, Required<'tcx>)>,
    actual: ty::GenericArgsRef<'tcx>,
    required: RequiredArguments<'tcx>,
)
{
    for (index, argument) in actual.iter().enumerate() {
        if let Some(actual) = argument.as_type() {
            let required = match required {
                | RequiredArguments::Foreign(arguments) => arguments
                    .get(index)
                    .and_then(|argument| argument.as_type())
                    .map_or(Required::Authored, Required::Foreign),
                | RequiredArguments::Authored => Required::Authored,
            };
            pending.push((actual, required));
        }
    }
}

/// Find the same trait constructor in a foreign opaque or dynamic bound.
///
/// # Specification
/// - ensures: a same-spelled different trait cannot supply required arguments.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 dynamic and opaque generic-argument witnesses distinguish
///   foreign-required absence from absence introduced by substitution.
/// - witness: `tests::ui_options`
fn required_trait<'tcx>(
    cx: &LateContext<'tcx>,
    required: Required<'tcx>,
    trait_id: rustc_hir::def_id::DefId,
) -> RequiredArguments<'tcx>
{
    let Required::Foreign(required) = required
    else {
        return RequiredArguments::Authored;
    };
    match *required.kind() {
        | ty::Dynamic(predicates, _) => {
            for predicate in predicates {
                if let ty::ExistentialPredicate::Trait(bound) = predicate.skip_binder()
                    && bound.def_id == trait_id
                {
                    return RequiredArguments::Foreign(bound.args);
                }
            }
        },
        | ty::Alias(ty::AliasTy {
            kind: ty::Opaque { def_id } | ty::Projection { def_id },
            args,
            ..
        }) => {
            for (clause, _) in cx
                .tcx
                .explicit_item_bounds(def_id)
                .iter_instantiated_copied(cx.tcx, args)
                .map(ty::Unnormalized::skip_norm_wip)
            {
                if let Some(bound) = clause.as_trait_clause()
                    && bound.skip_binder().trait_ref.def_id == trait_id
                {
                    return RequiredArguments::Foreign(bound.skip_binder().trait_ref.args);
                }
            }
        },
        | _ => {},
    }
    RequiredArguments::Authored
}

/// Nominal representations terminate structural absence inspection.
enum NominalBoundary
{
    /// The nominal boundary is recognized.
    Reached,
    /// Generic arguments still belong to the inspected structure.
    InspectArguments,
}

/// Recognize local transparent wrappers and the canonical reason-bearing enum.
///
/// # Specification
/// - ensures: field layouts are never traversed; a same-spelled foreign Maybe
///   is not confused with the Quenchant definition.
/// - ensures: aliases and re-exports retain the producer's diagnostic-item
///   `DefId`; an unmarked dependency receives no canonical exemption.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 same-crate-name rlibs with distinct metadata distinguish
///   diagnostic-item identity from spelling; aliases retain real identity.
/// - witness: `tests::ui_options`
fn nominal_boundary(
    cx: &LateContext<'_>,
    adt: ty::AdtDef<'_>,
) -> NominalBoundary
{
    if adt.did().is_local() && adt.repr().transparent() {
        return NominalBoundary::Reached;
    }
    if cx
        .tcx
        .is_diagnostic_item(Symbol::intern("quenchant_maybe"), adt.did())
    {
        NominalBoundary::Reached
    }
    else {
        NominalBoundary::InspectArguments
    }
}

/// Anchor the observable absence-policy diagnostic to its signature position.
///
/// # Specification
/// trivial.
fn emit(
    cx: &LateContext<'_>,
    span: Span,
)
{
    clippy_utils::diagnostics::span_lint(
        cx,
        OPTION_SIGNATURE,
        span,
        "crate-defined signatures must preserve the reason for absence instead of exposing Option",
    );
}
