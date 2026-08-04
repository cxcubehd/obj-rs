//! Fat-pointer machinery behind down- and side-casts.
//!
//! Most of this is `pub(crate)`; users reach it through the cast methods on
//! [`Obj`](crate::Obj), [`Ref`](crate::Ref) and [`RefMut`](crate::RefMut). The `subobject`
//! helpers are public because generated `Deref` impls call them.

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

/// Locates the `T` *data* subobject inside `src`, for writing.
///
/// Not just `data_ptr_of(..).cast_mut()`: a pointer derived from a shared reference carries
/// read-only provenance, so writing through it is undefined behaviour even though the caller holds
/// a unique borrow. The address still comes from [`AnyObj::obj_addr`], but the provenance is taken
/// from the unique borrow itself.
pub(crate) fn data_ptr_of_mut<T: Class, S: AnyObj + ?Sized>(src: &mut S) -> Option<*mut T> {
    let entry = src.class_meta().find_base(TypeId::of::<T>())?;
    let addr = src.obj_addr().addr();
    let base: *mut u8 = (src as *mut S).cast();
    // SAFETY: `addr` is the start of the very object `base` points at, so `with_addr` keeps the
    // pointer inside its own allocation; `entry` came from that object's own metadata, so
    // `data_offset` lands in bounds on its `T` subobject.
    Some(unsafe { base.with_addr(addr).add(entry.data_offset).cast::<T>() })
}

/// Looks up the vtable that views `src`'s most-derived class through `T`'s interface.
///
/// `None` if `src` does not derive from `T`, or — only for an abstract class's own table, which a
/// live object never has — if the entry records no vtable.
pub(crate) fn dyn_vtable_of<T: Class, S: AnyObj + ?Sized>(src: &S) -> Option<VTablePtr> {
    src.class_meta().find_base(TypeId::of::<T>())?.dyn_vtable
}

/// Builds a *polymorphic* pointer to `src` viewed as `T`'s interface.
///
/// Note the deliberate absence of `data_offset`: Rust implements a `dyn` trait for the
/// most-derived type, so the data half must keep addressing the complete object. Applying the
/// offset here would silently dispatch to `T`'s own implementation instead of the override.
pub(crate) fn dyn_ptr_of<T: Class, S: AnyObj + ?Sized>(src: &S) -> Option<*const T::Dyn> {
    let vtable = dyn_vtable_of::<T, S>(src)?;
    // SAFETY: `vtable` was captured from this object's most-derived type coerced to `T::Dyn`, and
    // `obj_addr` is that object's address, so the pair is coherent.
    Some(unsafe { rebuild_fat::<T::Dyn>(src.obj_addr(), vtable) })
}

/// Returns whether `src`'s most-derived class is, or derives from, `T`.
pub(crate) fn is_a<T: Class, S: AnyObj + ?Sized>(src: &S) -> bool {
    src.class_meta().is_a(TypeId::of::<T>())
}

/// Borrows the `T` subobject of an object known to derive from `T`.
///
/// This is how a polymorphic handle reaches a base class's *fields*: `Deref for dyn TDyn` resolves
/// the offset through the most-derived class's table. Doing it dynamically is what frees a class
/// from having to know the layout of ancestors it was never told about.
///
/// # Panics
///
/// If `src` does not derive from `T`. Generated code only ever calls this where the interface
/// itself proves the relationship, so this cannot fire from safe user code.
pub fn subobject<T: Class, S: AnyObj + ?Sized>(src: &S) -> &T {
    let ptr = data_ptr_of::<T, S>(src).unwrap_or_else(|| {
        panic!(
            "obj: `{}` does not derive from `{}`",
            src.class_meta().name,
            T::META.name,
        )
    });
    // SAFETY: `data_ptr_of` returns an in-bounds pointer to the live `T` subobject of `src`, whose
    // borrow we are reusing.
    unsafe { &*ptr }
}

/// Mutably borrows the `T` subobject of an object known to derive from `T`.
///
/// # Panics
///
/// See [`subobject`].
pub fn subobject_mut<T: Class, S: AnyObj + ?Sized>(src: &mut S) -> &mut T {
    let found = data_ptr_of_mut::<T, S>(src);
    let ptr = found.unwrap_or_else(|| {
        panic!(
            "obj: `{}` does not derive from `{}`",
            src.class_meta().name,
            T::META.name,
        )
    });
    // SAFETY: as `subobject`, and `data_ptr_of_mut` carried the unique borrow's provenance
    // through, so `src`'s uniqueness backs the returned reference for its whole lifetime.
    unsafe { &mut *ptr }
}
