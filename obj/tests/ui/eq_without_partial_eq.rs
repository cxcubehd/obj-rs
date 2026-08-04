//! `Eq` only marks an existing `PartialEq` as total, so it cannot be asked for alone.
#[obj::class(dyn_traits(Eq))]
pub struct Shape {
    pub x: i32,
}

#[obj::methods]
impl Shape {}

fn main() {}
