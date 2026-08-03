//! Single inheritance through the attribute macros.
//!
//! The hierarchy is three levels deep on purpose: `Square` names only `Polygon`, and must still
//! end up with a working `Shape` entry in its base table. That is the case the recursive
//! `__obj_ancestors_*` chain exists to solve.

#![allow(missing_docs)]

use obj::{Class, Obj, Ref};

#[obj::class(abstract)]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    /// Pure virtual: `Shape` cannot be instantiated.
    #[obj(virtual)]
    fn area(&self) -> f64;

    /// Virtual with a default body.
    #[obj(virtual)]
    fn scale(&mut self, k: f64) {
        self.x *= k;
    }

    /// Non-virtual: inherited through `Deref`.
    fn describe(&self) -> String {
        format!("at x={}", self.x)
    }
}

#[obj::class(extends = Shape, abstract)]
pub struct Polygon {
    pub sides: u32,
}

#[obj::methods]
impl Polygon {
    #[obj(virtual)]
    fn perimeter(&self) -> f64;
}

#[obj::class(extends = Polygon)]
pub struct Square {
    pub side: f64,
}

#[obj::methods]
impl Square {
    #[obj(override)]
    fn area(&self) -> f64 {
        self.side * self.side
    }

    #[obj(override)]
    fn perimeter(&self) -> f64 {
        4.0 * self.side
    }

    #[obj(override)]
    fn scale(&mut self, k: f64) {
        self.side *= k;
        // `super` call: a plain qualified call to the base's own implementation.
        Shape::scale(self, k);
    }
}

/// A fourth level that overrides nothing, to prove inherited overrides still win.
#[obj::class(extends = Square)]
pub struct ColoredSquare {
    pub color: &'static str,
}

#[obj::methods]
impl ColoredSquare {}

impl Square {
    fn make(x: f64, side: f64) -> Square {
        Square {
            polygon: Polygon {
                shape: Shape { x },
                sides: 4,
            },
            side,
        }
    }
}

impl ColoredSquare {
    fn make(x: f64, side: f64, color: &'static str) -> ColoredSquare {
        ColoredSquare {
            square: Square::make(x, side),
            color,
        }
    }
}

const EPS: f64 = 1e-9;

#[test]
fn fields_and_methods_across_three_levels() {
    let sq = Obj::<Square>::new(Square::make(1.0, 3.0));
    assert!((sq.side - 3.0).abs() < EPS, "own field");
    assert_eq!(sq.sides, 4, "parent field");
    assert!((sq.x - 1.0).abs() < EPS, "grandparent field");
    assert_eq!(sq.describe(), "at x=1", "grandparent non-virtual method");
    assert!((sq.area() - 9.0).abs() < EPS, "virtual");
    assert!((sq.perimeter() - 12.0).abs() < EPS, "parent's virtual");
}

#[test]
fn upcast_to_grandparent_keeps_virtual_dispatch() {
    let sq = Obj::<Square>::new(Square::make(1.0, 3.0));
    let shape: Obj<Shape> = sq.upcast();
    assert!((shape.area() - 9.0).abs() < EPS, "Square::area still runs");
    assert!((shape.x - 1.0).abs() < EPS);
    assert_eq!(shape.class().name, "Square");
}

#[test]
fn base_table_contains_every_ancestor() {
    // Square's macro named only `Polygon`; `Shape` arrived through the recursive chain.
    let bases = <Square as Class>::META.bases;
    assert_eq!(bases.len(), 3, "Shape, Polygon, Square");
    assert!(<Square as Class>::META.is_a(core::any::TypeId::of::<Shape>()));
    assert!(<Square as Class>::META.is_a(core::any::TypeId::of::<Polygon>()));

    let four = <ColoredSquare as Class>::META;
    assert_eq!(four.bases.len(), 4);
    assert!(four.is_a(core::any::TypeId::of::<Shape>()));
}

#[test]
fn downcast_to_a_distant_ancestor_is_polymorphic() {
    let obj: Obj<Shape> = Obj::<ColoredSquare>::new(ColoredSquare::make(1.0, 3.0, "red")).upcast();

    // Polymorphic cast to a class two levels below the handle's static type and two above the
    // most-derived one. Its vtable must still dispatch to ColoredSquare's inherited override.
    let poly: Ref<'_, Polygon> = obj.borrow().cast_obj::<Polygon>().expect("is a Polygon");
    assert!((poly.area() - 9.0).abs() < EPS);
    assert!((poly.perimeter() - 12.0).abs() < EPS);
    assert_eq!(poly.sides, 4);

    let sq: &Square = obj.borrow().cast::<Square>().expect("is a Square");
    assert!((sq.side - 3.0).abs() < EPS);

    let cs: &ColoredSquare = obj.borrow().cast::<ColoredSquare>().expect("most-derived");
    assert_eq!(cs.color, "red");
}

#[test]
fn inherited_override_wins_for_a_class_that_overrides_nothing() {
    // ColoredSquare defines no methods at all; Square's overrides must still be used.
    let cs = Obj::<ColoredSquare>::new(ColoredSquare::make(2.0, 5.0, "blue"));
    assert!((cs.area() - 25.0).abs() < EPS);
    assert!((cs.perimeter() - 20.0).abs() < EPS);
    let shape: Obj<Shape> = cs.upcast();
    assert!((shape.area() - 25.0).abs() < EPS);
}

#[test]
fn override_with_super_call_runs_both_levels() {
    let mut sq = Obj::<Square>::new(Square::make(3.0, 5.0));
    sq.scale(2.0);
    assert!((sq.side - 10.0).abs() < EPS, "Square::scale ran");
    assert!((sq.x - 6.0).abs() < EPS, "super call to Shape::scale ran");
}

#[test]
fn non_overridden_virtual_falls_back_to_the_base_body() {
    // ColoredSquare inherits Square::scale, which itself calls Shape::scale.
    let mut cs = Obj::<ColoredSquare>::new(ColoredSquare::make(3.0, 5.0, "g"));
    cs.scale(2.0);
    assert!((cs.side - 10.0).abs() < EPS);
    assert!((cs.x - 6.0).abs() < EPS);
}

#[test]
fn heterogeneous_collection() {
    let shapes: Vec<Obj<Shape>> = vec![
        Obj::<Square>::new(Square::make(0.0, 2.0)).upcast(),
        Obj::<ColoredSquare>::new(ColoredSquare::make(0.0, 3.0, "red")).upcast(),
    ];
    let total: f64 = shapes.iter().map(|s| s.area()).sum();
    assert!((total - 13.0).abs() < EPS);
    let names: Vec<&str> = shapes.iter().map(|s| s.class().name).collect();
    assert_eq!(names, ["Square", "ColoredSquare"]);
}

#[test]
fn static_view_is_non_virtual() {
    let sq = Square::make(1.0, 3.0);
    // Inherent method on a plain reference: C++ `obj.f()`.
    assert!((sq.area() - 9.0).abs() < EPS);
    assert!((sq.x - 1.0).abs() < EPS, "field through the Deref chain");
}
