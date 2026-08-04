//! Polymorphic handles.
//!
//! These are the `obj` equivalent of a C++ pointer or reference to a base class: they know the
//! object's most-derived class at runtime, so method calls through them dispatch virtually.
//!
//! A plain `&Circle` is the opposite: a *static* view, where calls resolve to inherent methods
//! exactly like `obj.f()` in C++. Going from a handle to a static view is a [`Deref`] away; going
//! back requires constructing a handle from a complete object, which is what makes slicing
//! unrepresentable.

use alloc::boxed::Box;
use alloc::rc::{Rc, Weak as RcWeak};
use alloc::sync::{Arc, Weak as ArcWeak};
use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::{Deref, DerefMut};

use crate::cast::{
    data_ptr_of, data_ptr_of_mut, dyn_ptr_of, dyn_vtable_of, is_a, rebuild_fat, vtable_of,
};
use crate::class::{ArcCoerce, Class, Concrete, SubclassOf};
use crate::dyn_traits::{CloneObj, DynEq, DynHash, DynTotalEq};
use crate::meta::ClassMeta;

/// Deep-copies the object behind a polymorphic reference, preserving its most-derived class.
///
/// No base-table lookup is needed: [`CloneObj`] promises the copy has the same most-derived type
/// as `src`, so `src`'s own vtable already describes it.
fn clone_dyn<C: Class>(src: &C::Dyn) -> Box<C::Dyn>
where
    C::Dyn: CloneObj,
{
    let vtable = vtable_of(core::ptr::from_ref(src));
    let raw = src.clone_raw();
    // SAFETY: `CloneObj` guarantees `raw` came from `Box::into_raw` on a fresh object of the same
    // most-derived type as `src`, so pairing it with `src`'s vtable is coherent, and the size,
    // alignment and drop glue that `Box` recovers are the ones the allocation was made with.
    let fat = unsafe { rebuild_fat::<C::Dyn>(raw.cast_const(), vtable) };
    // SAFETY: as above. `clone_raw` handed over ownership of the allocation.
    unsafe { Box::from_raw(fat.cast_mut()) }
}

/// An owning polymorphic handle — the `obj` analogue of C++'s `unique_ptr<C>`.
///
/// Derefs to the class's `dyn` interface (virtual methods), which in turn derefs to the class data
/// (fields and inherent methods) and onwards up the base chain, so `obj.virtual_method()`,
/// `obj.own_field` and `obj.inherited_field` all resolve as you would expect.
pub struct Obj<C: Class>(Box<C::Dyn>);

impl<C: Class> Obj<C> {
    /// Allocates a new object.
    ///
    /// Only available for [`Concrete`] classes: an abstract class (one with a pure virtual method)
    /// does not implement it, so this is a compile-time error rather than a runtime panic.
    pub fn new(value: C::Complete) -> Self
    where
        C: Concrete,
    {
        Self(C::into_dyn(Box::new(value)))
    }

    /// Wraps an already-boxed interface pointer.
    #[must_use]
    pub fn from_dyn_box(boxed: Box<C::Dyn>) -> Self {
        Self(boxed)
    }

    /// Unwraps into the underlying boxed interface pointer, for interop with APIs that want a
    /// plain `Box<dyn _>`.
    #[must_use]
    pub fn into_dyn_box(self) -> Box<C::Dyn> {
        self.0
    }

    /// Borrows this object as a polymorphic reference.
    #[must_use]
    pub fn borrow(&self) -> Ref<'_, C> {
        Ref(&*self.0)
    }

    /// Mutably borrows this object as a polymorphic reference.
    #[must_use]
    pub fn borrow_mut(&mut self) -> RefMut<'_, C> {
        RefMut(&mut *self.0)
    }

    /// Metadata for this object's most-derived class.
    #[must_use]
    pub fn class(&self) -> &'static ClassMeta {
        crate::class::AnyObj::class_meta(&*self.0)
    }

    /// Upcasts to a base class. Compile-time checked and free.
    #[must_use]
    pub fn upcast<B: Class>(self) -> Obj<B>
    where
        C: SubclassOf<B>,
    {
        Obj(C::up_box(self.0))
    }

    /// Returns whether this object's most-derived class is, or derives from, `T`.
    #[must_use]
    pub fn is<T: Class>(&self) -> bool {
        is_a::<T, _>(&*self.0)
    }

    /// Attempts to downcast, taking ownership.
    ///
    /// Returns the original handle unchanged on failure, so nothing is lost.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if this object's most-derived class does not derive from `T`.
    pub fn downcast<T: Class>(self) -> Result<Obj<T>, Self> {
        let Some(vtable) = dyn_vtable_of::<T, _>(&*self.0) else {
            return Err(self);
        };
        // Release ownership *before* rebuilding, so the object is never owned twice — and rebuild
        // from the raw pointer rather than from `obj_addr`, so the result inherits the
        // allocation's own provenance instead of that of the shared borrow used for the lookup.
        let raw: *mut C::Dyn = Box::into_raw(self.0);
        // SAFETY: the data half of `raw` is the address of the complete object, which is what a
        // polymorphic pointer must carry, and `vtable` was captured from that same most-derived
        // type coerced to `T::Dyn`, so the pair is coherent.
        let ptr: *const T::Dyn = unsafe { rebuild_fat(raw.cast::<u8>(), vtable) };
        // SAFETY: `ptr` addresses the very same allocation, and its vtable belongs to the same
        // most-derived type, so the size/align/drop recovered from it are the ones the allocation
        // was created with.
        Ok(Obj(unsafe { Box::from_raw(ptr.cast_mut()) }))
    }
}

impl<C: Class> Deref for Obj<C> {
    type Target = C::Dyn;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<C: Class> DerefMut for Obj<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A borrowed polymorphic handle — the `obj` analogue of C++'s `const C&` where the referent may
/// be any subclass.
pub struct Ref<'a, C: Class>(&'a C::Dyn);

impl<'a, C: Class> Ref<'a, C> {
    /// Wraps a borrowed interface pointer.
    #[must_use]
    pub fn from_dyn(value: &'a C::Dyn) -> Self {
        Self(value)
    }

    /// Borrows a complete object polymorphically.
    #[must_use]
    pub fn from_complete(value: &'a C::Complete) -> Self
    where
        C: Concrete,
    {
        Self(C::as_dyn(value))
    }

    /// The underlying interface pointer.
    #[must_use]
    pub fn as_dyn(self) -> &'a C::Dyn {
        self.0
    }

    /// Metadata for this object's most-derived class.
    #[must_use]
    pub fn class(self) -> &'static ClassMeta {
        crate::class::AnyObj::class_meta(self.0)
    }

    /// Upcasts to a base class. Compile-time checked and free.
    #[must_use]
    pub fn upcast<B: Class>(self) -> Ref<'a, B>
    where
        C: SubclassOf<B>,
    {
        Ref(C::up_ref(self.0))
    }

    /// Returns whether this object's most-derived class is, or derives from, `T`.
    #[must_use]
    pub fn is<T: Class>(self) -> bool {
        is_a::<T, _>(self.0)
    }

    /// Casts to the *data* view of another class in this object's hierarchy.
    ///
    /// This is the C++ `dynamic_cast<T*>` that lands on a subobject: the result is a static view,
    /// so calls through it are non-virtual. Use [`cast_obj`](Self::cast_obj) to keep polymorphism.
    ///
    /// Works downwards and sideways — casting from one base of a multiply-inherited class to
    /// another is a normal lookup in the base table.
    #[must_use]
    pub fn cast<T: Class>(self) -> Option<&'a T> {
        // SAFETY: `data_ptr_of` returns an in-bounds pointer to the `T` subobject of a live
        // object, which outlives `'a`.
        data_ptr_of::<T, _>(self.0).map(|p| unsafe { &*p })
    }

    /// Casts to another class in this object's hierarchy, keeping virtual dispatch.
    #[must_use]
    pub fn cast_obj<T: Class>(self) -> Option<Ref<'a, T>> {
        // SAFETY: as above; `dyn_ptr_of` pairs the object's address with a vtable captured from
        // its own most-derived type.
        dyn_ptr_of::<T, _>(self.0).map(|p| Ref(unsafe { &*p }))
    }
}

impl<C: Class> Deref for Ref<'_, C> {
    type Target = C::Dyn;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<C: Class> Clone for Ref<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: Class> Copy for Ref<'_, C> {}

/// A mutably borrowed polymorphic handle.
pub struct RefMut<'a, C: Class>(&'a mut C::Dyn);

impl<'a, C: Class> RefMut<'a, C> {
    /// Wraps a mutably borrowed interface pointer.
    #[must_use]
    pub fn from_dyn(value: &'a mut C::Dyn) -> Self {
        Self(value)
    }

    /// Mutably borrows a complete object polymorphically.
    #[must_use]
    pub fn from_complete(value: &'a mut C::Complete) -> Self
    where
        C: Concrete,
    {
        Self(C::as_dyn_mut(value))
    }

    /// Reborrows as a shared polymorphic reference.
    #[must_use]
    pub fn as_ref(&self) -> Ref<'_, C> {
        Ref(self.0)
    }

    /// Reborrows with a shorter lifetime.
    #[must_use]
    pub fn reborrow(&mut self) -> RefMut<'_, C> {
        RefMut(self.0)
    }

    /// The underlying interface pointer.
    #[must_use]
    pub fn into_dyn(self) -> &'a mut C::Dyn {
        self.0
    }

    /// Metadata for this object's most-derived class.
    #[must_use]
    pub fn class(&self) -> &'static ClassMeta {
        crate::class::AnyObj::class_meta(self.0)
    }

    /// Upcasts to a base class. Compile-time checked and free.
    #[must_use]
    pub fn upcast<B: Class>(self) -> RefMut<'a, B>
    where
        C: SubclassOf<B>,
    {
        RefMut(C::up_mut(self.0))
    }

    /// Returns whether this object's most-derived class is, or derives from, `T`.
    #[must_use]
    pub fn is<T: Class>(&self) -> bool {
        is_a::<T, _>(self.0)
    }

    /// Mutably casts to the *data* view of another class in this object's hierarchy.
    #[must_use]
    pub fn cast_mut<T: Class>(self) -> Option<&'a mut T> {
        // SAFETY: `data_ptr_of_mut` returns an in-bounds, writable pointer to the `T` subobject;
        // `self` holds the unique borrow for `'a`, so handing out a unique reference to a
        // subobject of it is sound.
        data_ptr_of_mut::<T, _>(self.0).map(|p| unsafe { &mut *p })
    }
}

impl<C: Class> Deref for RefMut<'_, C> {
    type Target = C::Dyn;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<C: Class> DerefMut for RefMut<'_, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

/// A reference-counted polymorphic handle — the `obj` analogue of C++'s `shared_ptr<C>`.
///
/// Single-threaded. For an object graph shared across threads, use [`ArcShared`].
pub struct Shared<C: Class>(Rc<C::Dyn>);

impl<C: Class> Shared<C> {
    /// Allocates a new shared object.
    pub fn new(value: C::Complete) -> Self
    where
        C: Concrete,
    {
        Self(C::rc_into_dyn(Rc::new(value)))
    }

    /// Borrows this object as a polymorphic reference.
    #[must_use]
    pub fn borrow(&self) -> Ref<'_, C> {
        Ref::from_dyn(&*self.0)
    }

    /// Metadata for this object's most-derived class.
    #[must_use]
    pub fn class(&self) -> &'static ClassMeta {
        crate::class::AnyObj::class_meta(&*self.0)
    }

    /// Upcasts to a base class. Compile-time checked and free.
    #[must_use]
    pub fn upcast<B: Class>(self) -> Shared<B>
    where
        C: SubclassOf<B>,
    {
        Shared(C::up_rc(self.0))
    }

    /// Returns whether this object's most-derived class is, or derives from, `T`.
    #[must_use]
    pub fn is<T: Class>(&self) -> bool {
        is_a::<T, _>(&*self.0)
    }

    /// Creates a non-owning handle to this object.
    #[must_use]
    pub fn downgrade(&self) -> WeakShared<C> {
        WeakShared(Rc::downgrade(&self.0))
    }

    /// Number of strong handles to this object.
    #[must_use]
    pub fn strong_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    /// Attempts to downcast, returning the original handle on failure.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if this object's most-derived class does not derive from `T`.
    pub fn downcast<T: Class>(self) -> Result<Shared<T>, Self> {
        let Some(vtable) = dyn_vtable_of::<T, _>(&*self.0) else {
            return Err(self);
        };
        // As in `Obj::downcast`: rebuild from the pointer that owns the allocation, so the result
        // does not inherit the read-only provenance of the borrow used for the lookup. `from_raw`
        // needs to reach the reference counts that sit *before* the value, which a pointer derived
        // from `&*self.0` has no provenance over.
        let raw: *const C::Dyn = Rc::into_raw(self.0);
        // SAFETY: the data half of `raw` addresses the complete object, and `vtable` was captured
        // from that same most-derived type coerced to `T::Dyn`.
        let ptr: *const T::Dyn = unsafe { rebuild_fat(raw.cast::<u8>(), vtable) };
        // SAFETY: `ptr` addresses the same allocation, and its vtable belongs to the same
        // most-derived type, so the layout recovered from it is the one the `Rc` was built with,
        // and the strong count was transferred rather than duplicated.
        Ok(Shared(unsafe { Rc::from_raw(ptr) }))
    }
}

impl<C: Class> Clone for Shared<C> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<C: Class> Deref for Shared<C> {
    type Target = C::Dyn;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A non-owning handle to a [`Shared`] object.
pub struct WeakShared<C: Class>(RcWeak<C::Dyn>);

impl<C: Class> WeakShared<C> {
    /// Upgrades to a strong handle, unless the object has been dropped.
    #[must_use]
    pub fn upgrade(&self) -> Option<Shared<C>> {
        self.0.upgrade().map(Shared)
    }
}

impl<C: Class> Clone for WeakShared<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// A thread-safe reference-counted polymorphic handle.
///
/// Stores [`Class::SendDyn`], so constructing one requires the class to be `Send + Sync`.
pub struct ArcShared<C: Class>(Arc<C::SendDyn>);

impl<C: Class> ArcShared<C> {
    /// Allocates a new shared object.
    pub fn new(value: C::Complete) -> Self
    where
        C: Concrete,
        C::SendDyn: ArcCoerce<C::Complete>,
    {
        Self(<C::SendDyn as ArcCoerce<C::Complete>>::arc_coerce(
            Arc::new(value),
        ))
    }

    /// Borrows this object as a polymorphic reference.
    #[must_use]
    pub fn borrow(&self) -> Ref<'_, C> {
        Ref::from_dyn(C::send_as_dyn(&self.0))
    }

    /// Metadata for this object's most-derived class.
    #[must_use]
    pub fn class(&self) -> &'static ClassMeta {
        crate::class::AnyObj::class_meta(&*self.0)
    }

    /// Upcasts to a base class. Compile-time checked and free.
    #[must_use]
    pub fn upcast<B: Class>(self) -> ArcShared<B>
    where
        C: SubclassOf<B>,
    {
        ArcShared(C::up_arc(self.0))
    }

    /// Returns whether this object's most-derived class is, or derives from, `T`.
    #[must_use]
    pub fn is<T: Class>(&self) -> bool {
        is_a::<T, _>(&*self.0)
    }

    /// Creates a non-owning handle to this object.
    #[must_use]
    pub fn downgrade(&self) -> WeakArcShared<C> {
        WeakArcShared(Arc::downgrade(&self.0))
    }

    /// Number of strong handles to this object.
    #[must_use]
    pub fn strong_count(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// Attempts to downcast, returning the original handle on failure.
    ///
    /// # Errors
    ///
    /// Returns `Err(self)` if this object's most-derived class does not derive from `T`.
    pub fn downcast<T: Class>(self) -> Result<ArcShared<T>, Self> {
        let Some(vtable) = dyn_vtable_of::<T, _>(&*self.0) else {
            return Err(self);
        };
        // As in `Shared::downcast`: the address has to come from the pointer that owns the
        // allocation, not from a borrow of the value, because `from_raw` walks back to the
        // reference counts stored ahead of it.
        let raw: *const C::SendDyn = Arc::into_raw(self.0);
        // SAFETY: the rebuilt pointer addresses the same allocation and its vtable belongs to the
        // same most-derived type, so the layout `Arc::from_raw` recovers is the one the handle was
        // built with. `Send + Sync` carry over because the concrete type is unchanged.
        let fat: *const T::SendDyn = unsafe { rebuild_fat(raw.cast::<u8>(), vtable) };
        // SAFETY: as above.
        Ok(ArcShared(unsafe { Arc::from_raw(fat) }))
    }
}

impl<C: Class> Clone for ArcShared<C> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<C: Class> Deref for ArcShared<C> {
    type Target = C::SendDyn;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A non-owning handle to an [`ArcShared`] object.
pub struct WeakArcShared<C: Class>(ArcWeak<C::SendDyn>);

impl<C: Class> WeakArcShared<C> {
    /// Upgrades to a strong handle, unless the object has been dropped.
    #[must_use]
    pub fn upgrade(&self) -> Option<ArcShared<C>> {
        self.0.upgrade().map(ArcShared)
    }
}

impl<C: Class> Clone for WeakArcShared<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

// ------------------------------------------------------------------ virtual clone

impl<C: Class> Clone for Obj<C>
where
    C::Dyn: CloneObj,
{
    /// Deep-copies the object, preserving its most-derived class.
    ///
    /// Cloning an `Obj<Shape>` that holds a `Circle` yields an `Obj<Shape>` holding a `Circle` —
    /// the C++ "virtual clone" idiom. Available once the class opts in with
    /// `#[obj::class(dyn_traits(Clone))]`.
    fn clone(&self) -> Self {
        Obj(clone_dyn::<C>(&self.0))
    }
}

impl<C: Class> Ref<'_, C> {
    /// Deep-copies the referent into an owned handle, preserving its most-derived class.
    #[must_use]
    pub fn clone_obj(self) -> Obj<C>
    where
        C::Dyn: CloneObj,
    {
        Obj(clone_dyn::<C>(self.0))
    }
}

impl<C: Class> RefMut<'_, C> {
    /// Deep-copies the referent into an owned handle, preserving its most-derived class.
    #[must_use]
    pub fn clone_obj(&self) -> Obj<C>
    where
        C::Dyn: CloneObj,
    {
        Obj(clone_dyn::<C>(self.0))
    }
}

impl<C: Class> Shared<C> {
    /// Deep-copies the object into a new, unshared handle.
    ///
    /// [`Clone`](Self::clone) shares the existing object by bumping its refcount; this copies it.
    #[must_use]
    pub fn clone_obj(&self) -> Obj<C>
    where
        C::Dyn: CloneObj,
    {
        Obj(clone_dyn::<C>(&self.0))
    }
}

impl<C: Class> ArcShared<C> {
    /// Deep-copies the object into a new, unshared handle.
    ///
    /// [`Clone`](Self::clone) shares the existing object by bumping its refcount; this copies it.
    #[must_use]
    pub fn clone_obj(&self) -> Obj<C>
    where
        C::Dyn: CloneObj,
    {
        Obj(clone_dyn::<C>(C::send_as_dyn(&self.0)))
    }
}

// ------------------------------------------------------- formatting, equality and hashing
//
// Each handle forwards to its referent, so the impls are available exactly when the class opted
// into the corresponding `dyn_traits(..)` entry.

/// Forwards `Debug` and `Display` from a handle to the object behind it.
macro_rules! forward_fmt {
    ($($name:ident $(<$lt:lifetime>)?, $target:ident, |$s:ident| $get:expr;)*) => {$(
        impl<$($lt,)? C: Class> fmt::Debug for $name<$($lt,)? C>
        where
            C::$target: fmt::Debug,
        {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let $s = self;
                fmt::Debug::fmt($get, f)
            }
        }

        impl<$($lt,)? C: Class> fmt::Display for $name<$($lt,)? C>
        where
            C::$target: fmt::Display,
        {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let $s = self;
                fmt::Display::fmt($get, f)
            }
        }
    )*};
}

forward_fmt! {
    Obj, Dyn, |s| &*s.0;
    Ref<'a>, Dyn, |s| s.0;
    RefMut<'a>, Dyn, |s| &*s.0;
    Shared, Dyn, |s| &*s.0;
    ArcShared, SendDyn, |s| &*s.0;
}

/// Forwards `PartialEq`, `Eq` and `Hash` from a handle to the object behind it.
///
/// Equality is heterogeneous — objects of different most-derived classes are never equal — and the
/// class identity is folded into the hash to match, so the two stay consistent.
macro_rules! forward_cmp {
    ($($name:ident $(<$lt:lifetime>)?, $target:ident, |$s:ident| $get:expr;)*) => {$(
        impl<$($lt,)? C: Class> PartialEq for $name<$($lt,)? C>
        where
            C::$target: DynEq,
        {
            fn eq(&self, other: &Self) -> bool {
                let ($s, o) = (self, other);
                let this: &C::$target = $get;
                let $s = o;
                this.dyn_eq($get.as_any())
            }
        }

        impl<$($lt,)? C: Class> Eq for $name<$($lt,)? C> where C::$target: DynTotalEq {}

        impl<$($lt,)? C: Class> Hash for $name<$($lt,)? C>
        where
            C::$target: DynHash,
        {
            fn hash<H: Hasher>(&self, state: &mut H) {
                let $s = self;
                $get.dyn_hash(state);
            }
        }
    )*};
}

forward_cmp! {
    Obj, Dyn, |s| &*s.0;
    Ref<'a>, Dyn, |s| s.0;
    Shared, Dyn, |s| &*s.0;
    ArcShared, SendDyn, |s| &*s.0;
}
