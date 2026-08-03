//! Reference-counted ownership: `Shared` (single-threaded) and `ArcShared` (across threads).

#![allow(missing_docs)]

use obj::{ArcShared, Obj, Shared};

#[obj::class(abstract)]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn area(&self) -> f64;
}

#[obj::class(extends = Shape)]
pub struct Square {
    pub side: f64,
}

#[obj::methods]
impl Square {
    #[obj(override)]
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

impl Square {
    fn make(side: f64) -> Square {
        Square {
            shape: Shape { x: 0.0 },
            side,
        }
    }
}

const EPS: f64 = 1e-9;

#[test]
fn shared_clones_point_at_one_object() {
    let a = Shared::<Square>::new(Square::make(3.0));
    let b = a.clone();
    assert_eq!(a.strong_count(), 2);
    assert!((b.area() - 9.0).abs() < EPS);
    drop(b);
    assert_eq!(a.strong_count(), 1);
}

#[test]
fn shared_upcast_keeps_virtual_dispatch_and_identity() {
    let sq = Shared::<Square>::new(Square::make(4.0));
    let also = sq.clone();
    let shape: Shared<Shape> = sq.upcast();
    assert!((shape.area() - 16.0).abs() < EPS, "Square::area still runs");
    assert_eq!(shape.class().name, "Square");
    assert_eq!(also.strong_count(), 2, "the upcast shares the refcount");
}

#[test]
fn shared_downcast_round_trips() {
    let shape: Shared<Shape> = Shared::<Square>::new(Square::make(5.0)).upcast();
    let sq: Shared<Square> = shape.downcast::<Square>().ok().expect("is a Square");
    assert!((sq.side - 5.0).abs() < EPS);
}

#[test]
fn weak_handles_do_not_keep_the_object_alive() {
    let sq = Shared::<Square>::new(Square::make(2.0));
    let weak = sq.downgrade();
    assert!(weak.upgrade().is_some());
    drop(sq);
    assert!(weak.upgrade().is_none(), "object was freed");
}

#[test]
fn arc_shared_crosses_threads() {
    let sq = ArcShared::<Square>::new(Square::make(6.0));
    let shape: ArcShared<Shape> = sq.upcast();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let s = shape.clone();
            std::thread::spawn(move || s.area())
        })
        .collect();

    for h in handles {
        assert!((h.join().expect("thread panicked") - 36.0).abs() < EPS);
    }
    assert_eq!(shape.class().name, "Square");
}

#[test]
fn arc_shared_downcast_and_weak() {
    let shape: ArcShared<Shape> = ArcShared::<Square>::new(Square::make(7.0)).upcast();
    let weak = shape.downgrade();

    let sq: ArcShared<Square> = shape.downcast::<Square>().ok().expect("is a Square");
    assert!((sq.side - 7.0).abs() < EPS);
    assert!(weak.upgrade().is_some(), "same allocation, still alive");

    drop(sq);
    assert!(weak.upgrade().is_none());
}

#[test]
fn borrowed_view_from_every_handle_kind() {
    let owned = Obj::<Square>::new(Square::make(3.0));
    let shared = Shared::<Square>::new(Square::make(3.0));
    let arc = ArcShared::<Square>::new(Square::make(3.0));

    for r in [owned.borrow(), shared.borrow(), arc.borrow()] {
        assert!((r.area() - 9.0).abs() < EPS);
        assert!(r.is::<Shape>());
        assert!((r.cast::<Square>().expect("is a Square").side - 3.0).abs() < EPS);
    }
}
