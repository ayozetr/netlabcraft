//! BoardScript: the small, line-based language programmable boards run. Our own design, made
//! to be safe inside the simulator: no file, no network but an HTTP GET to the board's IoT
//! server, every loop bounded by a step cap, every value and list capped.
//!
//! ```text
//! # comments start with a hash
//! let limit = 30            # variables hold whole numbers or text
//! t = read(A0)              # a pin: the wired sensor's reading (digital pins read 0 or 1)
//! if t > limit and t < 100
//!   write(D0, 1)            # a pin: switches the wired smart device on (0: off)
//! elif t < 10
//!   print "cold:", t
//! else
//!   write(D0, 0)
//! end
//! while x < 3
//!   x = x + 1
//! end
//! state = get("/iot/state?name=Fan1")   # HTTP GET to the board's IoT server: the answer's text
//! report t                  # registers the board with its IoT server as a sensor reading t
//! sleep 2                   # waits 2 s of simulated time (at least 1)
//! stop                      # ends the program (else it starts over a second after its end)
//! ```
//!
//! Operators: `+ - * / %`, `== != < > <= >=`, `and`, `or`, `not`, parentheses; `+` joins text.
//! Functions: `read(PIN)`, `get(TEXT)`, `time()` (seconds of simulated time), `number(TEXT)`.
//!
//! A program is compiled to a list of steps with jumps, so it can pause at a `sleep` and carry
//! on later. Each run slice executes at most [`MAX_STEPS`] steps: a loop that never sleeps stops
//! the program with a message instead of holding the simulator up.

use std::collections::BTreeMap;

/// The longest program (characters) and the most lines.
pub const MAX_SOURCE: usize = 8_000;
pub const MAX_LINES: usize = 400;
/// Most steps one run slice executes before the program is stopped.
pub const MAX_STEPS: usize = 5_000;
/// Most variables, the longest text value, the most log lines kept, blocks nested.
pub const MAX_VARS: usize = 64;
pub const MAX_TEXT: usize = 200;
pub const MAX_LOG: usize = 60;
const MAX_DEPTH: usize = 16;
/// Most HTTP requests and reports in one slice.
const MAX_REQUESTS: usize = 8;
/// The longest sleep (seconds).
pub const MAX_SLEEP: i64 = 3_600;

/// A value: a whole number or text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    Int(i64),
    Text(String),
}

impl Val {
    fn truthy(&self) -> bool {
        match self {
            Val::Int(n) => *n != 0,
            Val::Text(t) => !t.is_empty(),
        }
    }

    fn int(&self) -> Result<i64, String> {
        match self {
            Val::Int(n) => Ok(*n),
            Val::Text(t) => Err(format!("\"{t}\" is text, not a number (number(...) reads one)")),
        }
    }

    pub fn show(&self) -> String {
        match self {
            Val::Int(n) => n.to_string(),
            Val::Text(t) => t.clone(),
        }
    }
}

/// What a board offers its program: pins, its IoT server and the clock.
pub trait Io {
    /// A pin's level: the wired sensor's reading or device's state, or what was written.
    fn read(&mut self, pin: &str) -> Result<i64, String>;
    fn write(&mut self, pin: &str, level: i64) -> Result<(), String>;
    /// An HTTP GET to the board's IoT server: the answer's text.
    fn get(&mut self, path: &str) -> Result<String, String>;
    /// Registers the board with its IoT server as a sensor with this reading.
    fn report(&mut self, value: i64) -> Result<(), String>;
    /// Whether a pin name is one of the board's.
    fn has_pin(&self, pin: &str) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Num(i64),
    Str(String),
    Name(String),
    Sym(&'static str),
}

fn lex(line: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let cs: Vec<char> = line.chars().collect();
    let mut i = 0;
    while let Some(&c) = cs.get(i) {
        if c == '#' {
            break;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let mut n: i64 = 0;
            while let Some(d) = cs.get(i).and_then(|c| c.to_digit(10)) {
                n = n.checked_mul(10).and_then(|n| n.checked_add(i64::from(d))).ok_or("number too large")?;
                i += 1;
            }
            out.push(Tok::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while cs.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
                i += 1;
            }
            out.push(Tok::Name(cs.get(start..i).unwrap_or_default().iter().collect()));
            continue;
        }
        if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                match cs.get(i) {
                    None => return Err("text without its closing \"".into()),
                    Some('"') => break,
                    Some(c) => s.push(*c),
                }
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s.chars().take(MAX_TEXT).collect()));
            continue;
        }
        let two: String = cs.get(i..i + 2).map(|x| x.iter().collect()).unwrap_or_default();
        let sym = match two.as_str() {
            "==" => Some("=="),
            "!=" => Some("!="),
            "<=" => Some("<="),
            ">=" => Some(">="),
            _ => None,
        };
        if let Some(s) = sym {
            out.push(Tok::Sym(s));
            i += 2;
            continue;
        }
        let one = match c {
            '+' => "+",
            '-' => "-",
            '*' => "*",
            '/' => "/",
            '%' => "%",
            '<' => "<",
            '>' => ">",
            '=' => "=",
            '(' => "(",
            ')' => ")",
            ',' => ",",
            _ => return Err(format!("`{c}` is not part of the language")),
        };
        out.push(Tok::Sym(one));
        i += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Lit(Val),
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Read(String),
    Get(Box<Expr>),
    Time,
    Number(Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(x)) if *x == s) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Name(x)) if x.eq_ignore_ascii_case(w)) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, s: &str) -> Result<(), String> {
        if self.eat_sym(s) { Ok(()) } else { Err(format!("`{s}` expected")) }
    }

    fn done(&self) -> bool {
        self.at >= self.toks.len()
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut e = self.and()?;
        while self.eat_word("or") {
            e = Expr::Bin("or", Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut e = self.not()?;
        while self.eat_word("and") {
            e = Expr::Bin("and", Box::new(e), Box::new(self.not()?));
        }
        Ok(e)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if self.eat_word("not") {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }

    fn cmp(&mut self) -> Result<Expr, String> {
        let e = self.sum()?;
        for op in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.eat_sym(op) {
                return Ok(Expr::Bin(op, Box::new(e), Box::new(self.sum()?)));
            }
        }
        // A lone `=` in a condition is a slip for `==`.
        if self.eat_sym("=") {
            return Ok(Expr::Bin("==", Box::new(e), Box::new(self.sum()?)));
        }
        Ok(e)
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        loop {
            let op = if self.eat_sym("+") {
                "+"
            } else if self.eat_sym("-") {
                "-"
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat_sym("*") {
                "*"
            } else if self.eat_sym("/") {
                "/"
            } else if self.eat_sym("%") {
                "%"
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_sym("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn pin(&mut self) -> Result<String, String> {
        match self.toks.get(self.at).cloned() {
            Some(Tok::Name(n)) => {
                self.at += 1;
                Ok(n.to_ascii_uppercase())
            }
            _ => Err("a pin name (D0, A1…) expected".into()),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let t = self.toks.get(self.at).cloned().ok_or("a value expected")?;
        self.at += 1;
        match t {
            Tok::Num(n) => Ok(Expr::Lit(Val::Int(n))),
            Tok::Str(s) => Ok(Expr::Lit(Val::Text(s))),
            Tok::Sym("(") => {
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Name(n) => {
                let lower = n.to_ascii_lowercase();
                match lower.as_str() {
                    "true" => return Ok(Expr::Lit(Val::Int(1))),
                    "false" => return Ok(Expr::Lit(Val::Int(0))),
                    _ => {}
                }
                if !self.eat_sym("(") {
                    if KEYWORDS.contains(&lower.as_str()) {
                        return Err(format!("`{n}` is a word of the language, not a value"));
                    }
                    return Ok(Expr::Var(lower));
                }
                let e = match lower.as_str() {
                    "read" => Expr::Read(self.pin()?),
                    "get" => Expr::Get(Box::new(self.expr()?)),
                    "number" => Expr::Number(Box::new(self.expr()?)),
                    "time" => Expr::Time,
                    _ => return Err(format!("no function `{n}` (there are read, get, time and number)")),
                };
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Sym(s) => Err(format!("`{s}` where a value was expected")),
        }
    }
}

const KEYWORDS: [&str; 15] = ["let", "if", "elif", "else", "end", "while", "sleep", "print", "write", "report", "stop", "and", "or", "not", "read"];

/// One compiled step.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Let(String, Expr),
    Write(String, Expr),
    Print(Vec<Expr>),
    Sleep(Expr),
    Report(Expr),
    Stop,
    /// Jump to the target unless the condition holds.
    Unless(Expr, usize),
    Jump(usize),
}

/// A compiled program: its steps and the source line of each.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    steps: Vec<(Step, usize)>,
}

enum Block {
    If { unless: Option<usize>, ends: Vec<usize> },
    While { start: usize, unless: usize },
}

/// Compiles a program; an error names its line (from 1).
pub fn compile(source: &str) -> Result<Program, String> {
    if source.chars().count() > MAX_SOURCE {
        return Err(format!("the program is longer than {MAX_SOURCE} characters"));
    }
    if source.lines().count() > MAX_LINES {
        return Err(format!("the program has more than {MAX_LINES} lines"));
    }
    let mut steps: Vec<(Step, usize)> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let line_no = n + 1;
        let at = |e: String| format!("line {line_no}: {e}");
        let toks = lex(line).map_err(at)?;
        let Some(first) = toks.first().cloned() else { continue };
        let mut p = Parser { toks, at: 1 };
        let word = match &first {
            Tok::Name(w) => w.to_ascii_lowercase(),
            _ => return Err(at("a line starts with a word (let, if, while, write, sleep…) or a variable".into())),
        };
        let fill = |steps: &mut Vec<(Step, usize)>, i: usize, to: usize| {
            if let Some((Step::Unless(_, t) | Step::Jump(t), _)) = steps.get_mut(i) {
                *t = to;
            }
        };
        match word.as_str() {
            "if" | "while" => {
                if blocks.len() >= MAX_DEPTH {
                    return Err(at(format!("blocks nested deeper than {MAX_DEPTH}")));
                }
                let cond = p.expr().map_err(at)?;
                let i = steps.len();
                steps.push((Step::Unless(cond, 0), line_no));
                blocks.push(if word == "if" { Block::If { unless: Some(i), ends: Vec::new() } } else { Block::While { start: i, unless: i } });
            }
            "elif" | "else" => {
                let Some(Block::If { unless, ends }) = blocks.last_mut() else {
                    return Err(at(format!("`{word}` without its `if`")));
                };
                let Some(prev) = unless.take() else { return Err(at(format!("`{word}` after the `else`"))) };
                ends.push(steps.len());
                steps.push((Step::Jump(0), line_no));
                let here = steps.len();
                fill(&mut steps, prev, here);
                if word == "elif" {
                    let cond = p.expr().map_err(at)?;
                    if let Some(Block::If { unless, .. }) = blocks.last_mut() {
                        *unless = Some(steps.len());
                    }
                    steps.push((Step::Unless(cond, 0), line_no));
                }
            }
            "end" => match blocks.pop() {
                None => return Err(at("`end` without an `if` or `while`".into())),
                Some(Block::If { unless, ends }) => {
                    let here = steps.len();
                    if let Some(u) = unless {
                        fill(&mut steps, u, here);
                    }
                    for e in ends {
                        fill(&mut steps, e, here);
                    }
                }
                Some(Block::While { start, unless }) => {
                    steps.push((Step::Jump(start), line_no));
                    let here = steps.len();
                    fill(&mut steps, unless, here);
                }
            },
            "let" => {
                let Some(Tok::Name(v)) = p.toks.get(1).cloned() else { return Err(at("`let NAME = value`".into())) };
                p.at = 2;
                p.expect_sym("=").map_err(at)?;
                steps.push((Step::Let(var_name(&v).map_err(at)?, p.expr().map_err(at)?), line_no));
            }
            "write" => {
                p.expect_sym("(").map_err(at)?;
                let pin = p.pin().map_err(at)?;
                p.expect_sym(",").map_err(at)?;
                let e = p.expr().map_err(at)?;
                p.expect_sym(")").map_err(at)?;
                steps.push((Step::Write(pin, e), line_no));
            }
            "print" => {
                let mut parts = vec![p.expr().map_err(at)?];
                while p.eat_sym(",") {
                    parts.push(p.expr().map_err(at)?);
                }
                steps.push((Step::Print(parts), line_no));
            }
            "sleep" => steps.push((Step::Sleep(p.expr().map_err(at)?), line_no)),
            "report" => steps.push((Step::Report(p.expr().map_err(at)?), line_no)),
            "stop" => steps.push((Step::Stop, line_no)),
            _ if KEYWORDS.contains(&word.as_str()) => return Err(at(format!("`{word}` can't start a line"))),
            _ => {
                if !p.eat_sym("=") {
                    return Err(at(format!("`{word}` is not a statement: did you mean `{word} = …`?")));
                }
                steps.push((Step::Let(var_name(&word).map_err(at)?, p.expr().map_err(at)?), line_no));
            }
        }
        if !p.done() {
            return Err(at("more than one statement on the line".into()));
        }
    }
    if !blocks.is_empty() {
        return Err(format!("{} block(s) without their `end`", blocks.len()));
    }
    Ok(Program { steps })
}

fn var_name(v: &str) -> Result<String, String> {
    let lower = v.to_ascii_lowercase();
    if KEYWORDS.contains(&lower.as_str()) || matches!(lower.as_str(), "true" | "false" | "get" | "time" | "number") {
        return Err(format!("`{v}` is a word of the language, not a variable"));
    }
    Ok(lower)
}

/// Where a running program is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// Runs again at this simulated time (microseconds).
    Waiting(u64),
    /// Stopped: why (`None`: by `stop`).
    Stopped(Option<String>),
}

/// A program running on a board: where it is, its variables and what it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    program: Program,
    pc: usize,
    pub vars: BTreeMap<String, Val>,
    pub log: Vec<String>,
    pub state: RunState,
    /// The source line it is at (from 1), for the editor.
    pub line: usize,
    /// Steps taken since it started.
    pub steps: u64,
}

impl Machine {
    /// A machine about to run `program` at `now`.
    pub fn new(program: Program, now: u64) -> Machine {
        Machine { program, pc: 0, vars: BTreeMap::new(), log: Vec::new(), state: RunState::Waiting(now), line: 1, steps: 0 }
    }

    /// When it runs next, if it isn't stopped.
    pub fn due(&self) -> Option<u64> {
        match self.state {
            RunState::Waiting(at) => Some(at),
            RunState::Stopped(_) => None,
        }
    }

    pub fn note(&mut self, line: String) {
        self.log.push(line.chars().take(MAX_TEXT).collect());
        if self.log.len() > MAX_LOG {
            let extra = self.log.len() - MAX_LOG;
            self.log.drain(..extra);
        }
    }

    fn stop(&mut self, why: String) {
        let line = self.program.steps.get(self.pc).map_or(self.line, |s| s.1);
        let msg = format!("line {line}: {why}");
        self.note(format!("stopped: {msg}"));
        self.state = RunState::Stopped(Some(msg));
    }

    /// Runs from where it is until a `sleep`, the end, a `stop`, an error or the step cap.
    pub fn run(&mut self, io: &mut dyn Io, now: u64) {
        if self.due().is_none_or(|at| at > now) {
            return;
        }
        let mut requests = 0usize;
        for _ in 0..MAX_STEPS {
            self.steps = self.steps.saturating_add(1);
            let Some((step, line)) = self.program.steps.get(self.pc).cloned() else {
                // The end: like a microcontroller's loop, it starts over a second later.
                self.pc = 0;
                self.line = 1;
                self.state = RunState::Waiting(now.saturating_add(1_000_000));
                return;
            };
            self.line = line;
            let mut ctx = Ctx { vars: &self.vars, io: &mut *io, now, requests: &mut requests };
            let result = match &step {
                Step::Let(v, e) => ctx.eval(e).map(|x| Some((v.clone(), x))).map(|set| (set, None)),
                Step::Write(pin, e) => ctx.eval(e).and_then(|x| x.int()).and_then(|lvl| ctx.write(pin, lvl)).map(|_| (None, None)),
                Step::Print(parts) => {
                    let mut text = String::new();
                    let mut err = None;
                    for (i, e) in parts.iter().enumerate() {
                        match ctx.eval(e) {
                            Ok(v) => {
                                if i > 0 {
                                    text.push(' ');
                                }
                                text.push_str(&v.show());
                            }
                            Err(e) => {
                                err = Some(e);
                                break;
                            }
                        }
                    }
                    match err {
                        Some(e) => Err(e),
                        None => Ok((None, Some(text))),
                    }
                }
                Step::Sleep(e) => match ctx.eval(e).and_then(|x| x.int()) {
                    Ok(secs) => {
                        let secs = u64::try_from(secs.clamp(1, MAX_SLEEP)).unwrap_or(1);
                        self.pc += 1;
                        self.state = RunState::Waiting(now.saturating_add(secs.saturating_mul(1_000_000)));
                        return;
                    }
                    Err(e) => Err(e),
                },
                Step::Report(e) => ctx.eval(e).and_then(|x| x.int()).and_then(|v| ctx.report(v)).map(|_| (None, None)),
                Step::Stop => {
                    self.note("stopped by `stop`".into());
                    self.state = RunState::Stopped(None);
                    return;
                }
                Step::Unless(c, to) => match ctx.eval(c) {
                    Ok(v) => {
                        self.pc = if v.truthy() { self.pc + 1 } else { *to };
                        continue;
                    }
                    Err(e) => Err(e),
                },
                Step::Jump(to) => {
                    self.pc = *to;
                    continue;
                }
            };
            match result {
                Ok((set, printed)) => {
                    if let Some((v, x)) = set {
                        if !self.vars.contains_key(&v) && self.vars.len() >= MAX_VARS {
                            self.stop(format!("more than {MAX_VARS} variables"));
                            return;
                        }
                        self.vars.insert(v, x);
                    }
                    if let Some(t) = printed {
                        self.note(t);
                    }
                    self.pc += 1;
                }
                Err(e) => {
                    self.stop(e);
                    return;
                }
            }
        }
        self.stop(format!("{MAX_STEPS} steps without a `sleep`: a loop needs a `sleep` in it"));
    }
}

struct Ctx<'a> {
    vars: &'a BTreeMap<String, Val>,
    io: &'a mut dyn Io,
    now: u64,
    requests: &'a mut usize,
}

impl Ctx<'_> {
    fn pin_ok(&self, pin: &str) -> Result<(), String> {
        if self.io.has_pin(pin) { Ok(()) } else { Err(format!("the board has no pin {pin}")) }
    }

    fn write(&mut self, pin: &str, level: i64) -> Result<(), String> {
        self.pin_ok(pin)?;
        self.io.write(pin, level)
    }

    fn request(&mut self) -> Result<(), String> {
        *self.requests += 1;
        if *self.requests > MAX_REQUESTS {
            return Err(format!("more than {MAX_REQUESTS} requests without a `sleep`"));
        }
        Ok(())
    }

    fn report(&mut self, v: i64) -> Result<(), String> {
        self.request()?;
        self.io.report(v)
    }

    fn eval(&mut self, e: &Expr) -> Result<Val, String> {
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Var(n) => self.vars.get(n).cloned().ok_or_else(|| format!("`{n}` has no value yet"))?,
            Expr::Neg(x) => Val::Int(self.eval(x)?.int()?.saturating_neg()),
            Expr::Not(x) => Val::Int(i64::from(!self.eval(x)?.truthy())),
            Expr::Read(pin) => {
                self.pin_ok(pin)?;
                Val::Int(self.io.read(pin)?)
            }
            Expr::Get(path) => {
                let path = self.eval(path)?.show();
                self.request()?;
                let body = self.io.get(&path)?;
                Val::Text(body.lines().next().unwrap_or_default().chars().take(MAX_TEXT).collect())
            }
            Expr::Time => Val::Int(i64::try_from(self.now / 1_000_000).unwrap_or(i64::MAX)),
            Expr::Number(x) => {
                let v = self.eval(x)?;
                match v {
                    Val::Int(n) => Val::Int(n),
                    Val::Text(t) => Val::Int(t.trim().parse::<i64>().unwrap_or(0)),
                }
            }
            Expr::Bin(op, a, b) => {
                if *op == "and" {
                    return Ok(Val::Int(i64::from(self.eval(a)?.truthy() && self.eval(b)?.truthy())));
                }
                if *op == "or" {
                    return Ok(Val::Int(i64::from(self.eval(a)?.truthy() || self.eval(b)?.truthy())));
                }
                let (x, y) = (self.eval(a)?, self.eval(b)?);
                match (*op, &x, &y) {
                    ("+", Val::Text(_), _) | ("+", _, Val::Text(_)) => {
                        Val::Text(format!("{}{}", x.show(), y.show()).chars().take(MAX_TEXT).collect())
                    }
                    ("==", _, _) => Val::Int(i64::from(x == y)),
                    ("!=", _, _) => Val::Int(i64::from(x != y)),
                    _ => {
                        let (p, q) = (x.int()?, y.int()?);
                        Val::Int(match *op {
                            "+" => p.saturating_add(q),
                            "-" => p.saturating_sub(q),
                            "*" => p.saturating_mul(q),
                            "/" => p.checked_div(q).ok_or("division by zero")?,
                            "%" => p.checked_rem(q).ok_or("division by zero")?,
                            "<" => i64::from(p < q),
                            ">" => i64::from(p > q),
                            "<=" => i64::from(p <= q),
                            ">=" => i64::from(p >= q),
                            _ => return Err(format!("unknown operator {op}")),
                        })
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        pins: BTreeMap<String, i64>,
        gets: Vec<String>,
        reports: Vec<i64>,
    }

    impl Io for Fake {
        fn read(&mut self, pin: &str) -> Result<i64, String> {
            Ok(self.pins.get(pin).copied().unwrap_or(0))
        }
        fn write(&mut self, pin: &str, level: i64) -> Result<(), String> {
            self.pins.insert(pin.into(), level);
            Ok(())
        }
        fn get(&mut self, path: &str) -> Result<String, String> {
            self.gets.push(path.into());
            Ok("on\nmore".into())
        }
        fn report(&mut self, value: i64) -> Result<(), String> {
            self.reports.push(value);
            Ok(())
        }
        fn has_pin(&self, pin: &str) -> bool {
            matches!(pin, "D0" | "D1" | "A0")
        }
    }

    fn run(src: &str, io: &mut Fake) -> Machine {
        let mut m = Machine::new(compile(src).unwrap(), 0);
        m.run(io, 0);
        m
    }

    #[test]
    fn a_thermostat_loop_reads_writes_and_sleeps() {
        let src = "# a heater\nlet limit = 20\nt = read(A0)\nif t < limit\n  write(D0, 1)\nelif t > limit + 5\n  write(D0, 0)\n  print \"warm\", t\nelse\n  print \"ok\"\nend\nsleep 2\n";
        let mut io = Fake::default();
        io.pins.insert("A0".into(), 15);
        let mut m = run(src, &mut io);
        assert_eq!(io.pins.get("D0"), Some(&1));
        assert_eq!(m.state, RunState::Waiting(2_000_000));
        // Not due yet: nothing happens.
        io.pins.insert("A0".into(), 30);
        m.run(&mut io, 1_000_000);
        assert_eq!(io.pins.get("D0"), Some(&1));
        // After the sleep it reaches the end and starts over a second later.
        m.run(&mut io, 2_000_000);
        assert_eq!(m.state, RunState::Waiting(3_000_000));
        m.run(&mut io, 3_000_000);
        assert_eq!(io.pins.get("D0"), Some(&0));
        assert_eq!(m.log.last().map(String::as_str), Some("warm 30"));
        assert_eq!(m.vars.get("limit"), Some(&Val::Int(20)));
    }

    #[test]
    fn while_loops_count_and_a_loop_without_sleep_is_stopped() {
        let mut io = Fake::default();
        let m = run("x = 0\nwhile x < 10\n x = x + 1\nend\nprint x\nstop\n", &mut io);
        assert_eq!((m.vars.get("x"), m.state.clone()), (Some(&Val::Int(10)), RunState::Stopped(None)));
        let m = run("while 1\n x = 1\nend\n", &mut io);
        let RunState::Stopped(Some(why)) = &m.state else { panic!("{:?}", m.state) };
        assert!(why.contains("without a `sleep`"), "{why}");
        // With a sleep inside, it pauses instead.
        let m = run("n = 0\nwhile true\n n = n + 1\n sleep 1\nend\n", &mut io);
        assert_eq!(m.state, RunState::Waiting(1_000_000));
    }

    #[test]
    fn text_http_and_reports() {
        let mut io = Fake::default();
        let m = run("s = get(\"/iot/state?name=\" + \"Fan1\")\nif s == \"on\"\n report 7\nend\nn = number(\"42\") + 1\nstop\n", &mut io);
        assert_eq!(io.gets, ["/iot/state?name=Fan1"]);
        assert_eq!(io.reports, [7]);
        assert_eq!(m.vars.get("n"), Some(&Val::Int(43)));
        // Too many requests between sleeps.
        let m = run("while 1\n s = get(\"/x\")\nend\n", &mut io);
        assert!(matches!(&m.state, RunState::Stopped(Some(w)) if w.contains("requests")), "{:?}", m.state);
    }

    #[test]
    fn mistakes_are_reported_with_their_line() {
        for (src, want) in [
            ("if 1\nwrite(D0, 1)\n", "without their `end`"),
            ("end\n", "line 1"),
            ("x 5\n", "line 1"),
            ("write(D0 1)\n", "`,` expected"),
            ("print \"open\n", "closing"),
            ("y = 99999999999999999999\n", "too large"),
            ("let if = 2\n", "word of the language"),
            ("x = 1 2\n", "more than one statement"),
            ("x = foo(1)\n", "no function"),
            ("else\n", "without its `if`"),
            ("if 1\nelse\nelse\nend\n", "after the `else`"),
            ("x = @\n", "not part of the language"),
        ] {
            let e = compile(src).unwrap_err();
            assert!(e.contains(want), "{src:?}: {e}");
        }
        assert!(compile(&"x = 1\n".repeat(MAX_LINES + 1)).is_err());
        assert!(compile(&"if 1\n".repeat(MAX_DEPTH + 1)).is_err());
        // Run-time mistakes stop the program with the line.
        let mut io = Fake::default();
        for (src, want) in
            [("x = 1 / 0\n", "division by zero"), ("x = y\n", "has no value"), ("write(D9, 1)\n", "no pin D9"), ("x = \"a\" * 2\n", "is text")]
        {
            let m = run(src, &mut io);
            assert!(matches!(&m.state, RunState::Stopped(Some(w)) if w.contains(want) && w.starts_with("line 1")), "{src}: {:?}", m.state);
        }
    }

    #[test]
    fn hostile_programs_never_hang_or_panic() {
        let mut io = Fake::default();
        for src in [
            "",
            "\n\n#only comments\n",
            "x = -9223372036854775807 - 10\ny = x * x\nz = -x\nstop\n",
            "x = 5 % 0\n",
            "while 1\nend\n",
            "if\n",
            "while\nend\n",
            "x = (((((1\n",
            "print\n",
            "sleep -5\n",
            "sleep 999999999\n",
            "a = \"x\"\nwhile 1\n a = a + a\nend\n",
            "x = ) + (\n",
            "write(, 1)\n",
            "report \"t\"\n",
        ] {
            if let Ok(p) = compile(src) {
                let mut m = Machine::new(p, 0);
                for t in 0..5 {
                    m.run(&mut io, t * 10_000_000_000);
                }
                assert!(m.log.len() <= MAX_LOG);
            }
        }
        // Many variables.
        let src: String = (0..MAX_VARS + 5).map(|i| format!("v{i} = {i}\n")).collect();
        let m = run(&src, &mut io);
        assert!(matches!(&m.state, RunState::Stopped(Some(w)) if w.contains("variables")));
    }
}
