//! Every base stored inside a class has to be initialised by its constructor.
obj::classes! {
    pub abstract class Shape {
        pub x: f64,
        virtual fn area(&self) -> f64;
    }

    pub abstract class Drawable {
        pub visible: bool,
        virtual fn draw(&self) -> u8;
    }

    pub class Circle : Shape, Drawable {
        pub r: f64,

        // `Drawable` is never initialised.
        ctor new(x: f64, r: f64) : Shape { x } { r }

        override fn area(&self) -> f64 { self.r }
        override fn draw(&self) -> u8 { 0 }
    }
}

fn main() {}
