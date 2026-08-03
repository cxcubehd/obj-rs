//! Multiple inheritance: several bases, each carrying its own fields and virtual methods.
//!
//! ```text
//!   Shape (abstract)   Drawable (abstract)   Named (abstract)
//!         \                  |                  /
//!          `------------- Circle --------------'
//!                            |
//!                       DashedCircle
//! ```

#![allow(missing_docs)]

use obj::{Class, Obj, Ref};

#[obj::class(abstract)]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn area(&self) -> f64;

    #[obj(virtual)]
    fn scale(&mut self, k: f64) {
        self.x *= k;
    }
}

#[obj::class(abstract)]
pub struct Drawable {
    pub visible: bool,
}

#[obj::methods]
impl Drawable {
    #[obj(virtual)]
    fn draw(&self) -> String;

    /// Virtual with a body, so a subclass may inherit it unchanged.
    #[obj(virtual)]
    fn hide(&mut self) {
        self.visible = false;
    }
}

#[obj::class(abstract)]
pub struct Named {
    pub name: &'static str,
}

#[obj::methods]
impl Named {
    #[obj(virtual)]
    fn label(&self) -> String {
        format!("<{}>", self.name)
    }
}

/// Primary base `Shape` sits at offset 0; `Drawable` and `Named` follow at real offsets.
#[obj::class(extends(Shape, Drawable, Named))]
pub struct Circle {
    pub r: f64,
}

#[obj::methods]
impl Circle {
    #[obj(override)]
    fn area(&self) -> f64 {
        core::f64::consts::PI * self.r * self.r
    }

    #[obj(override)]
    fn draw(&self) -> String {
        format!("circle r={}", self.r)
    }
}

#[obj::class(extends = Circle)]
pub struct DashedCircle {
    pub dashes: u32,
}

#[obj::methods]
impl DashedCircle {
    #[obj(override)]
    fn draw(&self) -> String {
        format!("dashed({}) r={}", self.dashes, self.r)
    }
}

impl Circle {
    fn make(x: f64, r: f64, name: &'static str) -> Circle {
        Circle {
            shape: Shape { x },
            drawable: Drawable { visible: true },
            named: Named { name },
            r,
        }
    }
}

impl DashedCircle {
    fn make(x: f64, r: f64, dashes: u32) -> DashedCircle {
        DashedCircle {
            circle: Circle::make(x, r, "dashed"),
            dashes,
        }
    }
}

const EPS: f64 = 1e-9;

#[test]
fn fields_from_every_base_are_reachable() {
    let c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    assert!((c.r - 2.0).abs() < EPS, "own field");
    assert!((c.x - 1.0).abs() < EPS, "primary base field, through Deref");
    // Secondary bases are not `Deref` targets, so they get named accessors.
    assert!(c.as_drawable().visible, "secondary base field");
    assert_eq!(c.as_named().name, "c", "third base field");
}

#[test]
fn virtuals_from_every_base_dispatch() {
    let c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    assert!(
        (c.area() - core::f64::consts::PI * 4.0).abs() < EPS,
        "Shape's"
    );
    assert_eq!(c.draw(), "circle r=2", "Drawable's");
    assert_eq!(c.label(), "<c>", "Named's, inherited unchanged");
}

#[test]
fn upcast_to_any_base_keeps_virtual_dispatch() {
    let c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    let shape: Obj<Shape> = c.upcast();
    assert!((shape.area() - core::f64::consts::PI * 4.0).abs() < EPS);

    let c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    let drawable: Obj<Drawable> = c.upcast();
    assert_eq!(drawable.draw(), "circle r=2");
    assert!(drawable.visible, "field of the base we upcast to");

    let c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    let named: Obj<Named> = c.upcast();
    assert_eq!(named.label(), "<c>");
}

#[test]
fn sidecast_between_unrelated_bases() {
    let shape: Obj<Shape> = Obj::<Circle>::new(Circle::make(1.0, 4.0, "c")).upcast();

    // Shape and Drawable are unrelated: neither interface is a supertrait of the other, so this
    // rebuilds the fat pointer from the base table.
    let d: Ref<'_, Drawable> = shape.borrow().cast_obj::<Drawable>().expect("is Drawable");
    assert_eq!(
        d.draw(),
        "circle r=4",
        "the override runs, not Drawable's own"
    );
    assert!(d.visible);

    let n: Ref<'_, Named> = shape.borrow().cast_obj::<Named>().expect("is Named");
    assert_eq!(n.label(), "<c>");

    // The plain-data view of a secondary base applies the byte offset instead.
    let data: &Drawable = shape.borrow().cast::<Drawable>().expect("is Drawable");
    assert!(data.visible);
}

#[test]
fn sidecast_reaches_an_override_declared_further_down() {
    let shape: Obj<Shape> = Obj::<DashedCircle>::new(DashedCircle::make(0.0, 3.0, 5)).upcast();
    let d: Ref<'_, Drawable> = shape.borrow().cast_obj::<Drawable>().expect("is Drawable");
    assert_eq!(d.draw(), "dashed(5) r=3", "DashedCircle's override wins");
}

#[test]
fn base_table_covers_every_branch() {
    let meta = <Circle as Class>::META;
    assert_eq!(meta.bases.len(), 4, "Shape, Drawable, Named, Circle");
    for id in [
        core::any::TypeId::of::<Shape>(),
        core::any::TypeId::of::<Drawable>(),
        core::any::TypeId::of::<Named>(),
        core::any::TypeId::of::<Circle>(),
    ] {
        assert!(meta.is_a(id));
    }

    // The secondary bases really are at non-zero offsets.
    let drawable = meta
        .find_base(core::any::TypeId::of::<Drawable>())
        .expect("present");
    assert_ne!(drawable.data_offset, 0);
    let shape = meta
        .find_base(core::any::TypeId::of::<Shape>())
        .expect("present");
    assert_eq!(shape.data_offset, 0, "primary base is at offset 0");

    // ...and a subclass inherits all of them.
    assert_eq!(<DashedCircle as Class>::META.bases.len(), 5);
}

#[test]
fn subclass_of_a_multiply_inheriting_class() {
    let dc = Obj::<DashedCircle>::new(DashedCircle::make(1.0, 3.0, 4));
    assert_eq!(dc.draw(), "dashed(4) r=3", "own override");
    assert!(
        (dc.area() - core::f64::consts::PI * 9.0).abs() < EPS,
        "grandparent's"
    );
    assert_eq!(dc.label(), "<dashed>", "inherited from a secondary base");
    assert!(dc.as_drawable().visible);

    // Upcast across the whole graph.
    let named: Obj<Named> = dc.upcast();
    assert_eq!(named.label(), "<dashed>");
}

#[test]
fn mutation_through_a_secondary_base_handle() {
    let mut c = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c"));
    c.hide();
    assert!(!c.as_drawable().visible, "Drawable::hide ran");

    c.scale(3.0);
    assert!((c.x - 3.0).abs() < EPS, "Shape::scale ran");
}

#[test]
fn downcast_from_a_secondary_base_to_the_concrete_class() {
    let drawable: Obj<Drawable> = Obj::<Circle>::new(Circle::make(1.0, 2.0, "c")).upcast();
    let circle: &Circle = drawable.borrow().cast::<Circle>().expect("is a Circle");
    assert!((circle.r - 2.0).abs() < EPS);
    assert!(drawable.is::<Shape>(), "and it is a Shape too");
}
