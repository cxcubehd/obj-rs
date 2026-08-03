//! Virtual methods must stay object-safe.
#[obj::class]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn convert<T: Default>(&self) -> T {
        T::default()
    }
}

fn main() {}
