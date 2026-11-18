//! The two front ends of the board languages: one lexer and one parser per language, producing
//! the syntax tree `lang` compiles. Only the documented subset is accepted; anything else is an
//! error that names its line. Nesting is bounded, and no loop here can run without consuming a
//! token.

use crate::lang::{Ast, Bop, Expr, FuncDef, Kind, MAX_FUNCS, Stmt, Target, Uop, V};

type R<T> = Result<T, String>;

/// Deepest nesting of expressions and of unary operators.
const MAX_DEPTH: usize = 30;
/// Most parts in one f-string or template.
const MAX_PARTS: usize = 24;
/// Most operators and postfix steps in one program's expressions (per statement they restart).
const MAX_CHAIN: usize = 400;
/// Deepest nesting of statements.
const MAX_NEST: usize = 40;

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Lit(String),
    Code(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Fmt(Vec<Part>),
    Name(String),
    Op(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

static EOF: Tok = Tok::Eof;

const OPS3_PY: [&str; 2] = ["**=", "//="];
const OPS3_JS: [&str; 3] = ["===", "!==", "**="];
const OPS2_PY: [&str; 12] = ["==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "**", "//", "->"];
const OPS2_JS: [&str; 16] = ["==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "**", "&&", "||", "=>", "++", "--", "??"];
const OPS1: [&str; 20] = ["+", "-", "*", "/", "%", "<", ">", "=", "(", ")", "[", "]", "{", "}", ",", ":", ";", ".", "?", "!"];

fn describe(t: &Tok) -> String {
    match t {
        Tok::Int(_) | Tok::Float(_) => "a number".into(),
        Tok::Str(_) | Tok::Fmt(_) => "a text".into(),
        Tok::Name(n) => format!("`{n}`"),
        Tok::Op(o) => format!("`{o}`"),
        Tok::Newline => "the end of the line".into(),
        Tok::Indent => "an indent".into(),
        Tok::Dedent => "the end of the block".into(),
        Tok::Eof => "the end of the program".into(),
    }
}

fn is_digit(c: Option<&char>) -> bool {
    c.is_some_and(char::is_ascii_digit)
}

/// Reads a string starting at its opening quote; returns what it holds and where it ends.
fn read_string(cs: &[char], start: usize, line: &mut usize, py: bool, fmt: bool) -> R<(Tok, usize)> {
    let quote = cs.get(start).copied().unwrap_or('"');
    let triple = py && cs.get(start + 1) == Some(&quote) && cs.get(start + 2) == Some(&quote);
    let multiline = triple || quote == '`';
    let mut i = start + if triple { 3 } else { 1 };
    let mut lit = String::new();
    let mut parts: Vec<Part> = Vec::new();
    let first = *line;
    loop {
        let Some(&c) = cs.get(i) else { return Err(format!("line {first}: the text is never closed")) };
        if triple {
            if c == quote && cs.get(i + 1) == Some(&quote) && cs.get(i + 2) == Some(&quote) {
                i += 3;
                break;
            }
        } else if c == quote {
            i += 1;
            break;
        }
        if c == '\n' {
            if !multiline {
                return Err(format!("line {first}: the text is never closed"));
            }
            *line += 1;
        }
        if c == '\\' {
            let Some(&n) = cs.get(i + 1) else { return Err(format!("line {first}: the text is never closed")) };
            match n {
                'n' => lit.push('\n'),
                't' => lit.push('\t'),
                'r' => lit.push('\r'),
                '0' => lit.push('\0'),
                '\n' => *line += 1,
                other => lit.push(other),
            }
            i += 2;
            continue;
        }
        let brace = if py { c == '{' } else { c == '$' && cs.get(i + 1) == Some(&'{') };
        if fmt && brace {
            if py && cs.get(i + 1) == Some(&'{') {
                lit.push('{');
                i += 2;
                continue;
            }
            let mut j = if py { i + 1 } else { i + 2 };
            let from = j;
            let mut depth = 1usize;
            while let Some(&d) = cs.get(j) {
                match d {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if depth != 0 {
                return Err(format!("line {first}: a `{{` in the text is never closed"));
            }
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            parts.push(Part::Code(cs.get(from..j).unwrap_or_default().iter().collect()));
            if parts.len() > MAX_PARTS {
                return Err(format!("line {first}: more than {MAX_PARTS} parts in one text"));
            }
            i = j + 1;
            continue;
        }
        if fmt && py && c == '}' {
            lit.push('}');
            i += if cs.get(i + 1) == Some(&'}') { 2 } else { 1 };
            continue;
        }
        lit.push(c);
        i += 1;
    }
    if fmt {
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok((Tok::Fmt(parts), i))
    } else {
        Ok((Tok::Str(lit), i))
    }
}

fn lex(src: &str, py: bool, first_line: usize) -> R<Vec<(Tok, usize)>> {
    let cs: Vec<char> = src.chars().collect();
    let mut out: Vec<(Tok, usize)> = Vec::new();
    let (mut i, mut line, mut depth) = (0usize, first_line, 0usize);
    let mut indents: Vec<usize> = vec![0];
    let mut bol = py;
    while i < cs.len() {
        if bol && depth == 0 {
            let (mut col, mut j) = (0usize, i);
            while let Some(&c) = cs.get(j) {
                match c {
                    ' ' => col += 1,
                    '\t' => col += 8,
                    '\r' => {}
                    _ => break,
                }
                j += 1;
            }
            match cs.get(j) {
                None => break,
                Some('\n') => {
                    i = j + 1;
                    line += 1;
                    continue;
                }
                Some('#') => {
                    while cs.get(j).is_some_and(|c| *c != '\n') {
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                _ => {}
            }
            i = j;
            bol = false;
            let top = indents.last().copied().unwrap_or(0);
            if col > top {
                indents.push(col);
                out.push((Tok::Indent, line));
            } else if col < top {
                while indents.last().copied().unwrap_or(0) > col {
                    indents.pop();
                    out.push((Tok::Dedent, line));
                }
                if indents.last().copied().unwrap_or(0) != col {
                    return Err(format!("line {line}: the indentation does not match any outer block"));
                }
            }
            continue;
        }
        let Some(&c) = cs.get(i) else { break };
        let next = cs.get(i + 1).copied();
        if c == '\n' {
            line += 1;
            i += 1;
            if py && depth == 0 {
                if !matches!(out.last(), None | Some((Tok::Newline, _))) {
                    out.push((Tok::Newline, line - 1));
                }
                bol = true;
            }
            continue;
        }
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            continue;
        }
        if (py && c == '#') || (!py && c == '/' && next == Some('/')) {
            while cs.get(i).is_some_and(|c| *c != '\n') {
                i += 1;
            }
            continue;
        }
        if !py && c == '/' && next == Some('*') {
            let first = line;
            i += 2;
            loop {
                match cs.get(i) {
                    None => return Err(format!("line {first}: the comment is never closed")),
                    Some('*') if cs.get(i + 1) == Some(&'/') => {
                        i += 2;
                        break;
                    }
                    Some('\n') => {
                        line += 1;
                        i += 1;
                    }
                    Some(_) => i += 1,
                }
            }
            continue;
        }
        if py && c == '\\' && next == Some('\n') {
            i += 2;
            line += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && is_digit(cs.get(i + 1))) {
            let start = i;
            if c == '0' && matches!(next, Some('x' | 'X')) {
                i += 2;
                while cs.get(i).is_some_and(char::is_ascii_hexdigit) {
                    i += 1;
                }
                let text: String = cs.get(start + 2..i).unwrap_or_default().iter().collect();
                let n = i64::from_str_radix(&text, 16)
                    .map_err(|_| format!("line {line}: a hexadecimal number is 0x followed by digits (and fits 63 bits)"))?;
                out.push((Tok::Int(n), line));
                continue;
            }
            let mut is_float = false;
            while is_digit(cs.get(i)) {
                i += 1;
            }
            if cs.get(i) == Some(&'.') && is_digit(cs.get(i + 1)) {
                is_float = true;
                i += 1;
                while is_digit(cs.get(i)) {
                    i += 1;
                }
            }
            if matches!(cs.get(i), Some('e' | 'E')) {
                let mut k = i + 1;
                if matches!(cs.get(k), Some('+' | '-')) {
                    k += 1;
                }
                if is_digit(cs.get(k)) {
                    is_float = true;
                    i = k;
                    while is_digit(cs.get(i)) {
                        i += 1;
                    }
                }
            }
            let text: String = cs.get(start..i).unwrap_or_default().iter().collect();
            if is_float {
                let f = text.parse::<f64>().map_err(|_| format!("line {line}: `{text}` is not a number"))?;
                out.push((Tok::Float(f), line));
            } else {
                let n = text.parse::<i64>().map_err(|_| format!("line {line}: the number {text} is too large"))?;
                out.push((Tok::Int(n), line));
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' || (!py && c == '$') {
            let start = i;
            while cs.get(i).is_some_and(|c| c.is_alphanumeric() || *c == '_' || (!py && *c == '$')) {
                i += 1;
            }
            let name: String = cs.get(start..i).unwrap_or_default().iter().collect();
            if py && (name == "f" || name == "F") && matches!(cs.get(i), Some('"' | '\'')) {
                let at = line;
                let (t, end) = read_string(&cs, i, &mut line, py, true)?;
                out.push((t, at));
                i = end;
                continue;
            }
            out.push((Tok::Name(name), line));
            continue;
        }
        if c == '"' || c == '\'' || (!py && c == '`') {
            let at = line;
            let (t, end) = read_string(&cs, i, &mut line, py, c == '`')?;
            out.push((t, at));
            i = end;
            continue;
        }
        let mut found: Option<(&'static str, usize)> = None;
        let (ops3, ops2): (&[&'static str], &[&'static str]) = if py { (&OPS3_PY[..], &OPS2_PY[..]) } else { (&OPS3_JS[..], &OPS2_JS[..]) };
        for (n, list) in [(3usize, ops3), (2, ops2), (1, &OPS1[..])] {
            let s: String = cs.get(i..i + n).unwrap_or_default().iter().collect();
            if let Some(o) = list.iter().find(|o| **o == s) {
                found = Some((*o, n));
                break;
            }
        }
        let Some((op, n)) = found else { return Err(format!("line {line}: `{c}` is not part of the language subset")) };
        match op {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        out.push((Tok::Op(op), line));
        i += n;
    }
    if py {
        if !matches!(out.last(), None | Some((Tok::Newline, _))) {
            out.push((Tok::Newline, line));
        }
        while indents.len() > 1 {
            indents.pop();
            out.push((Tok::Dedent, line));
        }
    }
    out.push((Tok::Eof, line));
    Ok(out)
}

struct P {
    toks: Vec<(Tok, usize)>,
    at: usize,
    py: bool,
    funcs: Vec<FuncDef>,
    depth: usize,
    /// Statements nested inside each other.
    nest: usize,
    /// Operators and postfix steps read so far (a chain of them is a deep tree).
    chain: usize,
}

fn lit_int(n: i64) -> Expr {
    Expr::Lit(V::Int(n))
}

fn hoist(v: Vec<Stmt>) -> Vec<Stmt> {
    let (defs, rest): (Vec<Stmt>, Vec<Stmt>) = v.into_iter().partition(|s| matches!(s.kind, Kind::Def(..)));
    defs.into_iter().chain(rest).collect()
}

fn to_target(e: Expr, line: usize) -> R<Target> {
    match e {
        Expr::Name(n) => Ok(Target::Name(n)),
        Expr::Index(o, i) => Ok(Target::Index(*o, *i)),
        _ => Err(format!("line {line}: only a variable or a list item can be assigned to")),
    }
}

impl P {
    fn peek(&self) -> &Tok {
        self.toks.get(self.at).map_or(&EOF, |t| &t.0)
    }

    fn peek_at(&self, n: usize) -> &Tok {
        self.toks.get(self.at + n).map_or(&EOF, |t| &t.0)
    }

    fn line(&self) -> usize {
        self.toks.get(self.at).or(self.toks.last()).map_or(1, |t| t.1)
    }

    fn fail<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(format!("line {}: {}", self.line(), msg.into()))
    }

    fn unexpected<T>(&self) -> R<T> {
        self.fail(format!("unexpected {}", describe(self.peek())))
    }

    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.at < self.toks.len() {
            self.at += 1;
        }
        t
    }

    fn is_op(&self, o: &str) -> bool {
        matches!(self.peek(), Tok::Op(x) if *x == o)
    }

    fn eat_op(&mut self, o: &str) -> bool {
        let yes = self.is_op(o);
        if yes {
            self.at += 1;
        }
        yes
    }

    fn expect_op(&mut self, o: &str) -> R<()> {
        if self.eat_op(o) { Ok(()) } else { self.fail(format!("expected `{o}`, found {}", describe(self.peek()))) }
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == k)
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        let yes = self.is_kw(k);
        if yes {
            self.at += 1;
        }
        yes
    }

    fn expect_kw(&mut self, k: &str) -> R<()> {
        if self.eat_kw(k) { Ok(()) } else { self.fail(format!("expected `{k}`, found {}", describe(self.peek()))) }
    }

    fn keyword(&self, n: &str) -> bool {
        if self.py {
            matches!(
                n,
                "and"
                    | "or"
                    | "not"
                    | "if"
                    | "elif"
                    | "else"
                    | "while"
                    | "for"
                    | "in"
                    | "def"
                    | "return"
                    | "break"
                    | "continue"
                    | "pass"
                    | "global"
                    | "import"
                    | "from"
                    | "lambda"
                    | "True"
                    | "False"
                    | "None"
                    | "is"
            )
        } else {
            matches!(
                n,
                "let"
                    | "const"
                    | "var"
                    | "function"
                    | "if"
                    | "else"
                    | "while"
                    | "for"
                    | "of"
                    | "return"
                    | "break"
                    | "continue"
                    | "true"
                    | "false"
                    | "null"
                    | "new"
                    | "typeof"
                    | "do"
                    | "switch"
            )
        }
    }

    fn ident(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Name(n) if !self.keyword(&n) => {
                self.at += 1;
                Ok(n)
            }
            Tok::Name(n) => self.fail(format!("`{n}` is a word of the language, not a name")),
            _ => self.fail(format!("expected a name, found {}", describe(self.peek()))),
        }
    }

    fn params(&mut self) -> R<Vec<String>> {
        self.expect_op("(")?;
        let mut v = Vec::new();
        while !self.is_op(")") {
            v.push(self.ident()?);
            if self.is_op("=") {
                return self.fail("default values for parameters are not supported");
            }
            if !self.eat_op(",") {
                break;
            }
            if v.len() > 16 {
                return self.fail("more than 16 parameters");
            }
        }
        self.expect_op(")")?;
        Ok(v)
    }

    /// Counts one more operator or postfix step; a very long chain is refused.
    fn link(&mut self) -> R<()> {
        self.chain += 1;
        if self.chain > MAX_CHAIN {
            return self.fail("the expression is too long");
        }
        Ok(())
    }

    fn add_func(&mut self, f: FuncDef) -> R<usize> {
        if self.funcs.len() >= MAX_FUNCS {
            return self.fail(format!("more than {MAX_FUNCS} functions"));
        }
        self.funcs.push(f);
        Ok(self.funcs.len() - 1)
    }

    // ---- expressions ----

    fn expr(&mut self) -> R<Expr> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("the expression is nested too deeply");
        }
        let r = self.ternary();
        self.depth -= 1;
        r
    }

    fn ternary(&mut self) -> R<Expr> {
        if self.py && self.is_kw("lambda") {
            let line = self.line();
            self.bump();
            let mut params = Vec::new();
            while !self.is_op(":") {
                params.push(self.ident()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(":")?;
            let e = self.expr()?;
            let idx = self.add_func(FuncDef { name: "<lambda>".into(), params, body: vec![Stmt { line, kind: Kind::Return(Some(e)) }], line })?;
            return Ok(Expr::Func(idx));
        }
        let first = self.or_expr()?;
        if self.py {
            if self.eat_kw("if") {
                let cond = self.or_expr()?;
                self.expect_kw("else")?;
                let other = self.expr()?;
                return Ok(Expr::Cond(Box::new(cond), Box::new(first), Box::new(other)));
            }
        } else if self.eat_op("?") {
            let a = self.expr()?;
            self.expect_op(":")?;
            let b = self.expr()?;
            return Ok(Expr::Cond(Box::new(first), Box::new(a), Box::new(b)));
        }
        Ok(first)
    }

    fn or_expr(&mut self) -> R<Expr> {
        let mut left = self.and_expr()?;
        loop {
            let more = if self.py { self.eat_kw("or") } else { self.eat_op("||") };
            if !more {
                break;
            }
            self.link()?;
            let right = self.and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> R<Expr> {
        let mut left = self.not_expr()?;
        loop {
            let more = if self.py { self.eat_kw("and") } else { self.eat_op("&&") };
            if !more {
                break;
            }
            self.link()?;
            let right = self.not_expr()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn not_expr(&mut self) -> R<Expr> {
        if self.py && self.eat_kw("not") {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return self.fail("the expression is nested too deeply");
            }
            let e = self.not_expr()?;
            self.depth -= 1;
            return Ok(Expr::Un(Uop::Not, Box::new(e)));
        }
        self.compare()
    }

    fn compare(&mut self) -> R<Expr> {
        let mut left = self.add()?;
        loop {
            let op = match self.peek().clone() {
                Tok::Op("==" | "===") => Bop::Eq,
                Tok::Op("!=" | "!==") => Bop::Ne,
                Tok::Op("<") => Bop::Lt,
                Tok::Op(">") => Bop::Gt,
                Tok::Op("<=") => Bop::Le,
                Tok::Op(">=") => Bop::Ge,
                Tok::Name(n) if self.py && n == "in" => Bop::In,
                Tok::Name(n) if self.py && n == "not" && matches!(self.peek_at(1), Tok::Name(m) if m == "in") => {
                    self.at += 1;
                    Bop::NotIn
                }
                Tok::Name(n) if self.py && n == "is" => {
                    if matches!(self.peek_at(1), Tok::Name(m) if m == "not") {
                        self.at += 1;
                        Bop::Ne
                    } else {
                        Bop::Eq
                    }
                }
                _ => return Ok(left),
            };
            self.bump();
            self.link()?;
            let right = self.add()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn add(&mut self) -> R<Expr> {
        let mut left = self.mul()?;
        loop {
            let op = if self.is_op("+") {
                Bop::Add
            } else if self.is_op("-") {
                Bop::Sub
            } else {
                return Ok(left);
            };
            self.bump();
            self.link()?;
            let right = self.mul()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn mul(&mut self) -> R<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Op("*") => Bop::Mul,
                Tok::Op("/") => Bop::Div,
                Tok::Op("//") => Bop::FloorDiv,
                Tok::Op("%") => Bop::Mod,
                _ => return Ok(left),
            };
            self.bump();
            self.link()?;
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> R<Expr> {
        let op = match self.peek() {
            Tok::Op("-") => Some(Uop::Neg),
            Tok::Op("+") => Some(Uop::Pos),
            Tok::Op("!") if !self.py => Some(Uop::Not),
            _ => None,
        };
        let Some(op) = op else { return self.power() };
        self.bump();
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("the expression is nested too deeply");
        }
        let e = self.unary()?;
        self.depth -= 1;
        Ok(Expr::Un(op, Box::new(e)))
    }

    fn power(&mut self) -> R<Expr> {
        let base = self.postfix()?;
        if self.eat_op("**") {
            let e = self.unary()?;
            return Ok(Expr::Bin(Bop::Pow, Box::new(base), Box::new(e)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            if matches!(self.peek(), Tok::Op("(" | "[" | ".")) {
                self.link()?;
            }
            if self.eat_op("(") {
                let mut args = Vec::new();
                while !self.is_op(")") {
                    if self.py && matches!(self.peek(), Tok::Name(_)) && matches!(self.peek_at(1), Tok::Op("=")) {
                        return self.fail("keyword arguments are not supported");
                    }
                    args.push(self.expr()?);
                    if !self.eat_op(",") {
                        break;
                    }
                    if args.len() > 16 {
                        return self.fail("more than 16 arguments");
                    }
                }
                self.expect_op(")")?;
                e = Expr::Call(Box::new(e), args);
            } else if self.eat_op("[") {
                let i = self.expr()?;
                self.expect_op("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.eat_op(".") {
                match self.bump() {
                    Tok::Name(n) => e = Expr::Attr(Box::new(e), n),
                    _ => return self.fail("expected a name after `.`"),
                }
            } else {
                return Ok(e);
            }
        }
    }

    fn arrow_ahead(&self) -> bool {
        let mut j = self.at + 1;
        let mut depth = 1usize;
        while let Some((t, _)) = self.toks.get(j) {
            match t {
                Tok::Op("(") => depth += 1,
                Tok::Op(")") => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.toks.get(j + 1), Some((Tok::Op("=>"), _)));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            j += 1;
        }
        false
    }

    fn arrow(&mut self, params: Vec<String>, line: usize) -> R<Expr> {
        self.expect_op("=>")?;
        let body = if self.is_op("{") {
            self.js_block()?
        } else {
            let e = self.expr()?;
            vec![Stmt { line, kind: Kind::Return(Some(e)) }]
        };
        let idx = self.add_func(FuncDef { name: "<arrow>".into(), params, body, line })?;
        Ok(Expr::Func(idx))
    }

    /// An expression written inside an f-string or a template.
    fn sub_expr(&mut self, src: &str, line: usize) -> R<Expr> {
        let (code, spec) = if self.py { split_spec(src) } else { (src, "") };
        let toks = lex(code, self.py, line)?;
        let mut sub = P { toks, at: 0, py: self.py, funcs: std::mem::take(&mut self.funcs), depth: self.depth + 1, nest: 0, chain: self.chain };
        let r = sub.expr();
        let done = matches!(sub.peek(), Tok::Eof | Tok::Newline);
        let at = sub.line();
        self.funcs = std::mem::take(&mut sub.funcs);
        let e = r?;
        if !done {
            return Err(format!("line {at}: more than one expression inside the braces"));
        }
        // `{x:.2f}`: the only format specification supported.
        if let Some(digits) = spec.strip_prefix('.').and_then(|s| s.strip_suffix('f')).and_then(|d| d.parse::<i64>().ok()) {
            return Ok(Expr::Call(Box::new(Expr::Attr(Box::new(e), "toFixed".into())), vec![lit_int(digits)]));
        }
        Ok(e)
    }

    fn primary(&mut self) -> R<Expr> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(lit_int(n))
            }
            Tok::Float(f) => {
                self.bump();
                Ok(Expr::Lit(V::Float(f)))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Lit(V::Str(s)))
            }
            Tok::Fmt(parts) => {
                self.bump();
                let mut out = Vec::new();
                for p in parts {
                    out.push(match p {
                        Part::Lit(s) => Expr::Lit(V::Str(s)),
                        Part::Code(c) => self.sub_expr(&c, line)?,
                    });
                }
                Ok(Expr::Fmt(out))
            }
            Tok::Op("(") => {
                if !self.py && self.arrow_ahead() {
                    let params = self.params()?;
                    return self.arrow(params, line);
                }
                self.bump();
                let e = self.expr()?;
                self.expect_op(")")?;
                Ok(e)
            }
            Tok::Op("[") => {
                self.bump();
                let mut items = Vec::new();
                while !self.is_op("]") {
                    items.push(self.expr()?);
                    if !self.eat_op(",") {
                        break;
                    }
                    if items.len() > 500 {
                        return self.fail("more than 500 values in a list");
                    }
                }
                self.expect_op("]")?;
                Ok(Expr::List(items))
            }
            Tok::Name(n) => {
                if self.py {
                    match n.as_str() {
                        "True" => return self.lit(V::Bool(true)),
                        "False" => return self.lit(V::Bool(false)),
                        "None" => return self.lit(V::None),
                        _ => {}
                    }
                } else {
                    match n.as_str() {
                        "true" => return self.lit(V::Bool(true)),
                        "false" => return self.lit(V::Bool(false)),
                        "null" => return self.lit(V::None),
                        "function" => {
                            self.bump();
                            if matches!(self.peek(), Tok::Name(_)) {
                                self.bump();
                            }
                            let params = self.params()?;
                            let body = self.js_block()?;
                            let idx = self.add_func(FuncDef { name: "<function>".into(), params, body, line })?;
                            return Ok(Expr::Func(idx));
                        }
                        _ => {}
                    }
                    if matches!(self.peek_at(1), Tok::Op("=>")) {
                        let p = self.ident()?;
                        return self.arrow(vec![p], line);
                    }
                }
                let name = self.ident()?;
                Ok(Expr::Name(name))
            }
            _ => self.unexpected(),
        }
    }

    fn lit(&mut self, v: V) -> R<Expr> {
        self.bump();
        Ok(Expr::Lit(v))
    }

    // ---- Python statements ----

    fn end_stmt(&mut self) -> R<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            _ => self.unexpected(),
        }
    }

    fn py_block(&mut self) -> R<Vec<Stmt>> {
        self.expect_op(":")?;
        let mut out = Vec::new();
        if matches!(self.peek(), Tok::Newline) {
            self.bump();
            if !matches!(self.peek(), Tok::Indent) {
                return self.fail("expected an indented block");
            }
            self.bump();
            loop {
                match self.peek() {
                    Tok::Dedent => {
                        self.bump();
                        break;
                    }
                    Tok::Eof => break,
                    _ => self.py_stmt(&mut out)?,
                }
            }
        } else {
            self.py_simple(&mut out)?;
            self.end_stmt()?;
        }
        Ok(out)
    }

    fn py_stmt(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        self.nest += 1;
        if self.nest > MAX_NEST {
            return self.fail("blocks nested too deeply");
        }
        self.chain = 0;
        let r = self.py_stmt_inner(out);
        self.nest -= 1;
        r
    }

    fn py_stmt_inner(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        let line = self.line();
        let word = match self.peek() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        match word.as_str() {
            "if" => {
                self.bump();
                let cond = self.expr()?;
                let body = self.py_block()?;
                let mut arms = vec![(cond, body)];
                let mut other = Vec::new();
                loop {
                    if self.eat_kw("elif") {
                        let c = self.expr()?;
                        let b = self.py_block()?;
                        arms.push((c, b));
                    } else if self.eat_kw("else") {
                        other = self.py_block()?;
                        break;
                    } else {
                        break;
                    }
                }
                out.push(Stmt { line, kind: Kind::If(arms, other) });
            }
            "while" => {
                self.bump();
                let cond = self.expr()?;
                let body = self.py_block()?;
                out.push(Stmt { line, kind: Kind::While(cond, body) });
            }
            "for" => {
                self.bump();
                let var = self.ident()?;
                self.expect_kw("in")?;
                let iter = self.expr()?;
                let body = self.py_block()?;
                out.push(Stmt { line, kind: Kind::ForIn(var, iter, body) });
            }
            "def" => {
                self.bump();
                let name = self.ident()?;
                let params = self.params()?;
                let body = self.py_block()?;
                let idx = self.add_func(FuncDef { name: name.clone(), params, body, line })?;
                out.push(Stmt { line, kind: Kind::Def(name, idx) });
            }
            "class" | "try" | "with" | "del" | "assert" | "raise" | "yield" | "async" | "await" | "nonlocal" | "elif" | "else" => {
                return self.fail(format!("`{word}` is not part of the Python subset the boards run"));
            }
            _ => {
                self.py_simple(out)?;
                self.end_stmt()?;
            }
        }
        Ok(())
    }

    fn py_simple(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        let line = self.line();
        let word = match self.peek() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        match word.as_str() {
            "return" => {
                self.bump();
                let v = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent) { None } else { Some(self.expr()?) };
                out.push(Stmt { line, kind: Kind::Return(v) });
            }
            "break" => {
                self.bump();
                out.push(Stmt { line, kind: Kind::Break });
            }
            "continue" => {
                self.bump();
                out.push(Stmt { line, kind: Kind::Continue });
            }
            "pass" => {
                self.bump();
            }
            "global" => {
                self.bump();
                let mut names = vec![self.ident()?];
                while self.eat_op(",") {
                    names.push(self.ident()?);
                }
                out.push(Stmt { line, kind: Kind::Global(names) });
            }
            "import" => {
                self.bump();
                loop {
                    let m = self.ident()?;
                    if !matches!(m.as_str(), "time" | "math") {
                        return self.fail(format!("`import {m}`: only time and math are available on a board"));
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
            }
            "from" => {
                self.bump();
                let m = self.ident()?;
                if !matches!(m.as_str(), "time" | "math") {
                    return self.fail(format!("`from {m}`: only time and math are available on a board"));
                }
                self.expect_kw("import")?;
                // `from time import sleep` works as written: those names are available anyway.
                while !matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent) {
                    self.bump();
                }
            }
            _ => {
                let e = self.expr()?;
                let aug = match self.peek() {
                    Tok::Op("+=") => Some(Bop::Add),
                    Tok::Op("-=") => Some(Bop::Sub),
                    Tok::Op("*=") => Some(Bop::Mul),
                    Tok::Op("/=") => Some(Bop::Div),
                    Tok::Op("//=") => Some(Bop::FloorDiv),
                    Tok::Op("%=") => Some(Bop::Mod),
                    Tok::Op("**=") => Some(Bop::Pow),
                    _ => None,
                };
                if self.eat_op("=") {
                    let target = to_target(e, line)?;
                    let value = self.expr()?;
                    out.push(Stmt { line, kind: Kind::Set { target, value, declare: true } });
                } else if let Some(op) = aug {
                    self.bump();
                    let target = to_target(e.clone(), line)?;
                    let v = self.expr()?;
                    let value = Expr::Bin(op, Box::new(e), Box::new(v));
                    out.push(Stmt { line, kind: Kind::Set { target, value, declare: true } });
                } else {
                    out.push(Stmt { line, kind: Kind::Expr(e) });
                }
            }
        }
        Ok(())
    }

    // ---- JavaScript statements ----

    fn semi(&mut self) {
        self.eat_op(";");
    }

    fn js_block(&mut self) -> R<Vec<Stmt>> {
        self.expect_op("{")?;
        let mut out = Vec::new();
        while !self.is_op("}") {
            if matches!(self.peek(), Tok::Eof) {
                return self.fail("a `{` is never closed with `}`");
            }
            self.js_stmt(&mut out)?;
        }
        self.expect_op("}")?;
        Ok(hoist(out))
    }

    fn js_body(&mut self) -> R<Vec<Stmt>> {
        if self.is_op("{") {
            return self.js_block();
        }
        let mut v = Vec::new();
        self.js_stmt(&mut v)?;
        Ok(v)
    }

    fn js_paren(&mut self) -> R<Expr> {
        self.expect_op("(")?;
        let e = self.expr()?;
        self.expect_op(")")?;
        Ok(e)
    }

    fn js_stmt(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        self.nest += 1;
        if self.nest > MAX_NEST {
            return self.fail("blocks nested too deeply");
        }
        self.chain = 0;
        let r = self.js_stmt_inner(out);
        self.nest -= 1;
        r
    }

    fn js_stmt_inner(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        let line = self.line();
        if self.eat_op(";") {
            return Ok(());
        }
        if self.is_op("{") {
            let body = self.js_block()?;
            out.push(Stmt { line, kind: Kind::If(vec![(Expr::Lit(V::Bool(true)), body)], Vec::new()) });
            return Ok(());
        }
        let word = match self.peek() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        match word.as_str() {
            "let" | "const" | "var" => {
                self.bump();
                loop {
                    let name = self.ident()?;
                    let value = if self.eat_op("=") { self.expr()? } else { Expr::Lit(V::None) };
                    out.push(Stmt { line, kind: Kind::Set { target: Target::Name(name), value, declare: true } });
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.semi();
            }
            "function" => {
                self.bump();
                let name = self.ident()?;
                let params = self.params()?;
                let body = self.js_block()?;
                let idx = self.add_func(FuncDef { name: name.clone(), params, body, line })?;
                out.push(Stmt { line, kind: Kind::Def(name, idx) });
            }
            "if" => {
                self.bump();
                let cond = self.js_paren()?;
                let body = self.js_body()?;
                let mut other = Vec::new();
                if self.eat_kw("else") {
                    other = if self.is_kw("if") {
                        let mut v = Vec::new();
                        self.js_stmt(&mut v)?;
                        v
                    } else {
                        self.js_body()?
                    };
                }
                out.push(Stmt { line, kind: Kind::If(vec![(cond, body)], other) });
            }
            "while" => {
                self.bump();
                let cond = self.js_paren()?;
                let body = self.js_body()?;
                out.push(Stmt { line, kind: Kind::While(cond, body) });
            }
            "for" => {
                self.bump();
                self.expect_op("(")?;
                let of_loop = matches!(self.peek(), Tok::Name(k) if matches!(k.as_str(), "let" | "const" | "var"))
                    && matches!(self.peek_at(1), Tok::Name(_))
                    && matches!(self.peek_at(2), Tok::Name(o) if o == "of");
                if of_loop {
                    self.bump();
                    let var = self.ident()?;
                    self.bump();
                    let iter = self.expr()?;
                    self.expect_op(")")?;
                    let body = self.js_body()?;
                    out.push(Stmt { line, kind: Kind::ForIn(var, iter, body) });
                } else {
                    let mut init = Vec::new();
                    if !self.eat_op(";") {
                        self.js_stmt(&mut init)?;
                    }
                    let cond = if self.is_op(";") { None } else { Some(self.expr()?) };
                    self.expect_op(";")?;
                    let mut step = Vec::new();
                    while !self.is_op(")") {
                        self.js_simple(&mut step)?;
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op(")")?;
                    let body = self.js_body()?;
                    out.push(Stmt { line, kind: Kind::ForC(init, cond, step, body) });
                }
            }
            "return" => {
                self.bump();
                let v = if self.is_op(";") || self.is_op("}") || matches!(self.peek(), Tok::Eof) { None } else { Some(self.expr()?) };
                self.semi();
                out.push(Stmt { line, kind: Kind::Return(v) });
            }
            "break" => {
                self.bump();
                self.semi();
                out.push(Stmt { line, kind: Kind::Break });
            }
            "continue" => {
                self.bump();
                self.semi();
                out.push(Stmt { line, kind: Kind::Continue });
            }
            "do" | "switch" | "try" | "throw" | "class" | "new" | "async" | "await" | "import" | "export" | "typeof" | "delete" | "with" => {
                return self.fail(format!("`{word}` is not part of the JavaScript subset the boards run"));
            }
            _ => {
                self.js_simple(out)?;
                self.semi();
            }
        }
        Ok(())
    }

    /// An assignment, an increment or a call, without its closing `;`.
    fn js_simple(&mut self, out: &mut Vec<Stmt>) -> R<()> {
        let line = self.line();
        let step = |op: Bop, e: Expr| Expr::Bin(op, Box::new(e), Box::new(lit_int(1)));
        if self.is_op("++") || self.is_op("--") {
            let add = self.is_op("++");
            self.bump();
            let e = self.postfix()?;
            let target = to_target(e.clone(), line)?;
            let value = step(if add { Bop::Add } else { Bop::Sub }, e);
            out.push(Stmt { line, kind: Kind::Set { target, value, declare: false } });
            return Ok(());
        }
        let e = self.expr()?;
        let aug = match self.peek() {
            Tok::Op("+=") => Some(Bop::Add),
            Tok::Op("-=") => Some(Bop::Sub),
            Tok::Op("*=") => Some(Bop::Mul),
            Tok::Op("/=") => Some(Bop::Div),
            Tok::Op("%=") => Some(Bop::Mod),
            Tok::Op("**=") => Some(Bop::Pow),
            _ => None,
        };
        if self.is_op("++") || self.is_op("--") {
            let add = self.is_op("++");
            self.bump();
            let target = to_target(e.clone(), line)?;
            let value = step(if add { Bop::Add } else { Bop::Sub }, e);
            out.push(Stmt { line, kind: Kind::Set { target, value, declare: false } });
        } else if self.eat_op("=") {
            let target = to_target(e, line)?;
            let value = self.expr()?;
            out.push(Stmt { line, kind: Kind::Set { target, value, declare: false } });
        } else if let Some(op) = aug {
            self.bump();
            let target = to_target(e.clone(), line)?;
            let v = self.expr()?;
            out.push(Stmt { line, kind: Kind::Set { target, value: Expr::Bin(op, Box::new(e), Box::new(v)), declare: false } });
        } else {
            out.push(Stmt { line, kind: Kind::Expr(e) });
        }
        Ok(())
    }
}

/// Splits `x:.2f` at the first colon outside brackets and quotes.
fn split_spec(src: &str) -> (&str, &str) {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in src.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            (None, ':') if depth == 0 => return (src.get(..i).unwrap_or(src), src.get(i + 1..).unwrap_or_default()),
            _ => {}
        }
    }
    (src, "")
}

/// Parses a program in Python (`py`) or JavaScript.
pub fn parse(src: &str, py: bool) -> R<Ast> {
    let toks = lex(src, py, 1)?;
    let mut p = P { toks, at: 0, py, funcs: Vec::new(), depth: 0, nest: 0, chain: 0 };
    let mut body = Vec::new();
    while !matches!(p.peek(), Tok::Eof) {
        if py {
            p.py_stmt(&mut body)?;
        } else {
            p.js_stmt(&mut body)?;
        }
    }
    if !py {
        body = hoist(body);
    }
    Ok(Ast { body, funcs: p.funcs })
}
