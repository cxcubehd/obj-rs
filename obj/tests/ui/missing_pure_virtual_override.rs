//! A concrete class must implement every pure virtual it inherits.
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

// Square is concrete but never overrides `area`.
#[obj::methods]
impl Square {}

fn main() {}
