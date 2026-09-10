//! Primitive exposure before a declared local transparent boundary.
//!
//! HIR preserves how a signature was written; normalized compiler types expose
//! aliases and substitutions. Walking them together prevents either spelling
//! from hiding a primitive inside references, pointers, aggregates, function
//! signatures, or inspected generic arguments.
//!
//! The first local transparent ADT stops this traversal. That is a structural
//! policy boundary, not evidence that its name or domain meaning is adequate.
//! Async outputs also need their declared HIR view when normalization exposes
//! only an opaque future. An explicit worklist bounds traversal by the visited
//! type structure instead of the host stack's recursion depth.

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

/// Explicit continuation state preserves signature traversal and diagnostic
/// order.
enum PrimitiveWork<'tcx>
{
    /// Source spelling remains paired with its instantiated type.
    SemanticTy(&'tcx Ty<'tcx>, rustc_ty::Ty<'tcx>),
    /// Declared parameters remain paired with the instantiated function
    /// signature.
    SemanticFnDecl(&'tcx FnDecl<'tcx>, rustc_ty::FnSig<'tcx>),
    /// Source-only inspection preserves a declared type hidden by
    /// normalization.
    HirTy(&'tcx Ty<'tcx>),
    /// Function-pointer syntax is traversed without a substituted signature.
    HirFnDecl(&'tcx FnDecl<'tcx>),
    /// An opaque future still exposes a declared output for inspection.
    Opaque(&'tcx OpaqueTy<'tcx>),
    /// Source-level output inspection follows generic arguments across opaque
    /// normalization.
    ///
    /// An async return's normalized future hides the declared output. This path
    /// therefore applies the boundary rule directly to HIR: only a local
    /// transparent type stops descent. A nominal container such as `Option<u8>`
    /// cannot hide its primitive argument.
    OpaqueOutputTy(&'tcx Ty<'tcx>),
    /// A path-level diagnostic remains available if no descendant supplied one.
    PathFallback(&'tcx Ty<'tcx>, DiagnosticCount),
}

/// Signature inspection follows inputs and explicit outputs until a declared
/// semantic boundary.
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
                        // economy: positional pairing can misidentify a descendant span when
                        // aliases reorder or discard arguments, as `type Flip<A, B> = Result<B, A>`
                        // does. `PathFallback` preserves the denial when descendants supply none.
                        // Alias-aware substitution would improve span precision for alias-heavy
                        // inputs.
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

/// The first trait bound must identify the language future before output
/// inspection applies.
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

/// A single associated-type equality exposes the future's declared output.
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

/// Normalized ADT arguments and tuple components expose substituted child
/// types.
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

/// Final-segment syntax determines which generic arguments were written on a
/// path.
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

/// The primitive policy fixes the scalar and string types that require a
/// nominal boundary.
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

/// A locally declared transparent ADT terminates primitive exposure analysis.
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

/// Source-path resolution applies the same local transparent boundary as
/// semantic types.
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

/// Primitive exposure is detected before traversal reaches an admitted nominal
/// boundary.
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

/// Typing-context normalization exposes aliases and projections where rustc can
/// resolve them.
///
/// Ownership analysis uses this same interpretation before classifying field
/// types. Sharing normalization prevents aliases from acquiring different
/// meanings in the signature and ownership analyses.
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

/// The selected source span identifies the primitive exposure being reported.
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
