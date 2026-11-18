//! A small, safe expression language for custom environment variables: numbers, names, the
//! operators `+ - * / % ^`, comparisons, `and`, `or`, `not` and a few functions. A formula such as
//! `20 + 8 * sin((hour - 9) / 24 * 2 * pi)` is checked when it is typed (`parse`) and computed when
//! a variable is read (`Expr::eval`).
//!
//! It cannot loop, allocate or recurse beyond its bounds: a formula is at most [`MAX_FORMULA`]
//! characters and [`MAX_NODES`] nodes nested at most [`MAX_DEPTH`] deep, a call has at most
//! [`MAX_ARGS`] arguments, and a result that is not a finite number is an error, never a value.
//!
//! Functions (radians for the trigonometric ones): `sin cos abs sqrt floor ceil round exp ln`,
//! `min max` (any number of arguments), `clamp(x, lo, hi)`, `if(condition, then, else)`. Constants:
//! `pi` and `e`. A comparison gives 1 or 0 and any non-zero number is true.

/// The longest formula (characters).
pub const MAX_FORMULA: usize = 200;
/// The most nodes in a formula, how deep they nest and how many arguments a call takes.
pub const MAX_NODES: usize = 64;
pub const MAX_DEPTH: usize = 12;
pub const MAX_ARGS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Num(f64),
    Name(String),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// A parsed formula.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    root: Node,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Sym(&'static str),
}

const SYMS: [&str; 20] = ["<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "^", "<", ">", "(", ")", ",", "!", "=", ";"];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = cs.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while cs.get(i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                i += 1;
            }
            if matches!(cs.get(i), Some('e' | 'E')) {
                let mut k = i + 1;
                if matches!(cs.get(k), Some('+' | '-')) {
                    k += 1;
                }
                if cs.get(k).is_some_and(char::is_ascii_digit) {
                    i = k;
                    while cs.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
            }
            let text: String = cs.get(start..i).unwrap_or_default().iter().collect();
            let n = text.parse::<f64>().map_err(|_| format!("`{text}` is not a number"))?;
            out.push(Tok::Num(n));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while cs.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
                i += 1;
            }
            out.push(Tok::Name(cs.get(start..i).unwrap_or_default().iter().collect()));
        } else {
            let two: String = cs.get(i..i + 2).unwrap_or_default().iter().collect();
            let one = c.to_string();
            let found = SYMS.iter().find(|s| **s == two).or_else(|| SYMS.iter().find(|s| **s == one));
            let Some(sym) = found else { return Err(format!("`{c}` is not part of the formula language")) };
            i += sym.chars().count();
            out.push(Tok::Sym(sym));
        }
    }
    Ok(out)
}

struct P {
    toks: Vec<Tok>,
    at: usize,
    nodes: usize,
    depth: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn sym(&mut self, s: &str) -> bool {
        let yes = matches!(self.peek(), Some(Tok::Sym(x)) if *x == s);
        if yes {
            self.at += 1;
        }
        yes
    }

    fn word(&mut self, w: &str) -> bool {
        let yes = matches!(self.peek(), Some(Tok::Name(x)) if x == w);
        if yes {
            self.at += 1;
        }
        yes
    }

    fn node(&mut self, n: Node) -> Result<Node, String> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("the formula has more than {MAX_NODES} parts"));
        }
        Ok(n)
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("the formula nests deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }

    fn or(&mut self) -> Result<Node, String> {
        self.enter()?;
        let mut left = self.and()?;
        while self.word("or") || self.sym("||") {
            let right = self.and()?;
            left = self.node(Node::Bin(Op::Or, Box::new(left), Box::new(right)))?;
        }
        self.depth -= 1;
        Ok(left)
    }

    fn and(&mut self) -> Result<Node, String> {
        let mut left = self.cmp()?;
        while self.word("and") || self.sym("&&") {
            let right = self.cmp()?;
            left = self.node(Node::Bin(Op::And, Box::new(left), Box::new(right)))?;
        }
        Ok(left)
    }

    fn cmp(&mut self) -> Result<Node, String> {
        let left = self.add()?;
        let op = match self.peek() {
            Some(Tok::Sym("<")) => Op::Lt,
            Some(Tok::Sym(">")) => Op::Gt,
            Some(Tok::Sym("<=")) => Op::Le,
            Some(Tok::Sym(">=")) => Op::Ge,
            Some(Tok::Sym("==" | "=")) => Op::Eq,
            Some(Tok::Sym("!=")) => Op::Ne,
            _ => return Ok(left),
        };
        self.at += 1;
        let right = self.add()?;
        self.node(Node::Bin(op, Box::new(left), Box::new(right)))
    }

    fn add(&mut self) -> Result<Node, String> {
        let mut left = self.mul()?;
        loop {
            let op = if self.sym("+") {
                Op::Add
            } else if self.sym("-") {
                Op::Sub
            } else {
                return Ok(left);
            };
            let right = self.mul()?;
            left = self.node(Node::Bin(op, Box::new(left), Box::new(right)))?;
        }
    }

    fn mul(&mut self) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let op = if self.sym("*") {
                Op::Mul
            } else if self.sym("/") {
                Op::Div
            } else if self.sym("%") {
                Op::Mod
            } else {
                return Ok(left);
            };
            let right = self.unary()?;
            left = self.node(Node::Bin(op, Box::new(left), Box::new(right)))?;
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.sym("-") {
            self.enter()?;
            let x = self.unary()?;
            self.depth -= 1;
            return self.node(Node::Neg(Box::new(x)));
        }
        if self.word("not") || self.sym("!") {
            self.enter()?;
            let x = self.unary()?;
            self.depth -= 1;
            return self.node(Node::Not(Box::new(x)));
        }
        let base = self.atom()?;
        if self.sym("^") {
            self.enter()?;
            let e = self.unary()?;
            self.depth -= 1;
            return self.node(Node::Bin(Op::Pow, Box::new(base), Box::new(e)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.at += 1;
                self.node(Node::Num(n))
            }
            Some(Tok::Sym("(")) => {
                self.at += 1;
                let e = self.or()?;
                if !self.sym(")") {
                    return Err("a `(` is never closed".into());
                }
                Ok(e)
            }
            Some(Tok::Name(name)) => {
                self.at += 1;
                if self.sym("(") {
                    let mut args = Vec::new();
                    while !matches!(self.peek(), Some(Tok::Sym(")"))) {
                        args.push(self.or()?);
                        if args.len() > MAX_ARGS {
                            return Err(format!("`{name}` takes at most {MAX_ARGS} arguments"));
                        }
                        if !self.sym(",") {
                            break;
                        }
                    }
                    if !self.sym(")") {
                        return Err(format!("the call of `{name}` is never closed"));
                    }
                    return self.node(Node::Call(name, args));
                }
                self.node(Node::Name(name))
            }
            Some(Tok::Sym(s)) => Err(format!("unexpected `{s}`")),
            None => Err("the formula ends too soon".into()),
        }
    }
}

/// Parses a formula; the error says what is wrong.
pub fn parse(src: &str) -> Result<Expr, String> {
    if src.chars().count() > MAX_FORMULA {
        return Err(format!("a formula is at most {MAX_FORMULA} characters"));
    }
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("the formula is empty".into());
    }
    let mut p = P { toks, at: 0, nodes: 0, depth: 0 };
    let root = p.or()?;
    if let Some(t) = p.peek() {
        return Err(match t {
            Tok::Sym(s) => format!("unexpected `{s}`"),
            Tok::Name(n) => format!("unexpected `{n}`"),
            Tok::Num(_) => "unexpected number".into(),
        });
    }
    check_calls(&root)?;
    Ok(Expr { root })
}

/// The functions and how many arguments each takes (`None`: any number from one).
fn arity(name: &str) -> Option<Option<usize>> {
    Some(match name {
        "sin" | "cos" | "abs" | "sqrt" | "floor" | "ceil" | "round" | "exp" | "ln" => Some(1),
        "min" | "max" => None,
        "clamp" | "if" => Some(3),
        _ => return Option::None,
    })
}

fn check_calls(n: &Node) -> Result<(), String> {
    match n {
        Node::Num(_) | Node::Name(_) => Ok(()),
        Node::Neg(x) | Node::Not(x) => check_calls(x),
        Node::Bin(_, a, b) => check_calls(a).and_then(|()| check_calls(b)),
        Node::Call(name, args) => {
            match arity(name) {
                None => return Err(format!("`{name}` is not a function (sin cos abs sqrt floor ceil round exp ln min max clamp if)")),
                Some(Some(k)) if args.len() != k => return Err(format!("`{name}` takes {k} argument(s), not {}", args.len())),
                Some(None) if args.is_empty() => return Err(format!("`{name}` needs at least one argument")),
                Some(_) => {}
            }
            args.iter().try_for_each(check_calls)
        }
    }
}

fn names_of(n: &Node, out: &mut Vec<String>) {
    match n {
        Node::Num(_) => {}
        Node::Name(s) => {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        Node::Neg(x) | Node::Not(x) => names_of(x, out),
        Node::Bin(_, a, b) => {
            names_of(a, out);
            names_of(b, out);
        }
        Node::Call(_, args) => args.iter().for_each(|a| names_of(a, out)),
    }
}

fn bool_num(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn eval(n: &Node, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    let v = match n {
        Node::Num(x) => *x,
        Node::Name(s) => match s.as_str() {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            _ => vars(s).ok_or_else(|| format!("`{s}` has no value"))?,
        },
        Node::Neg(x) => -eval(x, vars)?,
        Node::Not(x) => bool_num(eval(x, vars)? == 0.0),
        Node::Bin(op, a, b) => {
            let (x, y) = (eval(a, vars)?, eval(b, vars)?);
            match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => {
                    if y == 0.0 {
                        return Err("division by zero".into());
                    }
                    x / y
                }
                Op::Mod => {
                    if y == 0.0 {
                        return Err("division by zero".into());
                    }
                    x % y
                }
                Op::Pow => x.powf(y),
                Op::Lt => bool_num(x < y),
                Op::Gt => bool_num(x > y),
                Op::Le => bool_num(x <= y),
                Op::Ge => bool_num(x >= y),
                Op::Eq => bool_num(x == y),
                Op::Ne => bool_num(x != y),
                Op::And => bool_num(x != 0.0 && y != 0.0),
                Op::Or => bool_num(x != 0.0 || y != 0.0),
            }
        }
        Node::Call(name, args) => {
            let mut v = Vec::with_capacity(args.len());
            // `if` computes only the branch it takes.
            if name == "if" {
                let (c, a, b) = (args.first(), args.get(1), args.get(2));
                let (Some(c), Some(a), Some(b)) = (c, a, b) else { return Err("`if` takes three arguments".into()) };
                return eval(if eval(c, vars)? != 0.0 { a } else { b }, vars);
            }
            for a in args {
                v.push(eval(a, vars)?);
            }
            let first = v.first().copied().unwrap_or(0.0);
            match name.as_str() {
                "sin" => first.sin(),
                "cos" => first.cos(),
                "abs" => first.abs(),
                "sqrt" => {
                    if first < 0.0 {
                        return Err("the square root of a negative number".into());
                    }
                    first.sqrt()
                }
                "floor" => first.floor(),
                "ceil" => first.ceil(),
                "round" => first.round(),
                "exp" => first.exp(),
                "ln" => {
                    if first <= 0.0 {
                        return Err("the logarithm of a number that is not positive".into());
                    }
                    first.ln()
                }
                "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
                "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                "clamp" => {
                    let (lo, hi) = (v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0));
                    first.max(lo.min(hi)).min(hi.max(lo))
                }
                other => return Err(format!("`{other}` is not a function")),
            }
        }
    };
    if v.is_finite() { Ok(v) } else { Err("the result is not a finite number".into()) }
}

impl Expr {
    /// The value of the formula; `vars` gives the value of each name (`pi` and `e` are built in).
    pub fn eval(&self, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
        eval(&self.root, vars)
    }

    /// The names the formula reads, in order of appearance, without `pi` and `e`.
    pub fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        names_of(&self.root, &mut out);
        out.retain(|n| n != "pi" && n != "e");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(src: &str) -> Result<f64, String> {
        parse(src)?.eval(&|n| match n {
            "x" => Some(4.0),
            "hour" => Some(15.0),
            _ => None,
        })
    }

    #[test]
    fn arithmetic_comparison_and_functions() {
        assert_eq!(calc("1 + 2 * 3"), Ok(7.0));
        assert_eq!(calc("(1 + 2) * 3"), Ok(9.0));
        assert_eq!(calc("2 ^ 3 ^ 2"), Ok(512.0));
        assert_eq!(calc("-x ^ 2"), Ok(-16.0), "a power binds tighter than a sign");
        assert_eq!(calc("10 % 4 + 7 / 2"), Ok(5.5));
        assert_eq!(calc("x > 3 and x < 5"), Ok(1.0));
        assert_eq!(calc("not (x == 4) or x != 4"), Ok(0.0));
        assert_eq!(calc("if(x > 3, 100, 1 / 0)"), Ok(100.0), "only the taken branch is computed");
        assert_eq!(calc("min(3, x, 9) + max(1, 2) + clamp(50, 0, 10)"), Ok(15.0));
        assert_eq!(calc("round(sqrt(16)) + floor(2.9) + ceil(2.1) + abs(-1)"), Ok(10.0));
        assert!((calc("20 + 8 * sin((hour - 9) / 24 * 2 * pi)").unwrap() - 28.0).abs() < 1e-9);
        assert!((calc("exp(ln(5))").unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(parse("a + b * a").unwrap().names(), ["a", "b"]);
        assert_eq!(parse("pi * e").unwrap().names(), Vec::<String>::new());
    }

    #[test]
    fn mistakes_are_errors_not_values() {
        for (src, why) in [
            ("", "empty"),
            ("1 +", "ends too soon"),
            ("(1", "never closed"),
            ("1 2", "unexpected"),
            ("sin(1, 2)", "takes 1"),
            ("nope(1)", "not a function"),
            ("min()", "at least one"),
            ("1 / 0", "division by zero"),
            ("5 % 0", "division by zero"),
            ("sqrt(-1)", "negative"),
            ("ln(0)", "logarithm"),
            ("y + 1", "no value"),
            ("2 ^ 5000", "finite"),
            ("1e999", "finite"),
            ("a $ b", "not part"),
            ("#", "not part"),
        ] {
            let e = calc(src).unwrap_err();
            assert!(e.contains(why), "{src}: {e}");
        }
    }

    #[test]
    fn hostile_formulas_are_bounded() {
        assert!(parse(&"1+".repeat(300)).unwrap_err().contains("at most"));
        assert!(parse(&format!("{}1{}", "(".repeat(100), ")".repeat(100))).unwrap_err().contains("deeper"));
        assert!(parse(&format!("{}1", "-".repeat(150))).unwrap_err().contains("deeper"));
        assert!(parse(&format!("{}1", "not ".repeat(40))).is_err());
        assert!(parse(&"1+1".repeat(40)).unwrap_err().contains("parts"));
        assert!(parse(&format!("max({})", "1,".repeat(20))).is_err());
        assert!(parse("1.2.3").is_err());
        for cut in 0.."20 + 8 * sin((hour - 9) / 24 * 2 * pi) >= x".len() {
            let _ = parse(&"20 + 8 * sin((hour - 9) / 24 * 2 * pi) >= x".chars().take(cut).collect::<String>());
        }
        for junk in ["\0", "é", "((((", "))))", ",,,", "if(", "if(1,2", ".", "e", "pi pi", "1e", "1e+", "--1", "!!1"] {
            let _ = parse(junk).map(|e| e.eval(&|_| Some(1.0)));
        }
    }
}
