//! `dyn_traits(..)` where the hierarchy has a virtual base.
//!
//! The shared base is a field of the generated complete object and of nothing else, so it takes
//! part in `Clone`, `PartialEq` and `Hash` exactly once however many paths reach it.

#![allow(missing_docs)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use obj::{Class, Obj, VBase};

#[obj::class(abstract, dyn_traits(Debug, Clone, PartialEq, Eq, Hash))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Node {
    pub id: u32,
}

#[obj::methods]
impl Node {
    #[obj(virtual)]
    fn label(&self) -> String;
}

#[obj::class(extends(virtual Node), abstract)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Left {
    pub l: u32,
}

#[obj::methods]
impl Left {}

#[obj::class(extends(virtual Node), abstract)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Right {
    pub r: u32,
}

#[obj::methods]
impl Right {}

#[obj::class(extends(Left, Right))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Both {
    pub b: u32,
}

#[obj::methods]
impl Both {
    #[obj(override)]
    fn label(&self) -> String {
        format!("{}/{}/{}/{}", self.id, self.l, self.as_right().r, self.b)
    }
}

fn both(id: u32, l: u32, r: u32, b: u32) -> <Both as Class>::Complete {
    Both::complete(
        Both {
            left: Left {
                node: VBase::new(),
                l,
            },
            right: Right {
                node: VBase::new(),
                r,
            },
            b,
        },
        Node { id },
    )
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

#[test]
fn cloning_copies_the_shared_base_once_and_relinks_it() {
    let original = Obj::<Both>::new(both(1, 2, 3, 4));
    let mut copy = original.clone();

    assert_eq!(copy.label(), "1/2/3/4", "the copy is intact");

    // The copy's links point into the copy, not back at the original.
    let via_left: *const Node = copy.as_node();
    let via_right: *const Node = copy.as_right().as_node();
    assert_eq!(via_left, via_right, "still one shared base in the copy");
    assert_ne!(
        via_left,
        std::ptr::from_ref::<Node>(original.as_node()),
        "and it is not the original's",
    );

    copy.as_node_mut().id = 99;
    assert_eq!(copy.label(), "99/2/3/4");
    assert_eq!(original.label(), "1/2/3/4", "the original is untouched");
}

#[test]
fn equality_compares_the_shared_base() {
    let a: Obj<Node> = Obj::<Both>::new(both(1, 2, 3, 4)).upcast();
    let same: Obj<Node> = Obj::<Both>::new(both(1, 2, 3, 4)).upcast();
    // Differs only in the *shared* base, which lives outside `Both` itself.
    let other_shared: Obj<Node> = Obj::<Both>::new(both(9, 2, 3, 4)).upcast();
    let other_own: Obj<Node> = Obj::<Both>::new(both(1, 2, 3, 9)).upcast();

    assert_eq!(a, same);
    assert_ne!(a, other_shared, "the shared base is part of the value");
    assert_ne!(a, other_own);

    assert_eq!(hash_of(&a), hash_of(&same));
}

#[test]
fn cloning_a_bare_subobject_clears_its_link() {
    let whole = Obj::<Both>::new(both(1, 2, 3, 4));

    // Copy the class subobject out of the complete object that owns the shared base. The copy is
    // not part of any complete object, so keeping the old offset would leave it pointing at
    // memory outside itself -- the original's `Node`, or past the end of the copy entirely.
    let stray: Both = whole.borrow().cast::<Both>().expect("is a Both").clone();

    assert_eq!(stray.b, 4, "the class's own data is copied");
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = stray.as_node().id;
    }));
    assert!(
        panicked.is_err(),
        "a copied subobject must not resolve the original's shared base",
    );
}

#[test]
fn debug_shows_the_object_and_its_shared_base() {
    let x: Obj<Node> = Obj::<Both>::new(both(1, 2, 3, 4)).upcast();
    let text = format!("{x:?}");

    assert!(text.contains("BothComplete"), "{text}");
    assert!(text.contains("id: 1"), "the shared base is shown: {text}");
    assert!(text.contains("b: 4"), "and the class's own field: {text}");
    assert_eq!(text.matches("id: 1").count(), 1, "shown once, not per path");
}
