//! `dyn_traits(..)` only knows the traits it can actually carry through a handle.
#[obj::class(dyn_traits(Ord))]
pub struct Shape {
    pub x: i32,
}

#[obj::methods]
impl Shape {}

fn main() {}
