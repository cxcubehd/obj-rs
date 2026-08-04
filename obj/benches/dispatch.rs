//! Does a virtual call through `Obj<C>` cost more than a plain `dyn Trait` call?
//!
//! It should not. `obj` maps a virtual method onto a generated `dyn` trait, so the call is the
//! same indirect call the compiler already emits for `&dyn Trait` — the object carries no vtable
//! pointer of its own, and dispatch adds no hop of its own either.
//!
//! Run with `cargo bench -p obj`. No harness, no dependencies: each case is timed directly, and
//! the numbers only mean anything relative to each other.

#![allow(missing_docs)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use obj::{Class, Obj, Ref};

// ------------------------------------------------------------------ the `obj` hierarchy

#[obj::class(abstract)]
pub struct Shape {
    pub x: u64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn area(&self) -> u64;
}

#[obj::class(extends = Shape)]
pub struct Square {
    pub side: u64,
}

#[obj::methods]
impl Square {
    #[obj(override)]
    fn area(&self) -> u64 {
        self.side * self.side
    }
}

#[obj::class(extends = Shape)]
pub struct Circle {
    pub r: u64,
}

#[obj::methods]
impl Circle {
    #[obj(override)]
    fn area(&self) -> u64 {
        3 * self.r * self.r
    }
}

/// A second level, so the measured call reaches an override two steps from the handle's class.
#[obj::class(extends = Square)]
pub struct Rounded {
    pub cut: u64,
}

#[obj::methods]
impl Rounded {
    #[obj(override)]
    fn area(&self) -> u64 {
        self.side * self.side - self.cut
    }
}

// ------------------------------------------------------- the same thing as a plain trait

trait PlainShape {
    fn area(&self) -> u64;
    /// A plain trait object cannot reach fields at all, so the only way to read an inherited one
    /// is an accessor per implementor. That is what `obj`'s `Deref` is measured against.
    fn x(&self) -> u64;
}

struct PlainSquare {
    _x: u64,
    side: u64,
}
struct PlainCircle {
    _x: u64,
    r: u64,
}
struct PlainRounded {
    _x: u64,
    side: u64,
    cut: u64,
}

impl PlainShape for PlainSquare {
    fn area(&self) -> u64 {
        self.side * self.side
    }
    fn x(&self) -> u64 {
        self._x
    }
}
impl PlainShape for PlainCircle {
    fn area(&self) -> u64 {
        3 * self.r * self.r
    }
    fn x(&self) -> u64 {
        self._x
    }
}
impl PlainShape for PlainRounded {
    fn area(&self) -> u64 {
        self.side * self.side - self.cut
    }
    fn x(&self) -> u64 {
        self._x
    }
}

// ------------------------------------------------------------------ harness

const N: usize = 20_000;
const ROUNDS: usize = 200;

/// Runs `f` until the timings settle, reporting the best round — the one least disturbed by the
/// scheduler. A mean would mostly measure the machine's other work.
fn bench(name: &str, mut f: impl FnMut() -> u64) {
    // Warm up, so the first timed round is not paying for cold branch predictors and caches.
    for _ in 0..10 {
        black_box(f());
    }
    let mut best = Duration::MAX;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        black_box(f());
        best = best.min(start.elapsed());
    }
    #[allow(clippy::cast_precision_loss)]
    let per_call = best.as_nanos() as f64 / N as f64;
    println!("  {name:<34} {per_call:>6.2} ns/call");
}

fn main() {
    // Same mix of concrete types in both, so neither gets an easier branch-prediction job.
    let objs: Vec<Obj<Shape>> = (0..N)
        .map(|i| match i % 3 {
            0 => Obj::<Square>::new(Square {
                shape: Shape { x: 0 },
                side: 3,
            })
            .upcast(),
            1 => Obj::<Circle>::new(Circle {
                shape: Shape { x: 0 },
                r: 2,
            })
            .upcast(),
            _ => Obj::<Rounded>::new(Rounded {
                square: Square {
                    shape: Shape { x: 0 },
                    side: 4,
                },
                cut: 1,
            })
            .upcast(),
        })
        .collect();

    let plains: Vec<Box<dyn PlainShape>> = (0..N)
        .map(|i| -> Box<dyn PlainShape> {
            match i % 3 {
                0 => Box::new(PlainSquare { _x: 0, side: 3 }),
                1 => Box::new(PlainCircle { _x: 0, r: 2 }),
                _ => Box::new(PlainRounded {
                    _x: 0,
                    side: 4,
                    cut: 1,
                }),
            }
        })
        .collect();

    let refs: Vec<Ref<'_, Shape>> = objs.iter().map(Obj::borrow).collect();

    println!("virtual dispatch, {N} objects, best of {ROUNDS} rounds\n");

    bench("dyn Trait (baseline)", || {
        plains.iter().map(|s| s.area()).sum()
    });
    bench("obj: Obj<Shape>", || objs.iter().map(|s| s.area()).sum());
    bench("obj: Ref<Shape>", || refs.iter().map(|s| s.area()).sum());

    println!("\nfield access through a base handle\n");

    // A `dyn Trait` has no fields, so reading one costs a virtual call to an accessor that every
    // implementor must write. `obj` reads it directly, but resolves the subobject offset through
    // the class metadata first -- this is the one place it genuinely does more work.
    bench("dyn Trait accessor (baseline)", || {
        plains.iter().map(|s| s.x()).sum()
    });
    bench("obj: inherited field via Deref", || {
        objs.iter().map(|s| s.x).sum()
    });

    println!("\nper-object size\n");
    println!(
        "  {:<34} {:>3} bytes",
        "Square (obj)",
        core::mem::size_of::<Square>()
    );
    println!(
        "  {:<34} {:>3} bytes",
        "PlainSquare",
        core::mem::size_of::<PlainSquare>()
    );
    println!(
        "\n  identical: the vtable rides in the handle, exactly as it does for `&dyn Trait`.\n\
         \n  {} has {} entries in its base table.",
        <Rounded as Class>::META.name,
        <Rounded as Class>::META.bases.len(),
    );
}
