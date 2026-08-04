//! `dyn_traits(..)`: standard traits carried through a polymorphic handle.
//!
//! ```text
//!   Shape (abstract)
//!     |        \
//!   Square    Circle
//!     |
//!  Rounded
//! ```

#![allow(missing_docs)]

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

use obj::{ArcShared, Obj, Ref, Shared};

#[obj::class(abstract, dyn_traits(Debug, Display, Clone, PartialEq, Eq, Hash))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape {
    pub x: i32,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn area(&self) -> i32;

    #[obj(virtual)]
    fn shift(&mut self, dx: i32) {
        self.x += dx;
    }
}

#[obj::class(extends = Shape)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Square {
    pub side: i32,
}

#[obj::methods]
impl Square {
    #[obj(override)]
    fn area(&self) -> i32 {
        self.side * self.side
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "square({})", self.side)
    }
}

/// Same field types as `Square`, so only the class identity tells them apart.
#[obj::class(extends = Shape)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Circle {
    pub r: i32,
}

#[obj::methods]
impl Circle {
    #[obj(override)]
    fn area(&self) -> i32 {
        3 * self.r * self.r
    }
}

impl fmt::Display for Circle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "circle({})", self.r)
    }
}

/// A third level, to check the traits keep flowing down the chain.
#[obj::class(extends = Square)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rounded {
    pub radius: i32,
}

#[obj::methods]
impl Rounded {
    #[obj(override)]
    fn area(&self) -> i32 {
        self.side * self.side - self.radius
    }
}

impl fmt::Display for Rounded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rounded({}, {})", self.side, self.radius)
    }
}

fn square(x: i32, side: i32) -> Square {
    Square {
        shape: Shape { x },
        side,
    }
}

fn circle(x: i32, r: i32) -> Circle {
    Circle {
        shape: Shape { x },
        r,
    }
}

fn rounded(x: i32, side: i32, radius: i32) -> Rounded {
    Rounded {
        square: square(x, side),
        radius,
    }
}

fn hash_of<T: Hash>(value: &T) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

#[test]
fn debug_through_a_base_handle_shows_the_most_derived_value() {
    let s: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();
    let text = format!("{s:?}");
    assert!(text.contains("Square"), "{text}");
    assert!(text.contains("side: 2"), "{text}");
    assert!(text.contains("x: 1"), "the inherited field too: {text}");
}

#[test]
fn display_through_a_base_handle() {
    let shapes: Vec<Obj<Shape>> = vec![
        Obj::<Square>::new(square(0, 3)).upcast(),
        Obj::<Circle>::new(circle(0, 4)).upcast(),
        Obj::<Rounded>::new(rounded(0, 5, 1)).upcast(),
    ];
    let rendered: Vec<String> = shapes.iter().map(ToString::to_string).collect();
    assert_eq!(rendered, ["square(3)", "circle(4)", "rounded(5, 1)"]);
}

#[test]
fn clone_preserves_the_most_derived_class() {
    let original: Obj<Shape> = Obj::<Rounded>::new(rounded(1, 5, 2)).upcast();
    let copy = original.clone();

    assert_eq!(copy.class().name, "Rounded", "not sliced to Shape");
    assert_eq!(copy.area(), original.area(), "the override still runs");
    assert!(copy.borrow().cast::<Rounded>().is_some(), "still a Rounded");
}

#[test]
fn clone_is_a_deep_copy() {
    let original = Obj::<Square>::new(square(1, 2));
    let mut copy = original.clone();
    copy.shift(10);

    assert_eq!(copy.x, 11, "the copy moved");
    assert_eq!(original.x, 1, "the original did not");
}

#[test]
fn clone_obj_from_every_handle() {
    let owned = Obj::<Square>::new(square(1, 2));

    let from_ref: Obj<Square> = owned.borrow().clone_obj();
    assert_eq!(from_ref.side, 2);

    let mut owned_mut = Obj::<Square>::new(square(1, 2));
    let from_ref_mut: Obj<Square> = owned_mut.borrow_mut().clone_obj();
    assert_eq!(from_ref_mut.side, 2);

    // `Shared::clone` shares; `clone_obj` copies.
    let shared = Shared::<Square>::new(square(1, 2));
    let shared_alias = shared.clone();
    assert_eq!(shared.strong_count(), 2, "clone shared the object");
    let deep: Obj<Square> = shared.clone_obj();
    assert_eq!(shared.strong_count(), 2, "clone_obj did not share it");
    assert_eq!(deep.side, 2);
    drop(shared_alias);

    let arc = ArcShared::<Square>::new(square(1, 2));
    let deep: Obj<Square> = arc.clone_obj();
    assert_eq!(arc.strong_count(), 1);
    assert_eq!(deep.side, 2);
}

#[test]
fn equality_needs_the_same_class_and_the_same_fields() {
    let a: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();
    let b: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();
    let different_field: Obj<Shape> = Obj::<Square>::new(square(1, 3)).upcast();
    let different_base_field: Obj<Shape> = Obj::<Square>::new(square(9, 2)).upcast();

    assert_eq!(a, b, "same class, same fields");
    assert_ne!(a, different_field, "own field differs");
    assert_ne!(a, different_base_field, "inherited field differs");
}

#[test]
fn objects_of_different_classes_are_never_equal() {
    // `Square { x: 1, side: 2 }` and `Circle { x: 1, r: 2 }` have identical layouts and identical
    // field values, so only the class identity separates them.
    let sq: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();
    let ci: Obj<Shape> = Obj::<Circle>::new(circle(1, 2)).upcast();

    assert_ne!(sq, ci);
    assert_ne!(ci, sq, "and symmetrically");
}

#[test]
fn a_subclass_never_equals_its_base_slice() {
    // `Rounded` starts with a `Square` whose fields match, which is exactly the comparison C++
    // slicing would get wrong.
    let sq: Obj<Shape> = Obj::<Square>::new(square(1, 5)).upcast();
    let ro: Obj<Shape> = Obj::<Rounded>::new(rounded(1, 5, 0)).upcast();

    assert_ne!(sq, ro);
}

#[test]
fn equal_objects_hash_equal() {
    let a: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();
    let b: Obj<Shape> = Obj::<Square>::new(square(1, 2)).upcast();

    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b), "Hash agrees with Eq");
}

#[test]
fn handles_work_as_hash_map_keys() {
    let mut map: HashMap<Obj<Shape>, &str> = HashMap::new();
    map.insert(Obj::<Square>::new(square(1, 2)).upcast(), "square");
    map.insert(Obj::<Circle>::new(circle(1, 2)).upcast(), "circle");
    // Same class and fields as the first key: replaces it.
    map.insert(Obj::<Square>::new(square(1, 2)).upcast(), "square again");

    assert_eq!(map.len(), 2, "the identical Square collapsed onto its twin");
    let lookup: Obj<Shape> = Obj::<Circle>::new(circle(1, 2)).upcast();
    assert_eq!(map.get(&lookup).copied(), Some("circle"));
}

#[test]
fn comparison_works_through_a_borrowed_handle_too() {
    let a = Obj::<Square>::new(square(1, 2));
    let b = Obj::<Square>::new(square(1, 2));
    let ra: Ref<'_, Shape> = a.borrow().upcast();
    let rb: Ref<'_, Shape> = b.borrow().upcast();

    assert_eq!(ra, rb);
    assert_eq!(hash_of(&ra), hash_of(&rb));
    assert_eq!(format!("{ra}"), "square(2)");
}

#[test]
fn shared_handles_forward_the_traits() {
    let a = Shared::<Square>::new(square(1, 2));
    let b = Shared::<Square>::new(square(1, 2));
    assert_eq!(a, b);
    assert_eq!(format!("{a}"), "square(2)");
    assert!(format!("{a:?}").contains("Square"));

    let arc = ArcShared::<Square>::new(square(1, 2));
    assert_eq!(format!("{arc}"), "square(2)");
    assert!(format!("{arc:?}").contains("Square"));
}
