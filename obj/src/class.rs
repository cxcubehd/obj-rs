//! The traits that define what it means to be a class.
//!
//! All of these are implemented by `#[obj::class]`; you should not need to write them by
//! hand, and several are `unsafe` to implement because the cast machinery trusts them.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::sync::Arc;

use crate::meta::ClassMeta;

/// Implemented by every class's `dyn` interface, giving type-erased access to the object's
/// identity.
///
/// # Safety
///
/// Implementors must return the metadata of the **most-derived** class and the address of the
/// **start of the complete object**. The cast machinery relies on both to compute subobject
/// addresses; a wrong answer produces pointers to the wrong memory.
pub unsafe trait AnyObj {
    /// Metadata for this object's most-derived class.
    fn class_meta(&self) -> &'static ClassMeta;

    /// Address of the start of the complete object.
    fn obj_addr(&self) -> *const u8;
}

/// A class.
///
/// Links the class's data type to its `dyn` interface and its runtime metadata.
///
/// # Safety
///
/// `Dyn` must be the `dyn` trait generated for this class, and `META` must describe this exact
/// class, with `bases[0]` being the class itself at offset 0. Casts rely on both.
pub unsafe trait Class: 'static {
    /// The generated `dyn` interface carrying this class's virtual methods.
    ///
    /// Polymorphic handles ([`Obj`](crate::Obj), [`Ref`](crate::Ref)) store a pointer to this.
    type Dyn: ?Sized + AnyObj;

    /// The same interface, plus `Send + Sync`.
    ///
    /// Auto traits are part of a trait object's type, so `dyn Shape` is never `Send` however
    /// thread-safe the underlying class is. [`ArcShared`](crate::ArcShared) stores this variant
    /// instead, which is why sharing an object across threads is expressible at all.
    type SendDyn: ?Sized + AnyObj + Send + Sync;

    /// The type that owns a complete object of this class.
    ///
    /// This is `Self` for ordinary classes. Classes with virtual bases use a generated wrapper
    /// that additionally stores the shared base subobjects, because those cannot live inside the
    /// class's own layout — see the crate-level docs on virtual inheritance.
    type Complete: 'static;

    /// This class's runtime metadata.
    const META: &'static ClassMeta;

    /// Views the thread-safe interface as the plain one, dropping the auto-trait bounds.
    ///
    /// Sound in one direction only, and needed because generic code cannot see that
    /// `Self::SendDyn` and `Self::Dyn` are the same trait with different auto traits.
    fn send_as_dyn(value: &Self::SendDyn) -> &Self::Dyn;
}

/// A class that can actually be instantiated.
///
/// Abstract classes — those with at least one pure virtual method — deliberately do **not**
/// implement this, which is what makes `Obj::new` of an abstract class a compile-time error
/// rather than a runtime panic.
///
/// # Safety
///
/// The returned pointers must refer to the same object that was passed in, coerced to this
/// class's `Dyn` interface.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is an abstract class, so it cannot be instantiated",
    label = "abstract class",
    note = "`{Self}` has at least one pure virtual method; instantiate a concrete subclass instead"
)]
pub unsafe trait Concrete: Class {
    /// Coerces an owned complete object to this class's interface.
    fn into_dyn(value: Box<Self::Complete>) -> Box<Self::Dyn>;

    /// Borrows a complete object as this class's interface.
    fn as_dyn(value: &Self::Complete) -> &Self::Dyn;

    /// Mutably borrows a complete object as this class's interface.
    fn as_dyn_mut(value: &mut Self::Complete) -> &mut Self::Dyn;

    /// Coerces a reference-counted complete object to this class's interface.
    fn rc_into_dyn(value: Rc<Self::Complete>) -> Rc<Self::Dyn>;

    /// Coerces an atomically reference-counted complete object to the thread-safe interface.
    fn arc_into_dyn(value: Arc<Self::Complete>) -> Arc<Self::SendDyn>
    where
        Self::Complete: Send + Sync;
}

/// `Self` derives from `B` (or *is* `B`).
///
/// Generated for every ancestor of every class, including the class itself, which is what makes
/// [`upcast`](crate::Obj::upcast) a compile-time-checked, zero-cost operation: each impl performs
/// a plain trait-upcasting coercion.
///
/// # Safety
///
/// The conversions must be pure coercions that preserve the object's address and identity.
pub unsafe trait SubclassOf<B: Class>: Class {
    /// Upcasts an owned handle.
    fn up_box(this: Box<Self::Dyn>) -> Box<B::Dyn>;

    /// Upcasts a shared reference.
    fn up_ref(this: &Self::Dyn) -> &B::Dyn;

    /// Upcasts a mutable reference.
    fn up_mut(this: &mut Self::Dyn) -> &mut B::Dyn;

    /// Upcasts a reference-counted handle.
    fn up_rc(this: Rc<Self::Dyn>) -> Rc<B::Dyn>;

    /// Upcasts an atomically reference-counted handle.
    fn up_arc(this: Arc<Self::SendDyn>) -> Arc<B::SendDyn>;
}
