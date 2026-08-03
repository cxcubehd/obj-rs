//! Items referenced by macro-generated code.
//!
//! Not a stable API: anything here may change in a patch release.

#![allow(missing_docs)]

pub use alloc::boxed::Box;
pub use core::any::TypeId;
pub use core::mem::offset_of;
pub use core::ops::{Deref, DerefMut};

pub use crate::class::{AnyObj, Class, Concrete, SubclassOf};
pub use crate::meta::{BaseEntry, ClassMeta, VTablePtr};
