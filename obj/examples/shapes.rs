//! A worked example of every feature `obj` currently provides.
//!
//! Run with `cargo run -p obj --example shapes`.
//!
//! ```text
//!   Shape (abstract)   Drawable (abstract)
//!         \                  /
//!          `---- Circle ----'          Square
//!                  |                     |
//!             DashedCircle          (extends Shape)
//! ```

use obj::{ArcShared, Class, Obj, Ref, Shared};

// ---------------------------------------------------------------- abstract bases

/// A shape with a position. Abstract: `area` has no implementation here.
#[obj::class(abstract)]
pub struct Shape {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
}

#[obj::methods]
impl Shape {
    /// Pure virtual — every concrete shape must provide it.
    #[obj(virtual)]
    fn area(&self) -> f64;

    /// Virtual with a default body, which subclasses may inherit or override.
    #[obj(virtual)]
    fn scale(&mut self, k: f64) {
        self.x *= k;
        self.y *= k;
    }

    /// Non-virtual: inherited through `Deref`, never dispatched dynamically.
    fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }
}

/// A second, unrelated base, to demonstrate multiple inheritance.
#[obj::class(abstract)]
pub struct Drawable {
    /// Whether the object should be rendered.
    pub visible: bool,
}

#[obj::methods]
impl Drawable {
    /// Renders the object, as text here.
    #[obj(virtual)]
    fn draw(&self) -> String;

    /// Marks the object hidden. Virtual, with a body subclasses may inherit.
    #[obj(virtual)]
    fn hide(&mut self) {
        self.visible = false;
    }
}

// ---------------------------------------------------------------- concrete classes

/// Inherits data and behaviour from *both* bases. `Shape` is primary, so it sits at offset 0
/// and is the `Deref` target; `Drawable` gets an `as_drawable()` accessor.
#[obj::class(extends(Shape, Drawable))]
pub struct Circle {
    /// Radius.
    pub r: f64,
}

#[obj::methods]
impl Circle {
    /// Overrides `Shape::area`.
    #[obj(override)]
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }

    /// Overrides `Drawable::draw` — a virtual from the *second* base.
    #[obj(override)]
    fn draw(&self) -> String {
        format!("circle(r={})", self.r)
    }

    /// Overrides `Shape::scale`, then calls the base implementation.
    #[obj(override)]
    fn scale(&mut self, k: f64) {
        self.r *= k;
        // A `super` call is just a qualified call to the base's own implementation.
        Shape::scale(self, k);
    }
}

/// Overrides only `draw`; everything else is inherited, including `Circle`'s `area`.
#[obj::class(extends = Circle)]
pub struct DashedCircle {
    /// Number of dashes around the outline.
    pub dashes: u32,
}

#[obj::methods]
impl DashedCircle {
    /// Overrides `Drawable::draw` again, two levels down.
    #[obj(override)]
    fn draw(&self) -> String {
        format!("dashed({}, r={})", self.dashes, self.r)
    }
}

/// A plain single-inheritance subclass, not `Drawable`.
#[obj::class(extends = Shape)]
pub struct Square {
    /// Side length.
    pub side: f64,
}

#[obj::methods]
impl Square {
    /// Overrides `Shape::area`.
    #[obj(override)]
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

// ---------------------------------------------------------------- constructors
//
// Constructors are ordinary Rust. Base subobjects are named after their class.

impl Circle {
    fn new(x: f64, y: f64, r: f64) -> Circle {
        Circle {
            shape: Shape { x, y },
            drawable: Drawable { visible: true },
            r,
        }
    }
}

impl DashedCircle {
    fn new(x: f64, y: f64, r: f64, dashes: u32) -> DashedCircle {
        DashedCircle {
            circle: Circle::new(x, y, r),
            dashes,
        }
    }
}

impl Square {
    fn new(x: f64, y: f64, side: f64) -> Square {
        Square {
            shape: Shape { x, y },
            side,
        }
    }
}

/// Generic over anything that derives from `Shape`, with no upcast needed.
fn describe(shape: Ref<'_, Shape>) -> String {
    format!("{} area={:.2}", shape.class().name, shape.area())
}

fn main() {
    // A heterogeneous collection, held by a base-class handle.
    let shapes: Vec<Obj<Shape>> = vec![
        Obj::<Circle>::new(Circle::new(0.0, 0.0, 1.0)).upcast(),
        Obj::<Square>::new(Square::new(1.0, 1.0, 2.0)).upcast(),
        Obj::<DashedCircle>::new(DashedCircle::new(2.0, 2.0, 3.0, 8)).upcast(),
    ];

    println!("== virtual dispatch through a base handle ==");
    for s in &shapes {
        println!("  {}", describe(s.borrow()));
    }

    println!("\n== dynamic_cast ==");
    for s in &shapes {
        // Downcast to a concrete class.
        if let Some(c) = s.borrow().cast::<Circle>() {
            println!("  {} is a Circle with r={}", s.class().name, c.r);
        }
        // Sidecast to an unrelated base: `Shape` and `Drawable` are siblings, so this rebuilds
        // the fat pointer from the class's base table.
        match s.borrow().cast_obj::<Drawable>() {
            Some(d) => println!("    ...and draws as {}", d.draw()),
            None => println!("  {} is not Drawable", s.class().name),
        }
    }

    println!("\n== inherited fields and non-virtual methods ==");
    let dashed = Obj::<DashedCircle>::new(DashedCircle::new(5.0, 6.0, 2.0, 4));
    println!("  own field      dashes = {}", dashed.dashes);
    println!("  parent field   r      = {}", dashed.r);
    println!("  grandparent    x      = {}", dashed.x);
    println!("  non-virtual    pos    = {:?}", dashed.position());
    println!("  secondary base visible= {}", dashed.as_drawable().visible);

    println!("\n== override resolution ==");
    let mut c = Obj::<Circle>::new(Circle::new(2.0, 2.0, 3.0));
    c.scale(2.0); // Circle::scale, which super-calls Shape::scale
    println!("  after scale(2): r={} pos={:?}", c.r, c.position());

    let mut d = Obj::<DashedCircle>::new(DashedCircle::new(1.0, 1.0, 1.0, 3));
    d.scale(3.0); // DashedCircle does not override scale; Circle's runs
    println!("  inherited override: r={} pos={:?}", d.r, d.position());

    println!("\n== shared ownership ==");
    let shared = Shared::<Circle>::new(Circle::new(0.0, 0.0, 4.0));
    let alias = shared.clone();
    println!("  strong_count = {}", shared.strong_count());
    println!("  area via alias = {:.2}", alias.area());

    let arc: ArcShared<Shape> = ArcShared::<Square>::new(Square::new(0.0, 0.0, 5.0)).upcast();
    let worker = {
        let arc = arc.clone();
        std::thread::spawn(move || arc.area())
    };
    println!(
        "  computed on another thread = {:.2}",
        worker.join().expect("worker panicked")
    );

    println!("\n== class metadata ==");
    let meta = <DashedCircle as Class>::META;
    println!(
        "  {} has {} entries in its base table",
        meta.name,
        meta.bases.len()
    );
    println!(
        "  is a Shape?    {}",
        meta.is_a(std::any::TypeId::of::<Shape>())
    );
    println!(
        "  is a Drawable? {}",
        meta.is_a(std::any::TypeId::of::<Drawable>())
    );
    println!(
        "  is a Square?   {}",
        meta.is_a(std::any::TypeId::of::<Square>())
    );
}
