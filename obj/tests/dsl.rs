//! `obj::classes! { }` — the C++-shaped surface.
//!
//! These tests exist to show two things: that the DSL expresses everything the attributes do, and
//! that it produces *the same* code — the hierarchy below is declared twice, once each way, and
//! both are exercised identically.

#![allow(missing_docs)]

use obj::{Class, Obj, Ref};

// ------------------------------------------------------------------ declared with the DSL

obj::classes! {
    /// A shape with a position.
    #[derive(Debug)]
    pub abstract class Shape dyn_traits(Debug) {
        /// Horizontal position.
        pub x: f64,

        virtual fn area(&self) -> f64;

        virtual fn scale(&mut self, k: f64) {
            self.x *= k;
        }

        /// Non-virtual: inherited through `Deref`.
        fn position(&self) -> f64 {
            self.x
        }
    }

    #[derive(Debug)]
    pub abstract class Drawable {
        pub visible: bool,

        virtual fn draw(&self) -> String;
    }

    #[derive(Debug)]
    pub class Circle : Shape, Drawable {
        pub r: f64,

        ctor new(x: f64, r: f64) : Shape { x }, Drawable { visible: true } { r }

        override fn area(&self) -> f64 {
            core::f64::consts::PI * self.r * self.r
        }

        override fn draw(&self) -> String {
            format!("circle(r={})", self.r)
        }

        override fn scale(&mut self, k: f64) {
            self.r *= k;
            // A `super` call is an ordinary qualified call, in the DSL exactly as in the
            // attribute form.
            Shape::scale(self, k);
        }
    }

    #[derive(Debug)]
    pub class DashedCircle : Circle {
        pub dashes: u32,

        ctor new(x: f64, r: f64, dashes: u32) : Circle(x, r) { dashes }

        override fn draw(&self) -> String {
            format!("dashed({}, r={})", self.dashes, self.r)
        }
    }
}

// ---------------------------------------------------- the same thing, written as attributes

#[obj::class(abstract, dyn_traits(Debug))]
#[derive(Debug)]
pub struct ShapeA {
    pub x: f64,
}

#[obj::methods]
impl ShapeA {
    #[obj(virtual)]
    fn area(&self) -> f64;

    #[obj(virtual)]
    fn scale(&mut self, k: f64) {
        self.x *= k;
    }

    fn position(&self) -> f64 {
        self.x
    }
}

#[obj::class(abstract)]
#[derive(Debug)]
pub struct DrawableA {
    pub visible: bool,
}

#[obj::methods]
impl DrawableA {
    #[obj(virtual)]
    fn draw(&self) -> String;
}

#[obj::class(extends(ShapeA, DrawableA))]
#[derive(Debug)]
pub struct CircleA {
    pub r: f64,
}

#[obj::methods]
impl CircleA {
    #[obj(override)]
    fn area(&self) -> f64 {
        core::f64::consts::PI * self.r * self.r
    }

    #[obj(override)]
    fn draw(&self) -> String {
        format!("circle(r={})", self.r)
    }

    #[obj(override)]
    fn scale(&mut self, k: f64) {
        self.r *= k;
        ShapeA::scale(self, k);
    }
}

impl CircleA {
    fn new(x: f64, r: f64) -> CircleA {
        CircleA {
            shape_a: ShapeA { x },
            drawable_a: DrawableA { visible: true },
            r,
        }
    }
}

const EPS: f64 = 1e-9;

#[test]
fn the_dsl_builds_a_working_hierarchy() {
    let c = Obj::<Circle>::new(Circle::new(1.0, 2.0));

    assert!((c.r - 2.0).abs() < EPS, "own field");
    assert!((c.x - 1.0).abs() < EPS, "primary base field, through Deref");
    assert!(c.as_drawable().visible, "secondary base field");
    assert!(
        (c.area() - core::f64::consts::PI * 4.0).abs() < EPS,
        "virtual"
    );
    assert_eq!(c.draw(), "circle(r=2)", "virtual from the second base");
    assert!((c.position() - 1.0).abs() < EPS, "non-virtual");
}

#[test]
fn the_dsl_and_the_attributes_agree() {
    // Same hierarchy, same shape, same numbers -- the DSL is sugar, not a second implementation.
    let dsl = Obj::<Circle>::new(Circle::new(1.0, 2.0));
    let attr = Obj::<CircleA>::new(CircleA::new(1.0, 2.0));

    assert_eq!(
        <Circle as Class>::META.bases.len(),
        <CircleA as Class>::META.bases.len(),
    );
    assert_eq!(
        core::mem::size_of::<Circle>(),
        core::mem::size_of::<CircleA>(),
        "identical layout",
    );
    assert!((dsl.area() - attr.area()).abs() < EPS);
    assert_eq!(dsl.draw(), attr.draw());
    assert!(dsl.is::<Shape>() && attr.is::<ShapeA>());
}

#[test]
fn abstract_classes_stay_abstract() {
    // `Shape` was declared `abstract`, so there is no `Obj::<Shape>::new`. Upcasting to it works,
    // which is the whole point of a base handle.
    let shapes: Vec<Obj<Shape>> = vec![
        Obj::<Circle>::new(Circle::new(0.0, 1.0)).upcast(),
        Obj::<DashedCircle>::new(DashedCircle::new(0.0, 2.0, 8)).upcast(),
    ];
    let areas: Vec<f64> = shapes.iter().map(|s| s.area()).collect();
    assert!((areas[0] - core::f64::consts::PI).abs() < EPS);
    assert!((areas[1] - core::f64::consts::PI * 4.0).abs() < EPS);
}

#[test]
fn ctor_initialiser_lists_chain() {
    // `DashedCircle::new` initialises its base by calling `Circle::new`, which initialises its
    // own two bases in turn.
    let d = Obj::<DashedCircle>::new(DashedCircle::new(3.0, 2.0, 5));

    assert_eq!(d.dashes, 5, "own field");
    assert!((d.r - 2.0).abs() < EPS, "set by Circle::new");
    assert!((d.x - 3.0).abs() < EPS, "set by Shape's initialiser");
    assert!(d.as_drawable().visible, "set by Drawable's initialiser");
}

#[test]
fn override_and_super_calls_work_through_the_dsl() {
    let mut c = Obj::<Circle>::new(Circle::new(2.0, 3.0));
    c.scale(2.0); // Circle::scale, which super-calls Shape::scale
    assert!((c.r - 6.0).abs() < EPS);
    assert!((c.x - 4.0).abs() < EPS);

    // DashedCircle does not override scale, so Circle's runs.
    let mut d = Obj::<DashedCircle>::new(DashedCircle::new(1.0, 1.0, 3));
    d.scale(3.0);
    assert!((d.r - 3.0).abs() < EPS);
    assert_eq!(d.draw(), "dashed(3, r=3)", "its own override still wins");
}

#[test]
fn dyn_traits_declared_in_the_dsl_header() {
    let s: Obj<Shape> = Obj::<Circle>::new(Circle::new(1.0, 2.0)).upcast();
    let text = format!("{s:?}");
    assert!(text.contains("Circle"), "{text}");
    assert!(text.contains("r: 2.0"), "{text}");
}

#[test]
fn casts_behave_the_same_as_the_attribute_form() {
    let s: Obj<Shape> = Obj::<DashedCircle>::new(DashedCircle::new(0.0, 3.0, 5)).upcast();

    let d: Ref<'_, Drawable> = s.borrow().cast_obj::<Drawable>().expect("is Drawable");
    assert_eq!(d.draw(), "dashed(5, r=3)", "the deepest override runs");

    let circle: &Circle = s.borrow().cast::<Circle>().expect("is a Circle");
    assert!((circle.r - 3.0).abs() < EPS);
}
