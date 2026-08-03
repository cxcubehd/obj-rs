//! The reference hierarchy, written out by hand exactly as `#[obj::class]` will generate it.
//!
//! This file is the specification for macro output: every construct the macros emit appears here,
//! spelled the way they must spell it. When a generated-code question comes up, the answer is
//! whatever makes this file's semantics reproduce.
//!
//! The hierarchy is:
//!
//! ```text
//! Shape (abstract)      Drawable (abstract)
//!   |        \             /
//! Square      Circle ------
//! ```

#![allow(clippy::undocumented_unsafe_blocks, missing_docs)]

use core::any::TypeId;
use core::ops::{Deref, DerefMut};

use obj::{AnyObj, BaseEntry, Class, ClassMeta, Concrete, Obj, Ref, SubclassOf};

// =====================================================================================
// class Shape { x: f64; virtual area() = 0; virtual scale(k); fn describe(); }
// =====================================================================================

#[repr(C)]
#[derive(Debug)]
pub struct Shape {
    pub x: f64,
}

/// Generated interface. Supertrait chain mirrors the class chain.
pub trait ShapeDyn: AnyObj + core::fmt::Debug {
    fn area(&self) -> f64;
    fn scale(&mut self, k: f64);
    #[doc(hidden)]
    fn __shape_data(&self) -> &Shape;
    #[doc(hidden)]
    fn __shape_data_mut(&mut self) -> &mut Shape;
}

// Field access through any polymorphic handle to a Shape.
impl Deref for dyn ShapeDyn {
    type Target = Shape;
    fn deref(&self) -> &Shape {
        self.__shape_data()
    }
}
impl DerefMut for dyn ShapeDyn {
    fn deref_mut(&mut self) -> &mut Shape {
        self.__shape_data_mut()
    }
}

// Inherent methods = this class's own implementations. Static dispatch; also the target of
// `super` calls from subclasses.
impl Shape {
    pub fn new(x: f64) -> Self {
        Shape { x }
    }

    /// Virtual with a default body.
    pub fn scale(&mut self, k: f64) {
        self.x *= k;
    }

    /// Non-virtual: inherited by subclasses through the `Deref` chain, never overridable.
    pub fn describe(&self) -> String {
        format!("at x={}", self.x)
    }
}

// Abstract: no `Concrete` impl, and every vtable slot is `None`.
unsafe impl Class for Shape {
    type Dyn = dyn ShapeDyn;
    type Complete = Shape;
    const META: &'static ClassMeta = &ClassMeta {
        name: "Shape",
        id: TypeId::of::<Shape>,
        bases: &[BaseEntry {
            id: TypeId::of::<Shape>,
            data_offset: 0,
            dyn_vtable: None,
        }],
    };
}

unsafe impl SubclassOf<Shape> for Shape {
    fn up_box(this: Box<dyn ShapeDyn>) -> Box<dyn ShapeDyn> {
        this
    }
    fn up_ref<'a>(this: &'a (dyn ShapeDyn + 'static)) -> &'a (dyn ShapeDyn + 'static) {
        this
    }
    fn up_mut<'a>(this: &'a mut (dyn ShapeDyn + 'static)) -> &'a mut (dyn ShapeDyn + 'static) {
        this
    }
}

// =====================================================================================
// class Drawable { visible: bool; virtual draw() = 0; }   -- second base of Circle
// =====================================================================================

#[repr(C)]
#[derive(Debug)]
pub struct Drawable {
    pub visible: bool,
}

pub trait DrawableDyn: AnyObj {
    fn draw(&self) -> String;
    #[doc(hidden)]
    fn __drawable_data(&self) -> &Drawable;
    #[doc(hidden)]
    fn __drawable_data_mut(&mut self) -> &mut Drawable;
}

impl Deref for dyn DrawableDyn {
    type Target = Drawable;
    fn deref(&self) -> &Drawable {
        self.__drawable_data()
    }
}
impl DerefMut for dyn DrawableDyn {
    fn deref_mut(&mut self) -> &mut Drawable {
        self.__drawable_data_mut()
    }
}

impl Default for Drawable {
    fn default() -> Self {
        Self::new()
    }
}

impl Drawable {
    pub fn new() -> Self {
        Drawable { visible: true }
    }
}

unsafe impl Class for Drawable {
    type Dyn = dyn DrawableDyn;
    type Complete = Drawable;
    const META: &'static ClassMeta = &ClassMeta {
        name: "Drawable",
        id: TypeId::of::<Drawable>,
        bases: &[BaseEntry {
            id: TypeId::of::<Drawable>,
            data_offset: 0,
            dyn_vtable: None,
        }],
    };
}

// =====================================================================================
// class Circle : Shape, Drawable { r: f64; override area(); override draw(); }
// =====================================================================================

#[repr(C)]
#[derive(Debug)]
pub struct Circle {
    pub __base_shape: Shape,
    pub __base_drawable: Drawable,
    pub r: f64,
}

// The primary base must sit at offset 0 so a `&Circle` is also a valid `&Shape`.
const _: () = assert!(core::mem::offset_of!(Circle, __base_shape) == 0);

pub trait CircleDyn: ShapeDyn + DrawableDyn {
    #[doc(hidden)]
    fn __circle_data(&self) -> &Circle;
    #[doc(hidden)]
    fn __circle_data_mut(&mut self) -> &mut Circle;
}

impl Deref for dyn CircleDyn {
    type Target = Circle;
    fn deref(&self) -> &Circle {
        self.__circle_data()
    }
}
impl DerefMut for dyn CircleDyn {
    fn deref_mut(&mut self) -> &mut Circle {
        self.__circle_data_mut()
    }
}

// Deref to the primary base: inherits Shape's fields and non-virtual methods.
impl Deref for Circle {
    type Target = Shape;
    fn deref(&self) -> &Shape {
        &self.__base_shape
    }
}
impl DerefMut for Circle {
    fn deref_mut(&mut self) -> &mut Shape {
        &mut self.__base_shape
    }
}

impl Circle {
    pub fn new(x: f64, r: f64) -> Self {
        Circle {
            __base_shape: Shape::new(x),
            __base_drawable: Drawable::new(),
            r,
        }
    }

    /// Secondary bases get an accessor, since `Deref` can only target one.
    pub fn as_drawable(&self) -> &Drawable {
        &self.__base_drawable
    }

    /// `override fn area`
    pub fn area(&self) -> f64 {
        core::f64::consts::PI * self.r * self.r
    }

    /// `override fn draw` — from the *second* base.
    pub fn draw(&self) -> String {
        format!("circle r={}", self.r)
    }

    // note: `scale` is deliberately NOT overridden; it must still dispatch to Shape::scale.
}

unsafe impl AnyObj for Circle {
    fn class_meta(&self) -> &'static ClassMeta {
        <Circle as Class>::META
    }
    fn obj_addr(&self) -> *const u8 {
        (self as *const Self).cast::<u8>()
    }
}

impl ShapeDyn for Circle {
    fn area(&self) -> f64 {
        Circle::area(self)
    }
    fn scale(&mut self, k: f64) {
        // not overridden => forward to the base's own implementation
        Shape::scale(&mut self.__base_shape, k);
    }
    fn __shape_data(&self) -> &Shape {
        &self.__base_shape
    }
    fn __shape_data_mut(&mut self) -> &mut Shape {
        &mut self.__base_shape
    }
}

impl DrawableDyn for Circle {
    fn draw(&self) -> String {
        Circle::draw(self)
    }
    fn __drawable_data(&self) -> &Drawable {
        &self.__base_drawable
    }
    fn __drawable_data_mut(&mut self) -> &mut Drawable {
        &mut self.__base_drawable
    }
}

impl CircleDyn for Circle {
    fn __circle_data(&self) -> &Circle {
        self
    }
    fn __circle_data_mut(&mut self) -> &mut Circle {
        self
    }
}

unsafe impl Class for Circle {
    type Dyn = dyn CircleDyn;
    type Complete = Circle;
    const META: &'static ClassMeta = &ClassMeta {
        name: "Circle",
        id: TypeId::of::<Circle>,
        bases: &[
            BaseEntry {
                id: TypeId::of::<Circle>,
                data_offset: 0,
                dyn_vtable: Some(obj::__vtable_of!(Circle as dyn CircleDyn)),
            },
            BaseEntry {
                id: TypeId::of::<Shape>,
                data_offset: core::mem::offset_of!(Circle, __base_shape),
                dyn_vtable: Some(obj::__vtable_of!(Circle as dyn ShapeDyn)),
            },
            BaseEntry {
                id: TypeId::of::<Drawable>,
                data_offset: core::mem::offset_of!(Circle, __base_drawable),
                dyn_vtable: Some(obj::__vtable_of!(Circle as dyn DrawableDyn)),
            },
        ],
    };
}

unsafe impl Concrete for Circle {
    fn into_dyn(value: Box<Circle>) -> Box<dyn CircleDyn> {
        value
    }
    fn as_dyn(value: &Circle) -> &(dyn CircleDyn + 'static) {
        value
    }
    fn as_dyn_mut(value: &mut Circle) -> &mut (dyn CircleDyn + 'static) {
        value
    }
}

unsafe impl SubclassOf<Circle> for Circle {
    fn up_box(this: Box<dyn CircleDyn>) -> Box<dyn CircleDyn> {
        this
    }
    fn up_ref<'a>(this: &'a (dyn CircleDyn + 'static)) -> &'a (dyn CircleDyn + 'static) {
        this
    }
    fn up_mut<'a>(this: &'a mut (dyn CircleDyn + 'static)) -> &'a mut (dyn CircleDyn + 'static) {
        this
    }
}

unsafe impl SubclassOf<Shape> for Circle {
    fn up_box(this: Box<dyn CircleDyn>) -> Box<dyn ShapeDyn> {
        this
    }
    fn up_ref<'a>(this: &'a (dyn CircleDyn + 'static)) -> &'a (dyn ShapeDyn + 'static) {
        this
    }
    fn up_mut<'a>(this: &'a mut (dyn CircleDyn + 'static)) -> &'a mut (dyn ShapeDyn + 'static) {
        this
    }
}

unsafe impl SubclassOf<Drawable> for Circle {
    fn up_box(this: Box<dyn CircleDyn>) -> Box<dyn DrawableDyn> {
        this
    }
    fn up_ref<'a>(this: &'a (dyn CircleDyn + 'static)) -> &'a (dyn DrawableDyn + 'static) {
        this
    }
    fn up_mut<'a>(this: &'a mut (dyn CircleDyn + 'static)) -> &'a mut (dyn DrawableDyn + 'static) {
        this
    }
}

// =====================================================================================
// class Square : Shape { s: f64; override area(); override scale(); }
// =====================================================================================

#[repr(C)]
#[derive(Debug)]
pub struct Square {
    pub __base_shape: Shape,
    pub s: f64,
}

const _: () = assert!(core::mem::offset_of!(Square, __base_shape) == 0);

pub trait SquareDyn: ShapeDyn {
    #[doc(hidden)]
    fn __square_data(&self) -> &Square;
    #[doc(hidden)]
    fn __square_data_mut(&mut self) -> &mut Square;
}

impl Deref for dyn SquareDyn {
    type Target = Square;
    fn deref(&self) -> &Square {
        self.__square_data()
    }
}
impl DerefMut for dyn SquareDyn {
    fn deref_mut(&mut self) -> &mut Square {
        self.__square_data_mut()
    }
}

impl Deref for Square {
    type Target = Shape;
    fn deref(&self) -> &Shape {
        &self.__base_shape
    }
}
impl DerefMut for Square {
    fn deref_mut(&mut self) -> &mut Shape {
        &mut self.__base_shape
    }
}

impl Square {
    pub fn new(x: f64, s: f64) -> Self {
        Square {
            __base_shape: Shape::new(x),
            s,
        }
    }

    pub fn area(&self) -> f64 {
        self.s * self.s
    }

    /// `override fn scale` with a super call — a plain qualified call, no new syntax.
    pub fn scale(&mut self, k: f64) {
        self.s *= k;
        Shape::scale(self, k);
    }
}

unsafe impl AnyObj for Square {
    fn class_meta(&self) -> &'static ClassMeta {
        <Square as Class>::META
    }
    fn obj_addr(&self) -> *const u8 {
        (self as *const Self).cast::<u8>()
    }
}

impl ShapeDyn for Square {
    fn area(&self) -> f64 {
        Square::area(self)
    }
    fn scale(&mut self, k: f64) {
        Square::scale(self, k);
    }
    fn __shape_data(&self) -> &Shape {
        &self.__base_shape
    }
    fn __shape_data_mut(&mut self) -> &mut Shape {
        &mut self.__base_shape
    }
}

impl SquareDyn for Square {
    fn __square_data(&self) -> &Square {
        self
    }
    fn __square_data_mut(&mut self) -> &mut Square {
        self
    }
}

unsafe impl Class for Square {
    type Dyn = dyn SquareDyn;
    type Complete = Square;
    const META: &'static ClassMeta = &ClassMeta {
        name: "Square",
        id: TypeId::of::<Square>,
        bases: &[
            BaseEntry {
                id: TypeId::of::<Square>,
                data_offset: 0,
                dyn_vtable: Some(obj::__vtable_of!(Square as dyn SquareDyn)),
            },
            BaseEntry {
                id: TypeId::of::<Shape>,
                data_offset: core::mem::offset_of!(Square, __base_shape),
                dyn_vtable: Some(obj::__vtable_of!(Square as dyn ShapeDyn)),
            },
        ],
    };
}

unsafe impl Concrete for Square {
    fn into_dyn(value: Box<Square>) -> Box<dyn SquareDyn> {
        value
    }
    fn as_dyn(value: &Square) -> &(dyn SquareDyn + 'static) {
        value
    }
    fn as_dyn_mut(value: &mut Square) -> &mut (dyn SquareDyn + 'static) {
        value
    }
}

unsafe impl SubclassOf<Shape> for Square {
    fn up_box(this: Box<dyn SquareDyn>) -> Box<dyn ShapeDyn> {
        this
    }
    fn up_ref<'a>(this: &'a (dyn SquareDyn + 'static)) -> &'a (dyn ShapeDyn + 'static) {
        this
    }
    fn up_mut<'a>(this: &'a mut (dyn SquareDyn + 'static)) -> &'a mut (dyn ShapeDyn + 'static) {
        this
    }
}

/// Generic over any subclass of Shape, calling a virtual with no upcast needed.
pub trait IsShape: Class<Dyn: ShapeDyn> {}
impl IsShape for Shape {}
impl IsShape for Circle {}
impl IsShape for Square {}

fn total_area<C: IsShape>(shapes: &[Ref<'_, C>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

// =====================================================================================
// Tests
// =====================================================================================

const EPS: f64 = 1e-9;

#[test]
fn fields_and_methods_resolve_through_the_deref_chain() {
    let c = Obj::<Circle>::new(Circle::new(1.0, 2.0));
    assert!((c.r - 2.0).abs() < EPS, "own field");
    assert!((c.x - 1.0).abs() < EPS, "inherited field");
    assert_eq!(c.describe(), "at x=1", "inherited non-virtual method");
    assert!(
        (c.area() - core::f64::consts::PI * 4.0).abs() < EPS,
        "virtual"
    );
}

#[test]
fn virtual_dispatch_survives_upcast() {
    let c = Obj::<Circle>::new(Circle::new(1.0, 2.0));
    let s: Obj<Shape> = c.upcast();
    assert!((s.area() - core::f64::consts::PI * 4.0).abs() < EPS);
    assert!((s.x - 1.0).abs() < EPS, "base fields still reachable");
    assert_eq!(s.class().name, "Circle", "most-derived class is retained");
}

#[test]
fn non_overridden_virtual_falls_back_to_the_base_implementation() {
    let mut c = Obj::<Circle>::new(Circle::new(3.0, 2.0));
    c.scale(2.0);
    assert!((c.x - 6.0).abs() < EPS, "Shape::scale ran");
    assert!((c.r - 2.0).abs() < EPS, "r untouched");
}

#[test]
fn override_with_super_call_runs_both() {
    let mut sq = Obj::<Square>::new(Square::new(3.0, 5.0));
    sq.scale(2.0);
    assert!((sq.s - 10.0).abs() < EPS, "Square::scale ran");
    assert!((sq.x - 6.0).abs() < EPS, "super call to Shape::scale ran");
}

#[test]
fn downcast_to_concrete_and_intermediate() {
    let s: Obj<Shape> = Obj::<Circle>::new(Circle::new(1.0, 2.0)).upcast();

    let circle: &Circle = s.borrow().cast::<Circle>().expect("is a Circle");
    assert!((circle.r - 2.0).abs() < EPS);

    let shape: &Shape = s.borrow().cast::<Shape>().expect("is a Shape");
    assert!((shape.x - 1.0).abs() < EPS);

    assert!(s.is::<Circle>());
    assert!(s.is::<Shape>());
    assert!(!s.is::<Square>());
    assert!(s.borrow().cast::<Square>().is_none(), "unrelated class");
}

#[test]
fn failed_downcast_of_a_sibling_class() {
    let s: Obj<Shape> = Obj::<Square>::new(Square::new(1.0, 2.0)).upcast();
    assert!(s.borrow().cast::<Circle>().is_none());
    assert!(!s.is::<Circle>());
}

#[test]
fn sidecast_across_multiple_inheritance_keeps_polymorphism() {
    let s: Obj<Shape> = Obj::<Circle>::new(Circle::new(1.0, 4.0)).upcast();

    // Shape -> Drawable: a different base, at a non-zero offset, with a different vtable.
    let d: Ref<'_, Drawable> = s.borrow().cast_obj::<Drawable>().expect("is Drawable");
    assert_eq!(
        d.draw(),
        "circle r=4",
        "override runs, not Drawable's own impl"
    );
    assert!(d.visible, "secondary base field");

    // and the plain-data view of that same secondary base
    let data: &Drawable = s.borrow().cast::<Drawable>().expect("is Drawable");
    assert!(data.visible);
}

#[test]
fn owning_downcast_returns_the_handle_on_failure() {
    let s: Obj<Shape> = Obj::<Square>::new(Square::new(1.0, 2.0)).upcast();
    let s = s.downcast::<Circle>().err().expect("not a Circle");
    // the original handle survived intact
    assert_eq!(s.class().name, "Square");

    let sq: Obj<Square> = s.downcast::<Square>().ok().expect("is a Square");
    assert!((sq.s - 2.0).abs() < EPS);
}

#[test]
fn owning_downcast_preserves_the_allocation() {
    let s: Obj<Shape> = Obj::<Circle>::new(Circle::new(1.0, 2.0)).upcast();
    let c: Obj<Circle> = s.downcast::<Circle>().ok().expect("is a Circle");
    assert!((c.r - 2.0).abs() < EPS);
    // dropping `c` must free with Circle's layout, recovered from the rebuilt vtable
    drop(c);
}

#[test]
fn heterogeneous_collection_dispatches_per_element() {
    let shapes: Vec<Obj<Shape>> = vec![
        Obj::<Circle>::new(Circle::new(0.0, 1.0)).upcast(),
        Obj::<Square>::new(Square::new(0.0, 2.0)).upcast(),
    ];
    let total: f64 = shapes.iter().map(|s| s.area()).sum();
    assert!((total - (core::f64::consts::PI + 4.0)).abs() < EPS);

    let names: Vec<&str> = shapes.iter().map(|s| s.class().name).collect();
    assert_eq!(names, ["Circle", "Square"]);
}

#[test]
fn generic_over_subclasses_calls_virtuals_without_upcasting() {
    let a = Obj::<Circle>::new(Circle::new(0.0, 1.0));
    let b = Obj::<Circle>::new(Circle::new(0.0, 2.0));
    let refs = [a.borrow(), b.borrow()];
    let expected = core::f64::consts::PI * (1.0 + 4.0);
    assert!((total_area(&refs) - expected).abs() < EPS);
}

#[test]
fn mutation_through_a_polymorphic_handle() {
    let mut c = Obj::<Circle>::new(Circle::new(1.0, 2.0));
    {
        let mut m = c.borrow_mut();
        m.scale(3.0);
        m.x += 1.0; // field mutation through DerefMut on the interface
    }
    assert!((c.x - 4.0).abs() < EPS);
}

#[test]
fn class_metadata_reports_the_hierarchy() {
    let c = Obj::<Circle>::new(Circle::new(1.0, 2.0));
    let meta = c.class();
    assert_eq!(meta.name, "Circle");
    assert_eq!(meta.bases.len(), 3);
    assert!(meta.is_a(TypeId::of::<Shape>()));
    assert!(meta.is_a(TypeId::of::<Drawable>()));
    assert!(!meta.is_a(TypeId::of::<Square>()));
}

#[test]
fn static_view_dispatches_statically() {
    // A plain `&Circle` resolves `area` to the inherent method: C++ `obj.f()`, not `ptr->f()`.
    let c = Circle::new(1.0, 2.0);
    assert!((c.area() - core::f64::consts::PI * 4.0).abs() < EPS);
    // ...and a `&Shape` view of it sees Shape's own (absent) implementation only through the
    // interface, which is why `Ref<Shape>` exists.
    let as_shape: &Shape = &c;
    assert!((as_shape.x - 1.0).abs() < EPS);
}
