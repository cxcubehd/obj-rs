//! Items referenced by macro-generated code.
//!
//! Not a stable API: anything here may change in a patch release.

#![allow(missing_docs)]

pub use alloc::boxed::Box;
pub use alloc::rc::Rc;
pub use alloc::sync::Arc;
pub use core::any::{Any, TypeId};
pub use core::clone::Clone;
pub use core::cmp::PartialEq;
pub use core::hash::{Hash, Hasher};
pub use core::mem::offset_of;
pub use core::ops::{Deref, DerefMut};

pub use crate::cast::{subobject, subobject_mut};
pub use crate::class::{AnyObj, Class, Concrete, SubclassOf};
pub use crate::dyn_traits::{CloneObj, DynEq, DynHash, DynTotalEq};
pub use crate::meta::{BaseEntry, BaseTable, ClassMeta, VTablePtr};
pub use crate::vbase::VBase;
