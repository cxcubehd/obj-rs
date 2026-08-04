//! Virtual (shared) base classes.
//!
//! With ordinary inheritance a base subobject lives inside its derived class, so a diamond ends up
//! with two copies of the shared ancestor. Virtual inheritance is C++'s answer: the shared base is
//! stored **once, in the complete object**, and every subobject that needs it holds a link.
//!
//! ```text
//!            Doc                     XhtmlComplete
//!           /   \                    +----------------+
//!    (virtual) (virtual)             | Xhtml          |
//!         /       \                  |   Html --------+--.
//!      Html       Xml                |   Xml  --------+--|
//!          \     /                   |   strict       |  |
//!           Xhtml                    | Doc  <---------+--'
//!                                    +----------------+
//! ```
//!
//! [`VBase<T>`] is that link. It stores a signed byte offset rather than a pointer, because Rust
//! moves values freely and an offset between two subobjects of the same complete object survives a
//! move while a pointer would dangle.
//!
//! You do not build one by hand beyond writing [`VBase::new()`] in a struct literal, which marks
//! the slot unlinked; the generated `Class::complete(..)` constructor fills every slot in.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// Offset value meaning "this slot has not been placed in a complete object yet".
///
/// `i32::MIN` cannot be a real offset — no object is that large — so a class literal written by
/// hand is always distinguishable from a linked one.
const UNLINKED: i32 = i32::MIN;

/// A link from a subobject to a virtual base shared across the complete object.
///
/// `#[obj::class(extends(virtual Doc))]` gives the class a `VBase<Doc>` field where a non-virtual
/// base would have a `Doc` subobject. Write [`VBase::new()`] for it in a struct literal; the
/// most-derived class's `complete(..)` constructor is what links it.
///
/// Reach the base through the generated `as_doc()` / `as_doc_mut()` accessors, or through `Deref`
/// when the virtual base is the class's only base.
#[repr(transparent)]
pub struct VBase<T> {
    /// Signed byte offset from the start of the owning subobject to the shared base.
    offset: i32,
    /// The link only ever *yields* a `T`, so this is covariant and imposes no auto-trait bounds.
    _marker: PhantomData<fn() -> T>,
}

impl<T> VBase<T> {
    /// An unlinked slot — what you write in a class literal.
    ///
    /// Resolving one panics; the complete-object constructor replaces it with a real link.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            offset: UNLINKED,
            _marker: PhantomData,
        }
    }

    /// Links this slot to a shared base sitting `offset` bytes from the owning subobject.
    ///
    /// # Safety
    ///
    /// `offset` must be the distance, within one complete object, from the subobject that stores
    /// this `VBase` to a live `T` subobject. Generated code derives it by subtracting two
    /// `offset_of!` constants of the same `#[repr(C)]` complete type, which is the only way it is
    /// meant to be produced.
    ///
    /// # Panics
    ///
    /// If `offset` does not fit in an `i32`, which would mean a single object larger than 2 GiB.
    #[must_use]
    pub const unsafe fn link(offset: isize) -> Self {
        assert!(
            offset > UNLINKED as isize && offset <= i32::MAX as isize,
            "obj: virtual base is too far from its subobject to address with a 32-bit offset",
        );
        Self {
            offset: offset as i32,
            _marker: PhantomData,
        }
    }

    /// Whether this slot has been placed in a complete object.
    #[must_use]
    pub const fn is_linked(&self) -> bool {
        self.offset != UNLINKED
    }

    /// Duplicates the link itself, rather than clearing it the way [`Clone`] does.
    ///
    /// Generated accessors use this to lift the link out of a subobject before taking a unique
    /// borrow of that same subobject. It is not a way to move a link to a different owner: the
    /// copy is only meaningful for the subobject it came from.
    #[doc(hidden)]
    #[must_use]
    pub const fn raw(&self) -> Self {
        Self {
            offset: self.offset,
            _marker: PhantomData,
        }
    }

    /// The stored offset in bytes.
    ///
    /// # Panics
    ///
    /// If the slot is still unlinked.
    fn byte_offset(self) -> isize {
        assert!(
            self.is_linked(),
            "obj: virtual base is not linked -- this subobject was not built through the \
             most-derived class's `complete(..)` constructor",
        );
        self.offset as isize
    }

    /// Borrows the shared base, given the subobject that owns this link.
    ///
    /// # Panics
    ///
    /// If the slot is still unlinked.
    #[must_use]
    pub fn resolve<O>(&self, owner: &O) -> &T {
        let addr = Self::target_addr(core::ptr::from_ref(owner).cast::<u8>(), self.raw());
        // SAFETY: `link` guarantees the offset reaches a live, initialised `T` subobject of the
        // same complete object, and `target_addr` carried that object's provenance across, so the
        // pointer is valid to dereference for as long as `owner` is borrowed.
        unsafe { &*core::ptr::with_exposed_provenance::<T>(addr) }
    }

    /// Mutably borrows the shared base, given the subobject that owns this link.
    ///
    /// Takes `self` by value so the caller can copy the link out first and hand over the unique
    /// borrow of the owner without overlapping it.
    ///
    /// # Panics
    ///
    /// If the slot is still unlinked.
    #[must_use]
    pub fn resolve_mut<O>(self, owner: &mut O) -> &mut T {
        let addr = Self::target_addr(core::ptr::from_mut(owner).cast::<u8>(), self);
        // SAFETY: as `resolve`, and `owner` is borrowed uniquely for the returned lifetime, so the
        // `T` subobject it addresses is reachable through no other live reference.
        unsafe { &mut *core::ptr::with_exposed_provenance_mut::<T>(addr) }
    }

    /// The address of the shared base, with the complete object's provenance made reachable.
    ///
    /// A reference to a subobject carries permission for **that subobject only**, so offsetting it
    /// to a *sibling* — which is exactly what a virtual base is — steps outside what the borrow
    /// grants, and Miri rejects it. Exposing the provenance first widens it back to the
    /// allocation the subobject belongs to, which is the complete object that owns both.
    ///
    /// This is the one place `obj` needs exposed provenance rather than strict provenance. It is
    /// confined to these two functions, and the address it produces is always
    /// `link`-derived — an offset between two `offset_of!` constants of one `#[repr(C)]` complete
    /// object, so it never leaves that object.
    fn target_addr(owner: *const u8, link: Self) -> usize {
        let offset = link.byte_offset();
        owner.expose_provenance().wrapping_add_signed(offset)
    }
}

impl<T> Default for VBase<T> {
    fn default() -> Self {
        Self::new()
    }
}

// These are written by hand rather than derived: a derive would add a `T: Trait` bound, and the
// link is a layout detail that has nothing to do with what `T` implements.

/// Cloning a link **clears** it.
///
/// A link says where the shared base is *relative to this subobject*, so it is only meaningful
/// inside the complete object it was built for. `#[derive(Clone)]` on a class copies its fields
/// into a fresh value that is not in any complete object, and carrying the old offset over would
/// leave it pointing outside — so the copy comes back unlinked and resolving it panics instead.
///
/// Cloning a whole object is unaffected: the generated `Complete` wrapper implements [`Clone`] by
/// rebuilding through its constructor, which links every slot again.
///
/// This is also why `VBase` is deliberately not [`Copy`] — a bitwise copy would disagree with
/// this, and `Copy` promises the two are the same.
impl<T> Clone for VBase<T> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for VBase<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_linked() {
            f.write_str("VBase(linked)")
        } else {
            f.write_str("VBase(unlinked)")
        }
    }
}

/// Two links are always equal, and hash identically.
///
/// A link is layout, not data. Comparing offsets would make two objects of the same class differ
/// for no observable reason, and — more importantly — the shared base itself is compared and
/// hashed exactly once, as a field of the generated complete object, so counting it here would
/// count it twice.
impl<T> PartialEq for VBase<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl<T> Eq for VBase<T> {}

impl<T> Hash for VBase<T> {
    fn hash<H: Hasher>(&self, _state: &mut H) {}
}
