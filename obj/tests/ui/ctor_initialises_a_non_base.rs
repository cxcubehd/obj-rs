//! A constructor may only initialise bases the class actually has.
obj::classes! {
    pub abstract class Shape {
        pub x: f64,
        virtual fn area(&self) -> f64;
    }

    pub class Square : Shape {
        pub side: f64,

        // `Elephant` is not a base of `Square`.
        ctor new(x: f64, side: f64) : Shape { x }, Elephant { trunk: 1 } { side }

        override fn area(&self) -> f64 { self.side }
    }
}

fn main() {}
