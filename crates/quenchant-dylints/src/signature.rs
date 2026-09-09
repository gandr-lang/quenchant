//! The signature plane: primitives exposed before a nominal boundary.
//!
//! The traversal walks a function declaration's declared HIR types beside their
//! substituted `rustc_middle` types, descending through references, pointers,
//! tuples, arrays, slices, function pointers, generic arguments, and type
//! aliases. It stops at the first local `#[repr(transparent)]` ADT, which is
//! the workspace's nominal wrapper boundary.
//!
//! The traversal is an explicit worklist so it never recurses over the depth of
//! a user-written type.

use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// Only the first trait bound participates in this opaque-future projection.
    mod future_bound {
        /// Why no recognized future reference is available.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The opaque type has no trait bound.
            NoTraitBound,
            /// Its first trait bound is not the language's Future trait.
            FirstTraitNotFuture,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Output inspection accepts exactly one associated-type equality.
    mod future_output {
        /// Why the trait reference supplies no inspectable output type.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// No final path segment exists.
            NoSegment,
            /// The final segment carries no generic arguments.
            NoArguments,
            /// The argument list does not contain exactly one constraint.
            ConstraintCount,
            /// The sole constraint names something other than Output.
            NotOutput,
            /// The Output constraint does not supply a type.
            NotType,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Generic traversal needs the syntax on the final path segment.
    mod path_arguments {
        /// Why this path has no argument list.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// A resolved path has no final segment.
            NoSegment,
            /// Its final segment carries no arguments.
            NoArguments,
        }
    }
}

use clippy_utils::diagnostics::span_lint;
use rustc_hir::FnDecl;
use rustc_hir::FnRetTy;
use rustc_hir::GenericArg;
use rustc_hir::GenericArgs;
use rustc_hir::GenericBound;
use rustc_hir::OpaqueTy;
use rustc_hir::PrimTy;
use rustc_hir::QPath;
use rustc_hir::TraitRef;
use rustc_hir::Ty;
use rustc_hir::TyKind;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_lint::LateContext;
use rustc_middle::ty as rustc_ty;
use rustc_span::Span;
use rustc_span::symbol::sym;

use crate::PRIMITIVE_SIGNATURE;
use crate::semantic::ContainsPrimitive;
use crate::semantic::DiagnosticCount;
use crate::semantic::DisallowedPrimitive;
use crate::semantic::PrimitiveDiagnosticEmitted;
use crate::semantic::SemanticBoundaryAdt;

/// One work item of the order-preserving primitive-signature traversal.
enum PrimitiveWork<'tcx>
{
    /// A HIR type paired with its substituted semantic type.
    SemanticTy(&'tcx Ty<'tcx>, rustc_ty::Ty<'tcx>),
    /// A HIR function declaration paired with its semantic signature.
    SemanticFnDecl(&'tcx FnDecl<'tcx>, rustc_ty::FnSig<'tcx>),
    /// A HIR type checked syntactically because semantic normalization hid the
    /// declared type, as async function signatures do.
    HirTy(&'tcx Ty<'tcx>),
    /// A HIR-only function pointer declaration.
    HirFnDecl(&'tcx FnDecl<'tcx>),
    /// An opaque async return type whose declared `Future::Output` is checked.
    Opaque(&'tcx OpaqueTy<'tcx>),
    /// A declared `Future::Output` type, checked syntactically and descended
    /// through its type arguments.
    ///
    /// Normalization hides an async signature's declared output behind the
    /// opaque future, so there is no substituted type to walk beside this one.
    /// The traversal therefore applies the nominal-boundary rule to the HIR
    /// path itself: it stops at a local `#[repr(transparent)]` type and
    /// otherwise keeps descending, so `Option<u8>` is not accepted merely
    /// because its head is nominal.
    OpaqueOutputTy(&'tcx Ty<'tcx>),
    /// Emit at a path node when no generic-argument descendant emitted.
    PathFallback(&'tcx Ty<'tcx>, DiagnosticCount),
}

/// Check every input and explicit output type in one function declaration.
///
/// # Specification
/// - requires: `fn_decl` and `fn_sig` describe the same function, `fn_sig`
///   instantiated with identity arguments.
/// - ensures: one diagnostic is emitted per offending HIR span, in declaration
///   order; a type whose substituted form reaches no disallowed primitive
///   before a local transparent ADT emits nothing.
/// - provides: the answer to whether the declaration exposes a primitive.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the traversal is a loop over an explicit worklist.
/// - measure: the total size of the HIR and middle type trees still on the
///   worklist.
/// - boundedness: every push replaces one item with strict subterms of it, and
///   type trees are finite.
/// - input recursion: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates direct primitives, primitives
///   under each structural layer, primitives behind aliases and generic
///   containers, and nominal transparent leaves that must pass.
/// - witness: `tests::ui`
pub fn check_fn_decl<'tcx>(
    cx: &LateContext<'tcx>,
    fn_decl: &'tcx FnDecl<'tcx>,
    fn_sig: rustc_ty::FnSig<'tcx>,
) -> PrimitiveDiagnosticEmitted
{
    let mut work = vec![PrimitiveWork::SemanticFnDecl(fn_decl, fn_sig)];
    let mut diagnostics = DiagnosticCount(0_usize);
    while let Some(item) = work.pop() {
        match item {
            | PrimitiveWork::SemanticTy(ty, semantic_ty) => {
                let semantic_ty = normalize_middle_ty(cx, semantic_ty);
                if !middle_ty_contains_primitive(cx, semantic_ty).0 {
                    work.push(PrimitiveWork::HirTy(ty));
                    continue;
                }
                match ty.kind {
                    | TyKind::Slice(inner) | TyKind::Array(inner, _) => match *semantic_ty.kind() {
                        | rustc_ty::Slice(semantic_inner) | rustc_ty::Array(semantic_inner, _) => {
                            work.push(PrimitiveWork::SemanticTy(inner, semantic_inner));
                        },
                        | _ => {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        },
                    },
                    | TyKind::Ptr(mut_ty) => {
                        if let rustc_ty::RawPtr(semantic_inner, _) = *semantic_ty.kind() {
                            work.push(PrimitiveWork::SemanticTy(mut_ty.ty, semantic_inner));
                        }
                        else {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        }
                    },
                    | TyKind::Ref(_, mut_ty) => {
                        if let rustc_ty::Ref(_, semantic_inner, _) = *semantic_ty.kind() {
                            work.push(PrimitiveWork::SemanticTy(mut_ty.ty, semantic_inner));
                        }
                        else {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        }
                    },
                    | TyKind::FnPtr(fn_ptr) => {
                        if let rustc_ty::FnPtr(sig_tys, header) = *semantic_ty.kind() {
                            work.push(PrimitiveWork::SemanticFnDecl(
                                fn_ptr.decl,
                                sig_tys.with(header).skip_binder(),
                            ));
                        }
                        else {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        }
                    },
                    | TyKind::Tup(types) => match *semantic_ty.kind() {
                        | rustc_ty::Tuple(semantic_types) if semantic_types.len() == types.len() => {
                            for (inner, semantic_inner) in
                                types.iter().zip(semantic_types.iter()).rev()
                            {
                                work.push(PrimitiveWork::SemanticTy(inner, semantic_inner));
                            }
                        },
                        | _ => {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        },
                    },
                    | TyKind::Path(ref qpath) => {
                        work.push(PrimitiveWork::PathFallback(ty, diagnostics));
                        // economy: HIR arguments are zipped positionally with
                        // the substituted ones, which a type alias can reorder
                        // or drop (`type Flip<A, B> = Result<B, A>`). A wrong
                        // pairing costs diagnostic precision — the span may name
                        // the sibling argument — but not the verdict, because
                        // `PathFallback` still reports the path itself when no
                        // descendant does. Mapping through the alias's own
                        // generics is the upgrade if alias-heavy code arrives.
                        if let Maybe::Present(generic_args) = last_segment_args(qpath) {
                            let semantic_args = semantic_type_args(cx, semantic_ty);
                            let mut pairs: Vec<_> = generic_args
                                .args
                                .iter()
                                .filter_map(|arg| match arg {
                                    | &GenericArg::Type(generic_ty) => {
                                        Some(generic_ty.as_unambig_ty())
                                    },
                                    | _ => None,
                                })
                                .zip(semantic_args)
                                .collect();
                            while let Some((hir_ty, semantic_arg)) = pairs.pop() {
                                work.push(PrimitiveWork::SemanticTy(hir_ty, semantic_arg));
                            }
                        }
                    },
                    | TyKind::Pat(inner, _) | TyKind::FieldOf(inner, _) => {
                        work.push(PrimitiveWork::SemanticTy(inner, semantic_ty));
                    },
                    | TyKind::OpaqueDef(opaque) => {
                        work.push(PrimitiveWork::Opaque(opaque));
                    },
                    | TyKind::InferDelegation(_)
                    | TyKind::UnsafeBinder(_)
                    | TyKind::Never
                    | TyKind::TraitAscription(_)
                    | TyKind::TraitObject(..)
                    | TyKind::Err(_)
                    | TyKind::Infer(()) => {},
                }
            },
            | PrimitiveWork::SemanticFnDecl(decl, sig) => {
                if let FnRetTy::Return(output) = decl.output {
                    work.push(PrimitiveWork::SemanticTy(output, sig.output()));
                }
                for (input, semantic_input) in decl.inputs.iter().zip(sig.inputs()).rev() {
                    work.push(PrimitiveWork::SemanticTy(input, *semantic_input));
                }
            },
            | PrimitiveWork::HirTy(ty) => match ty.kind {
                | TyKind::Slice(inner)
                | TyKind::Array(inner, _)
                | TyKind::Pat(inner, _)
                | TyKind::FieldOf(inner, _) => {
                    work.push(PrimitiveWork::HirTy(inner));
                },
                | TyKind::Ptr(mut_ty) | TyKind::Ref(_, mut_ty) => {
                    work.push(PrimitiveWork::HirTy(mut_ty.ty));
                },
                | TyKind::FnPtr(fn_ptr) => {
                    work.push(PrimitiveWork::HirFnDecl(fn_ptr.decl));
                },
                | TyKind::Tup(types) => {
                    for inner in types.iter().rev() {
                        work.push(PrimitiveWork::HirTy(inner));
                    }
                },
                | TyKind::Path(ref qpath) => {
                    if let Res::PrimTy(primitive) = cx.qpath_res(qpath, ty.hir_id)
                        && is_disallowed_primitive(primitive).0
                    {
                        emit_primitive(cx, ty.span);
                        diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                    }
                },
                | TyKind::OpaqueDef(opaque) => {
                    work.push(PrimitiveWork::Opaque(opaque));
                },
                | TyKind::InferDelegation(_)
                | TyKind::UnsafeBinder(_)
                | TyKind::Never
                | TyKind::TraitAscription(_)
                | TyKind::TraitObject(..)
                | TyKind::Err(_)
                | TyKind::Infer(()) => {},
            },
            | PrimitiveWork::HirFnDecl(decl) => {
                if let FnRetTy::Return(output) = decl.output {
                    work.push(PrimitiveWork::HirTy(output));
                }
                for input in decl.inputs.iter().rev() {
                    work.push(PrimitiveWork::HirTy(input));
                }
            },
            | PrimitiveWork::Opaque(opaque) => {
                if let Maybe::Present(trait_ref) = future_trait_ref(cx, opaque)
                    && let Maybe::Present(output) = future_output_ty(trait_ref)
                {
                    work.push(PrimitiveWork::OpaqueOutputTy(output));
                }
            },
            | PrimitiveWork::OpaqueOutputTy(ty) => match ty.kind {
                | TyKind::Slice(inner)
                | TyKind::Array(inner, _)
                | TyKind::Pat(inner, _)
                | TyKind::FieldOf(inner, _) => {
                    work.push(PrimitiveWork::OpaqueOutputTy(inner));
                },
                | TyKind::Ptr(mut_ty) | TyKind::Ref(_, mut_ty) => {
                    work.push(PrimitiveWork::OpaqueOutputTy(mut_ty.ty));
                },
                | TyKind::FnPtr(fn_ptr) => {
                    work.push(PrimitiveWork::HirFnDecl(fn_ptr.decl));
                },
                | TyKind::Tup(types) => {
                    for inner in types.iter().rev() {
                        work.push(PrimitiveWork::OpaqueOutputTy(inner));
                    }
                },
                | TyKind::Path(ref qpath) => {
                    let resolution = cx.qpath_res(qpath, ty.hir_id);
                    if let Res::PrimTy(primitive) = resolution {
                        if is_disallowed_primitive(primitive).0 {
                            emit_primitive(cx, ty.span);
                            diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                        }
                    }
                    else if !resolves_to_semantic_boundary(cx, resolution).0
                        && let Maybe::Present(generic_args) = last_segment_args(qpath)
                    {
                        for arg in generic_args.args.iter().rev() {
                            if let &GenericArg::Type(generic_ty) = arg {
                                work.push(PrimitiveWork::OpaqueOutputTy(
                                    generic_ty.as_unambig_ty(),
                                ));
                            }
                        }
                    }
                },
                | TyKind::OpaqueDef(opaque) => {
                    work.push(PrimitiveWork::Opaque(opaque));
                },
                | TyKind::InferDelegation(_)
                | TyKind::UnsafeBinder(_)
                | TyKind::Never
                | TyKind::TraitAscription(_)
                | TyKind::TraitObject(..)
                | TyKind::Err(_)
                | TyKind::Infer(()) => {},
            },
            | PrimitiveWork::PathFallback(ty, snapshot) => {
                if diagnostics == snapshot {
                    emit_primitive(cx, ty.span);
                    diagnostics.0 = diagnostics.0.saturating_add(1_usize);
                }
            },
        }
    }
    PrimitiveDiagnosticEmitted(diagnostics.0 > 0_usize)
}

/// Return the `Future` bound on an opaque async return type, when present.
///
/// # Specification
/// - ensures: returns the first trait bound of the opaque type exactly when it
///   is the language's `Future`, and nothing otherwise.
/// - provides: `future_bound::Missing::NoTraitBound` means no trait occurs;
///   `FirstTraitNotFuture` means the first one has a different identity.
/// - panics: none.
fn future_trait_ref<'tcx>(
    cx: &LateContext<'tcx>,
    opaque: &'tcx OpaqueTy<'tcx>,
) -> Maybe<&'tcx TraitRef<'tcx>, future_bound::Missing>
{
    let Some(trait_ref) = opaque.bounds.iter().find_map(|bound| match *bound {
        | GenericBound::Trait(ref poly) => Some(&poly.trait_ref),
        | _ => None,
    })
    else {
        return Maybe::Absent(future_bound::Missing::NoTraitBound);
    };
    if trait_ref.trait_def_id() == cx.tcx.lang_items().future_trait() {
        return Maybe::Present(trait_ref);
    }
    Maybe::Absent(future_bound::Missing::FirstTraitNotFuture)
}

/// Return the declared `Future::Output` type from a future trait reference.
///
/// # Specification
/// - ensures: returns the type bound to `Output` exactly when the reference's
///   last segment carries that one associated-type constraint.
/// - provides: `future_output::Missing` distinguishes absent path syntax
///   (`NoSegment`, `NoArguments`), a non-singleton `ConstraintCount`, a
///   differently named constraint (`NotOutput`), and a non-type one
///   (`NotType`).
/// - panics: none.
fn future_output_ty<'tcx>(
    trait_ref: &'tcx TraitRef<'tcx>
) -> Maybe<&'tcx Ty<'tcx>, future_output::Missing>
{
    let Some(segment) = trait_ref.path.segments.last()
    else {
        return Maybe::Absent(future_output::Missing::NoSegment);
    };
    let Some(args) = segment.args
    else {
        return Maybe::Absent(future_output::Missing::NoArguments);
    };
    let [ref constraint] = *args.constraints
    else {
        return Maybe::Absent(future_output::Missing::ConstraintCount);
    };
    if constraint.ident.name != sym::Output {
        return Maybe::Absent(future_output::Missing::NotOutput);
    }
    match constraint.ty() {
        | Some(output) => Maybe::Present(output),
        | None => Maybe::Absent(future_output::Missing::NotType),
    }
}

/// Return substituted semantic type arguments represented by a path type.
///
/// # Specification
/// - ensures: returns the type arguments of a normalized ADT and the components
///   of a tuple; every other type kind yields none.
/// - panics: none.
fn semantic_type_args<'tcx>(
    cx: &LateContext<'tcx>,
    semantic_ty: rustc_ty::Ty<'tcx>,
) -> Vec<rustc_ty::Ty<'tcx>>
{
    let semantic_ty = normalize_middle_ty(cx, semantic_ty);
    match *semantic_ty.kind() {
        | rustc_ty::Adt(_, args) => args
            .iter()
            .filter_map(rustc_ty::GenericArg::as_type)
            .collect(),
        | rustc_ty::Tuple(types) => types.iter().collect(),
        | _ => Vec::new(),
    }
}

/// Return the last path segment's generic arguments, if any.
///
/// # Specification
/// - ensures: returns the arguments written on the path's last segment, for a
///   resolved and a type-relative path alike.
/// - provides: `path_arguments::Missing::NoSegment` means a resolved path is
///   empty; `NoArguments` means a final segment exists without arguments.
/// - panics: none.
fn last_segment_args<'hir>(
    qpath: &QPath<'hir>
) -> Maybe<&'hir GenericArgs<'hir>, path_arguments::Missing>
{
    let segment = match *qpath {
        | QPath::Resolved(_, path) => {
            let Some(segment) = path.segments.last()
            else {
                return Maybe::Absent(path_arguments::Missing::NoSegment);
            };
            segment
        },
        | QPath::TypeRelative(_, segment) => segment,
    };
    match segment.args {
        | Some(arguments) => Maybe::Present(arguments),
        | None => Maybe::Absent(path_arguments::Missing::NoArguments),
    }
}

/// Return whether `primitive` is one of the policy's banned signature
/// primitives.
///
/// # Specification
/// - ensures: answers affirmatively for `bool`, `char`, every integer and float
///   width, and `str`.
/// - provides: the banned set the whole traversal is keyed on.
/// - panics: none.
fn is_disallowed_primitive(primitive: PrimTy) -> DisallowedPrimitive
{
    DisallowedPrimitive(matches!(
        primitive,
        PrimTy::Bool
            | PrimTy::Char
            | PrimTy::Int(_)
            | PrimTy::Uint(_)
            | PrimTy::Float(_)
            | PrimTy::Str
    ))
}

/// Return whether an ADT is a semantic wrapper boundary for type-boundary
/// linting.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the ADT is defined in this
///   crate and declares a transparent representation.
/// - provides: the boundary a signature traversal stops at.
/// - panics: none.
fn is_semantic_boundary_adt(adt: rustc_ty::AdtDef<'_>) -> SemanticBoundaryAdt
{
    SemanticBoundaryAdt(adt.did().is_local() && adt.repr().transparent())
}

/// Return whether a resolved HIR path names a semantic wrapper boundary.
///
/// # Specification
/// - requires: `resolution` is the resolution of a type path.
/// - ensures: applies the same rule as [`is_semantic_boundary_adt`] without a
///   substituted type, so a syntactic walk cuts where a semantic one would.
/// - provides: the boundary rule for the async-output traversal, which has no
///   substituted type to walk beside it.
/// - panics: none.
fn resolves_to_semantic_boundary(
    cx: &LateContext<'_>,
    resolution: Res,
) -> SemanticBoundaryAdt
{
    let Res::Def(DefKind::Struct | DefKind::Enum | DefKind::Union, def_id) = resolution
    else {
        return SemanticBoundaryAdt(false);
    };
    if !def_id.is_local() {
        return SemanticBoundaryAdt(false);
    }
    is_semantic_boundary_adt(cx.tcx.adt_def(def_id))
}

/// Return whether a substituted semantic type contains a disallowed primitive
/// before a nominal workspace boundary.
///
/// # Specification
/// - requires: `ty` is a substituted `rustc_middle` type in `cx`'s typing
///   environment.
/// - ensures: answers affirmatively exactly when some structural descendant of
///   `ty`, cut at the first local transparent ADT, is a banned primitive.
/// - provides: the pre-filter that keeps the HIR traversal off accepted types.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the walk is a loop over an explicit worklist.
/// - measure: the total size of the middle type trees still pending.
/// - boundedness: each pop pushes only strict subterms, and middle types are
///   finite trees once aliases are normalized.
/// - input recursion: none.
fn middle_ty_contains_primitive<'tcx>(
    cx: &LateContext<'tcx>,
    ty: rustc_ty::Ty<'tcx>,
) -> ContainsPrimitive
{
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        let ty = normalize_middle_ty(cx, ty);
        match *ty.kind() {
            | rustc_ty::Bool
            | rustc_ty::Char
            | rustc_ty::Int(_)
            | rustc_ty::Uint(_)
            | rustc_ty::Float(_)
            | rustc_ty::Str => return ContainsPrimitive(true),
            | rustc_ty::Array(inner, _)
            | rustc_ty::Pat(inner, _)
            | rustc_ty::Slice(inner)
            | rustc_ty::RawPtr(inner, _)
            | rustc_ty::Ref(_, inner, _) => pending.push(inner),
            | rustc_ty::Tuple(types) => pending.extend(types.iter()),
            | rustc_ty::FnPtr(sig_tys, header) => {
                let sig = sig_tys.with(header).skip_binder();
                pending.push(sig.output());
                pending.extend(sig.inputs().iter().rev());
            },
            | rustc_ty::Adt(adt, _) if is_semantic_boundary_adt(adt).0 => {},
            | rustc_ty::Adt(_, args) => {
                pending.extend(args.iter().filter_map(rustc_ty::GenericArg::as_type));
            },
            | _ => {},
        }
    }
    ContainsPrimitive(false)
}

/// Normalize aliases/projections where rustc can do so in this typing context.
///
/// Shared with the ownership plane, which walks the same substituted types and
/// needs the same alias handling: a type alias must be seen through before a
/// field's type can be classified.
///
/// # Specification
/// - ensures: returns the normalized type where rustc can normalize it in this
///   context, and the type unchanged otherwise.
/// - panics: none.
pub fn normalize_middle_ty<'tcx>(
    cx: &LateContext<'tcx>,
    ty: rustc_ty::Ty<'tcx>,
) -> rustc_ty::Ty<'tcx>
{
    cx.tcx
        .try_normalize_erasing_regions(cx.typing_env(), rustc_ty::Unnormalized::new_wip(ty))
        .unwrap_or(ty)
}

/// Emit the primitive signature diagnostic at `span`.
///
/// # Specification
/// trivial.
fn emit_primitive(
    cx: &LateContext<'_>,
    span: Span,
)
{
    span_lint(
        cx,
        PRIMITIVE_SIGNATURE,
        span,
        "signature exposes a Rust primitive before a semantic wrapper boundary",
    );
}
