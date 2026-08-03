//! C++-style inheritance for Rust.
//!
//! Rust deliberately has no language support for implementation inheritance, and for almost every
//! program that is the right call. `obj` exists for the remainder: deep framework hierarchies,
//! DOM/AST/widget trees, and ports of C++ designs where the inheritance *is* the architecture.
//!
//! # The idea
//!
//! Rust already has vtables — `dyn Trait`. Rather than emitting a C++-style `__vptr` into every
//! object and reimplementing dispatch, `obj` maps each C++ concept onto machinery the compiler
//! already provides:
//!
//! | C++ | `obj` | Cost |
//! |---|---|---|
//! | Field inheritance | `#[repr(C)]` base as first field, plus a `Deref` chain | zero |
//! | Virtual method | one generated trait per class; supertrait = base class | zero |
//! | `Derived*` → `Base*` | trait-upcasting coercion | zero |
//! | `dynamic_cast` | [`ClassMeta`] base table, reached by one virtual call | ~a call |
//! | Abstract class | [`Concrete`] simply not implemented | compile-time |
//! | Virtual destructor | `Box<dyn _>` drop glue | free |
//!
//! Objects carry **no per-object overhead**: the vtable rides in the handle, exactly as it does
//! for `&dyn Trait`.
//!
//! # The one rule
//!
//! There are two ways to refer to an object, and they mean different things:
//!
//! - `&Circle` / `&mut Circle` — a **static view**. Method calls resolve to inherent methods and
//!   are non-virtual, like `obj.f()` in C++.
//! - [`Obj<C>`] / [`Ref<C>`] / [`RefMut<C>`] — a **polymorphic handle**. Method calls go through
//!   the class's interface and dispatch virtually, like `ptr->f()` in C++.
//!
//! Handles deref to the static view, so fields and inherited methods are always in reach.
//! Crucially, the reverse requires a complete object, which is what makes C++ slicing
//! unrepresentable here.
//!
//! # Safety
//!
//! Generated code is safe; the `unsafe` lives in this crate and has three sources, all documented
//! at their definitions: prefix-layout subobject addressing (guarded by `#[repr(C)]` and generated
//! `offset_of!` assertions), fat-pointer reconstruction for sidecasts, and virtual-base offset
//! arithmetic.

#![no_std]
#![cfg_attr(feature = "nightly", feature(ptr_metadata))]

extern crate alloc;

mod cast;
pub mod class;
mod handle;
pub mod meta;

#[doc(hidden)]
pub mod __private;

pub use class::{AnyObj, Class, Concrete, SubclassOf};
pub use handle::{Obj, Ref, RefMut};
pub use meta::{BaseEntry, ClassId, ClassMeta, VTablePtr};

/// Captures the vtable produced by coercing a concrete type to a `dyn` interface.
///
/// Used by generated code to fill in [`BaseEntry::dyn_vtable`]. The null data pointer is never
/// dereferenced — casting a raw pointer to a raw trait-object pointer is an unsizing coercion that
/// only computes metadata.
///
/// This is not a stable API.
#[doc(hidden)]
#[macro_export]
macro_rules! __vtable_of {
    ($concrete:ty as $interface:ty) => {
        // SAFETY: the vtable half is taken from a genuine `*const $interface` coercion of
        // `$concrete`, so it is exactly the vtable for `$concrete as $interface`. `rebuild_fat`
        // reverses this transmute symmetrically.
        unsafe {
            $crate::meta::VTablePtr::from_raw(
                ::core::mem::transmute::<*const $interface, [*const (); 2]>(::core::ptr::null::<
                    $concrete,
                >()
                    as *const $interface)[1],
            )
        }
    };
}
