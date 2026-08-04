//! An expression AST, written with `obj::classes!`.
//!
//! ASTs are the case `obj` is for: a dozen node types that share a base, virtual methods over all
//! of them, and passes that need to ask "is this node actually a literal?".
//!
//! ```text
//!            Expr (abstract)
//!         /     |      \      \
//!      Num    Neg    Binary (abstract)
//!                      /   \
//!                    Add   Mul
//! ```
//!
//! Run with `cargo run -p obj --example ast`.

#![allow(missing_docs)]

use obj::{Class, Obj, Ref};

obj::classes! {
    /// Any node in the tree.
    pub abstract class Expr dyn_traits(Display) {
        /// Source position, carried by every node because it is on the base.
        pub span: u32,

        /// Evaluate. Pure virtual: every node kind must say what it means.
        virtual fn eval(&self) -> i64;

        /// How tightly this node binds, for deciding when to parenthesise.
        virtual fn precedence(&self) -> u8 {
            u8::MAX
        }

        /// Non-virtual, so it is inherited by every node without being dispatched.
        fn at(&self) -> String {
            format!("@{}", self.span)
        }
    }

    /// A literal.
    pub class Num : Expr {
        pub value: i64,

        ctor new(span: u32, value: i64) : Expr { span } { value }

        override fn eval(&self) -> i64 {
            self.value
        }
    }

    /// Negation.
    pub class Neg : Expr {
        pub operand: Obj<Expr>,

        ctor new(span: u32, operand: Obj<Expr>) : Expr { span } { operand }

        override fn eval(&self) -> i64 {
            -self.operand.eval()
        }

        override fn precedence(&self) -> u8 {
            3
        }
    }

    /// Anything with a left and a right. Abstract: it does not know what the operator *is*.
    pub abstract class Binary : Expr {
        pub lhs: Obj<Expr>,
        pub rhs: Obj<Expr>,

        /// The symbol to print. Pure virtual, so `Binary` stays abstract.
        virtual fn symbol(&self) -> char;
    }

    pub class Add : Binary {
        ctor new(span: u32, lhs: Obj<Expr>, rhs: Obj<Expr>)
            : Binary { expr: Expr { span }, lhs, rhs }
            { }

        override fn eval(&self) -> i64 {
            self.lhs.eval() + self.rhs.eval()
        }

        override fn symbol(&self) -> char {
            '+'
        }

        override fn precedence(&self) -> u8 {
            1
        }
    }

    pub class Mul : Binary {
        ctor new(span: u32, lhs: Obj<Expr>, rhs: Obj<Expr>)
            : Binary { expr: Expr { span }, lhs, rhs }
            { }

        override fn eval(&self) -> i64 {
            self.lhs.eval() * self.rhs.eval()
        }

        override fn symbol(&self) -> char {
            '*'
        }

        override fn precedence(&self) -> u8 {
            2
        }
    }
}

// `dyn_traits(Display)` put `Display` on `Expr`'s interface, so every node must provide it — and
// in return, any `Obj<Expr>` can be printed whatever it actually holds.

impl std::fmt::Display for Num {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl std::fmt::Display for Neg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "-{}", self.operand)
    }
}

/// One `Display` body for both operators.
///
/// `node` is a **static view** — a plain `&Binary` — so calls through it would resolve to
/// inherited *inherent* methods, not to the override. That is the one rule, and it is why the
/// operator and its precedence are passed in: each caller supplies its own, non-virtually,
/// because it already knows which class it is. The children are different: they are `Obj<Expr>`
/// handles, so `precedence()` on them does dispatch.
fn write_binary(
    node: &Binary,
    symbol: char,
    precedence: u8,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    let parens = |side: &Obj<Expr>| -> String {
        if side.precedence() < precedence {
            format!("({side})")
        } else {
            format!("{side}")
        }
    };
    write!(f, "{} {} {}", parens(&node.lhs), symbol, parens(&node.rhs))
}

impl std::fmt::Display for Add {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_binary(self, Add::symbol(self), Add::precedence(self), f)
    }
}

impl std::fmt::Display for Mul {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_binary(self, Mul::symbol(self), Mul::precedence(self), f)
    }
}

/// Walks the tree, counting nodes by class.
///
/// The visitor is just virtual dispatch plus a downcast where the pass needs to know more than the
/// base class offers — which is how most real passes are shaped.
fn describe(node: Ref<'_, Expr>, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    println!(
        "{indent}{:<8} {:<12} eval={}",
        node.class().name,
        format!("{node}"),
        node.eval(),
    );

    // `cast` asks the object what it really is. `Binary` is abstract, and a cast to it still
    // works: it is a real subobject of both `Add` and `Mul`.
    if let Some(binary) = node.cast::<Binary>() {
        describe(binary.lhs.borrow(), depth + 1);
        describe(binary.rhs.borrow(), depth + 1);
    } else if let Some(neg) = node.cast::<Neg>() {
        describe(neg.operand.borrow(), depth + 1);
    }
}

/// Constant folding: if a node's operands are all literals, replace it with one.
///
/// Returns `None` when nothing changed, so the caller can keep the original.
fn fold(node: Ref<'_, Expr>) -> Option<Obj<Expr>> {
    // A literal is already folded.
    if node.is::<Num>() {
        return None;
    }
    let binary = node.cast::<Binary>()?;
    if !binary.lhs.is::<Num>() || !binary.rhs.is::<Num>() {
        return None;
    }
    // Both sides are literals, so `eval` cannot recurse into anything dynamic.
    Some(Obj::<Num>::new(Num::new(node.span, node.eval())).upcast())
}

fn main() {
    // (2 + 3) * -(4)
    let tree: Obj<Expr> = Obj::<Mul>::new(Mul::new(
        0,
        Obj::<Add>::new(Add::new(1, num(2, 2), num(3, 3))).upcast(),
        Obj::<Neg>::new(Neg::new(4, num(5, 4))).upcast(),
    ))
    .upcast();

    println!("== the tree ==");
    describe(tree.borrow(), 0);

    println!("\n== printing through a base handle ==");
    // `Display` came from `dyn_traits(Display)`, so this works on `Obj<Expr>` directly.
    println!("  {tree}");
    println!("  evaluates to {}", tree.eval());
    println!("  root is at {}", tree.at());

    println!("\n== constant folding ==");
    let subtree: Obj<Expr> = Obj::<Add>::new(Add::new(1, num(2, 2), num(3, 3))).upcast();
    println!("  before: {subtree}  ({})", subtree.class().name);
    match fold(subtree.borrow()) {
        Some(folded) => println!("  after:  {folded}  ({})", folded.class().name),
        None => println!("  nothing to fold"),
    }

    println!("\n== what each node is ==");
    println!(
        "  {} is an Expr? {}  a Binary? {}  a Num? {}",
        tree.class().name,
        tree.is::<Expr>(),
        tree.is::<Binary>(),
        tree.is::<Num>(),
    );
    println!(
        "  Mul's base table: {} entries ({} -> Binary -> Expr)",
        <Mul as Class>::META.bases.len(),
        <Mul as Class>::META.name,
    );
}

fn num(span: u32, value: i64) -> Obj<Expr> {
    Obj::<Num>::new(Num::new(span, value)).upcast()
}
