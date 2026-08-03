//! Fat-pointer machinery behind down- and side-casts.
//!
//! Everything in here is `pub(crate)`; users reach it through the cast methods on
//! [`Obj`](crate::Obj), [`Ref`](crate::Ref) and [`RefMut`](crate::RefMut).

use core::any::TypeId;
use core::marker::PhantomData;

use crate::class::{AnyObj, Class};
use crate::meta::VTablePtr;

/// Compile-time proof that `*const D` really is a two-word fat pointer.
struct FatCheck<D: ?Sized>(PhantomData<D>);

impl<D: ?Sized> FatCheck<D> {
    const OK: () = assert!(
        core::mem::size_of::<*const D>() == 2 * core::mem::size_of::<usize>(),
        "obj: a class's `Dyn` interface must be a `dyn Trait` (a two-word pointer)",
    );
}

/// Rebuilds a fat pointer from a data address and a vtable.
///
/// # Safety
///
/// `vtable` must have been captured from the concrete type that actually lives at `data`, coerced
/// to `D`. In practice it always comes from a [`BaseEntry`](crate::meta::BaseEntry) belonging to
/// that object's own [`ClassMeta`](crate::meta::ClassMeta).
#[cfg(not(feature = "nightly"))]
pub(crate) unsafe fn rebuild_fat<D: ?Sized>(data: *const u8, vtable: VTablePtr) -> *const D {
    let () = FatCheck::<D>::OK;
    let parts: [*const (); 2] = [data.cast::<()>(), vtable.0];
    // SAFETY: `FatCheck` proves `*const D` is two words wide, and a `dyn` pointer is laid out as
    // (data, vtable). The caller guarantees the pair is coherent. This mirrors exactly how the
    // vtable was captured in `__vtable_of!`, so the round-trip is symmetric.
    unsafe { core::mem::transmute_copy::<[*const (); 2], *const D>(&parts) }
}

/// Rebuilds a fat pointer from a data address and a vtable.
///
/// # Safety
///
/// See the stable implementation above.
#[cfg(feature = "nightly")]
pub(crate) unsafe fn rebuild_fat<D: ?Sized>(data: *const u8, vtable: VTablePtr) -> *const D {
    let () = FatCheck::<D>::OK;
    // SAFETY: `D` is a `dyn` trait, so its pointer metadata is a one-word `DynMetadata`, which is
    // exactly what was captured into `vtable`.
    let meta: <D as core::ptr::Pointee>::Metadata = unsafe { core::mem::transmute_copy(&vtable.0) };
    core::ptr::from_raw_parts(data.cast::<()>(), meta)
}

/// Locates the `T` *data* subobject inside `src`, applying the recorded byte offset.
pub(crate) fn data_ptr_of<T: Class, S: AnyObj + ?Sized>(src: &S) -> Option<*const T> {
    let entry = src.class_meta().find_base(TypeId::of::<T>())?;
    // SAFETY: `entry` came from this object's own metadata, so `data_offset` is a valid in-bounds
    // offset from the start of the complete object to its `T` subobject.
    Some(unsafe { src.obj_addr().add(entry.data_offset).cast::<T>() })
}

/// Builds a *polymorphic* pointer to `src` viewed as `T`'s interface.
///
/// Note the deliberate absence of `data_offset`: Rust implements a `dyn` trait for the
/// most-derived type, so the data half must keep addressing the complete object. Applying the
/// offset here would silently dispatch to `T`'s own implementation instead of the override.
pub(crate) fn dyn_ptr_of<T: Class, S: AnyObj + ?Sized>(src: &S) -> Option<*const T::Dyn> {
    let entry = src.class_meta().find_base(TypeId::of::<T>())?;
    // `None` only for an abstract class's own table, which a live object never has.
    let vtable = entry.dyn_vtable?;
    // SAFETY: `vtable` was captured from this object's most-derived type coerced to `T::Dyn`, and
    // `obj_addr` is that object's address, so the pair is coherent.
    Some(unsafe { rebuild_fat::<T::Dyn>(src.obj_addr(), vtable) })
}

/// Returns whether `src`'s most-derived class is, or derives from, `T`.
pub(crate) fn is_a<T: Class, S: AnyObj + ?Sized>(src: &S) -> bool {
    src.class_meta().is_a(TypeId::of::<T>())
}
