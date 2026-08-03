//! A virtual method needs a `self` receiver to dispatch on.
#[obj::class]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn make() -> f64 {
        0.0
    }
}

fn main() {}
