//! Python and JavaScript for programmable boards: a small, bounded interpreter for a documented
//! subset of each language, written for the simulator (see `docs/board-languages.md`). The two
//! front ends (`lang_parse`) produce one syntax tree, which [`compile`] turns into a flat list of
//! operations a [`Vm`] runs. Because the machine keeps its own stack and frames, a program can
//! pause at `time.sleep` / `delay` and carry on when the simulated time comes, like a BoardScript
//! program does.
//!
//! Safety: no file, no network but an HTTP GET to the board's IoT server, no clock but the
//! simulated one. Every run slice executes at most [`MAX_STEPS`] operations, calls nest at most
//! [`MAX_FRAMES`] deep, the stack, the lists and the text are capped, whole numbers are checked
//! (an overflow is an error, not a wrap), and nothing here panics.
//!
//! Whatever the language, a program that defines a `loop` function (Arduino style) has it called
//! again and again, 10 ms of simulated time apart, after the top-level code has run (and after
//! `setup`, when there is one); a program without `loop` runs once and stops.

use std::collections::BTreeMap;

use netlabcraft_model::board::Language;

use crate::script::Io;

/// Most operations one run slice executes before the program is stopped.
pub const MAX_STEPS: usize = 20_000;
/// Deepest nesting of function calls.
pub const MAX_FRAMES: usize = 24;
const MAX_STACK: usize = 256;
/// Most lists alive at once, and most values in all of them.
const MAX_LISTS: usize = 128;
const MAX_CELLS: usize = 2_000;
/// The longest text value (characters), the most variables per scope and the most functions.
pub const MAX_STR: usize = 400;
const MAX_LOCALS: usize = 64;
pub const MAX_FUNCS: usize = 64;
/// Most operations in a compiled program.
const MAX_OPS: usize = 6_000;
/// Most HTTP requests and reports in one slice.
const MAX_REQUESTS: usize = 8;
/// The shortest and the longest pause (microseconds): a millisecond, an hour.
const MIN_SLEEP_US: u64 = 1_000;
const MAX_SLEEP_US: u64 = 3_600_000_000;
/// Simulated time between two calls of `loop` (microseconds).
const LOOP_GAP_US: u64 = 10_000;

type R<T> = Result<T, String>;

/// A value.
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    /// `None` / `undefined` (and `null`: JavaScript's two are one here).
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// A list, by its place in the machine's list store.
    List(usize),
    /// `range(start, stop, step)`, not stored: a loop over it costs no memory.
    Range(i64, i64, i64),
    /// A function (a `def`, a `function`, an arrow or a `lambda`), by its number.
    Func(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bop {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    In,
    NotIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uop {
    Neg,
    Pos,
    Not,
}

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(V),
    Name(String),
    List(Vec<Expr>),
    Un(Uop, Box<Expr>),
    Bin(Bop, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    /// `condition`, `then`, `else`.
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Attr(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// A function value, by its number in [`Ast::funcs`].
    Func(usize),
    /// An f-string or a template literal: the text of each part joined.
    Fmt(Vec<Expr>),
}

/// What an assignment writes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Name(String),
    Index(Expr, Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Expr(Expr),
    /// `declare`: Python's assignment and JavaScript's `let`/`const`/`var` make the name local to
    /// the function; a plain JavaScript assignment changes the name where it already lives.
    Set {
        target: Target,
        value: Expr,
        declare: bool,
    },
    If(Vec<(Expr, Vec<Stmt>)>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    ForIn(String, Expr, Vec<Stmt>),
    /// `for (init; condition; step) body`.
    ForC(Vec<Stmt>, Option<Expr>, Vec<Stmt>, Vec<Stmt>),
    Def(String, usize),
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    Global(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub line: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// A parsed program.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub body: Vec<Stmt>,
    pub funcs: Vec<FuncDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Store {
    Local,
    Assign,
    Global,
}

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Const(V),
    Load(String),
    Store(String, Store),
    StoreIndex,
    List(usize),
    Index,
    Len,
    ToIter,
    ToStr,
    Concat(usize),
    Bin(Bop),
    Un(Uop),
    Attr(String),
    Jump(usize),
    JumpFalse(usize),
    JumpFalseKeep(usize),
    JumpTrueKeep(usize),
    CallName(String, usize),
    CallMethod(String, usize),
    CallValue(usize),
    Pop,
    Return,
    End,
}

#[derive(Debug, Clone, PartialEq)]
struct FuncInfo {
    name: String,
    params: Vec<String>,
    entry: usize,
}

struct LoopCtx {
    breaks: Vec<usize>,
    conts: Vec<usize>,
}

struct Comp {
    code: Vec<(Op, usize)>,
    loops: Vec<LoopCtx>,
    declared_global: Vec<String>,
    in_func: bool,
    tmp: usize,
}

/// The names that stand for a module: `time.sleep(1)`, `console.log(x)`, `Math.floor(x)`.
fn is_module(n: &str) -> bool {
    matches!(n, "time" | "math" | "console" | "Math" | "Date" | "Number")
}

impl Comp {
    fn emit(&mut self, op: Op, line: usize) -> usize {
        self.code.push((op, line));
        self.code.len() - 1
    }

    fn here(&self) -> usize {
        self.code.len()
    }

    fn patch(&mut self, at: usize, to: usize) {
        if let Some((Op::Jump(t) | Op::JumpFalse(t) | Op::JumpFalseKeep(t) | Op::JumpTrueKeep(t), _)) = self.code.get_mut(at) {
            *t = to;
        }
    }

    fn check(&self, line: usize) -> R<()> {
        if self.code.len() > MAX_OPS {
            return Err(format!("line {line}: the program is too long to run"));
        }
        Ok(())
    }

    fn block(&mut self, body: &[Stmt]) -> R<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        let line = s.line;
        self.check(line)?;
        match &s.kind {
            Kind::Expr(e) => {
                self.expr(e, line)?;
                self.emit(Op::Pop, line);
            }
            Kind::Set { target, value, declare } => match target {
                Target::Name(n) => {
                    self.expr(value, line)?;
                    let how = if self.declared_global.contains(n) {
                        Store::Global
                    } else if *declare {
                        Store::Local
                    } else {
                        Store::Assign
                    };
                    self.emit(Op::Store(n.clone(), how), line);
                }
                Target::Index(o, i) => {
                    self.expr(o, line)?;
                    self.expr(i, line)?;
                    self.expr(value, line)?;
                    self.emit(Op::StoreIndex, line);
                }
            },
            Kind::If(arms, other) => {
                let mut ends = Vec::new();
                for (cond, body) in arms {
                    self.expr(cond, line)?;
                    let skip = self.emit(Op::JumpFalse(0), line);
                    self.block(body)?;
                    ends.push(self.emit(Op::Jump(0), line));
                    let here = self.here();
                    self.patch(skip, here);
                }
                self.block(other)?;
                let here = self.here();
                for e in ends {
                    self.patch(e, here);
                }
            }
            Kind::While(cond, body) => {
                let start = self.here();
                self.expr(cond, line)?;
                let out = self.emit(Op::JumpFalse(0), line);
                self.loops.push(LoopCtx { breaks: Vec::new(), conts: Vec::new() });
                self.block(body)?;
                self.emit(Op::Jump(start), line);
                let end = self.here();
                self.patch(out, end);
                self.close_loop(end, start);
            }
            Kind::ForIn(var, iter, body) => {
                self.tmp += 1;
                let (it, ix) = (format!("$it{}", self.tmp), format!("$ix{}", self.tmp));
                self.expr(iter, line)?;
                self.emit(Op::ToIter, line);
                self.emit(Op::Store(it.clone(), Store::Local), line);
                self.emit(Op::Const(V::Int(0)), line);
                self.emit(Op::Store(ix.clone(), Store::Local), line);
                let start = self.here();
                self.emit(Op::Load(ix.clone()), line);
                self.emit(Op::Load(it.clone()), line);
                self.emit(Op::Len, line);
                self.emit(Op::Bin(Bop::Lt), line);
                let out = self.emit(Op::JumpFalse(0), line);
                self.emit(Op::Load(it), line);
                self.emit(Op::Load(ix.clone()), line);
                self.emit(Op::Index, line);
                let how = if self.declared_global.contains(var) { Store::Global } else { Store::Local };
                self.emit(Op::Store(var.clone(), how), line);
                self.loops.push(LoopCtx { breaks: Vec::new(), conts: Vec::new() });
                self.block(body)?;
                let step = self.here();
                self.emit(Op::Load(ix.clone()), line);
                self.emit(Op::Const(V::Int(1)), line);
                self.emit(Op::Bin(Bop::Add), line);
                self.emit(Op::Store(ix, Store::Local), line);
                self.emit(Op::Jump(start), line);
                let end = self.here();
                self.patch(out, end);
                self.close_loop(end, step);
            }
            Kind::ForC(init, cond, step, body) => {
                self.block(init)?;
                let start = self.here();
                let out = match cond {
                    Some(c) => {
                        self.expr(c, line)?;
                        Some(self.emit(Op::JumpFalse(0), line))
                    }
                    None => None,
                };
                self.loops.push(LoopCtx { breaks: Vec::new(), conts: Vec::new() });
                self.block(body)?;
                let at_step = self.here();
                self.block(step)?;
                self.emit(Op::Jump(start), line);
                let end = self.here();
                if let Some(o) = out {
                    self.patch(o, end);
                }
                self.close_loop(end, at_step);
            }
            Kind::Def(name, idx) => {
                self.emit(Op::Const(V::Func(*idx)), line);
                let how = if self.declared_global.contains(name) { Store::Global } else { Store::Local };
                self.emit(Op::Store(name.clone(), how), line);
            }
            Kind::Return(e) => {
                if !self.in_func {
                    return Err(format!("line {line}: `return` outside a function"));
                }
                match e {
                    Some(e) => self.expr(e, line)?,
                    None => {
                        self.emit(Op::Const(V::None), line);
                    }
                }
                self.emit(Op::Return, line);
            }
            Kind::Break => {
                let at = self.emit(Op::Jump(0), line);
                match self.loops.last_mut() {
                    Some(l) => l.breaks.push(at),
                    None => return Err(format!("line {line}: `break` outside a loop")),
                }
            }
            Kind::Continue => {
                let at = self.emit(Op::Jump(0), line);
                match self.loops.last_mut() {
                    Some(l) => l.conts.push(at),
                    None => return Err(format!("line {line}: `continue` outside a loop")),
                }
            }
            Kind::Pass => {}
            Kind::Global(names) => {
                for n in names {
                    if !self.declared_global.contains(n) {
                        self.declared_global.push(n.clone());
                    }
                }
            }
        }
        Ok(())
    }

    fn close_loop(&mut self, end: usize, cont: usize) {
        if let Some(l) = self.loops.pop() {
            for b in l.breaks {
                self.patch(b, end);
            }
            for c in l.conts {
                self.patch(c, cont);
            }
        }
    }

    fn expr(&mut self, e: &Expr, line: usize) -> R<()> {
        self.check(line)?;
        match e {
            Expr::Lit(v) => {
                self.emit(Op::Const(v.clone()), line);
            }
            Expr::Name(n) => {
                self.emit(Op::Load(n.clone()), line);
            }
            Expr::List(items) => {
                for i in items {
                    self.expr(i, line)?;
                }
                self.emit(Op::List(items.len()), line);
            }
            Expr::Un(op, x) => {
                self.expr(x, line)?;
                self.emit(Op::Un(*op), line);
            }
            Expr::Bin(op, a, b) => {
                self.expr(a, line)?;
                self.expr(b, line)?;
                self.emit(Op::Bin(*op), line);
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                self.expr(a, line)?;
                let and = matches!(e, Expr::And(..));
                let j = self.emit(if and { Op::JumpFalseKeep(0) } else { Op::JumpTrueKeep(0) }, line);
                self.expr(b, line)?;
                let here = self.here();
                self.patch(j, here);
            }
            Expr::Cond(c, a, b) => {
                self.expr(c, line)?;
                let to_else = self.emit(Op::JumpFalse(0), line);
                self.expr(a, line)?;
                let to_end = self.emit(Op::Jump(0), line);
                let here = self.here();
                self.patch(to_else, here);
                self.expr(b, line)?;
                let here = self.here();
                self.patch(to_end, here);
            }
            Expr::Call(callee, args) => match &**callee {
                Expr::Name(n) => {
                    for a in args {
                        self.expr(a, line)?;
                    }
                    self.emit(Op::CallName(n.clone(), args.len()), line);
                }
                Expr::Attr(obj, m) => {
                    if let Expr::Name(module) = &**obj
                        && is_module(module)
                    {
                        for a in args {
                            self.expr(a, line)?;
                        }
                        self.emit(Op::CallName(format!("{module}.{m}"), args.len()), line);
                    } else {
                        self.expr(obj, line)?;
                        for a in args {
                            self.expr(a, line)?;
                        }
                        self.emit(Op::CallMethod(m.clone(), args.len()), line);
                    }
                }
                other => {
                    self.expr(other, line)?;
                    for a in args {
                        self.expr(a, line)?;
                    }
                    self.emit(Op::CallValue(args.len()), line);
                }
            },
            Expr::Attr(obj, a) => {
                if let Expr::Name(module) = &**obj
                    && is_module(module)
                {
                    let v = match (module.as_str(), a.as_str()) {
                        ("math" | "Math", "pi" | "PI") => V::Float(std::f64::consts::PI),
                        ("math" | "Math", "e" | "E") => V::Float(std::f64::consts::E),
                        ("math", "inf") => V::Float(f64::INFINITY),
                        _ => return Err(format!("line {line}: `{module}.{a}` is not available")),
                    };
                    self.emit(Op::Const(v), line);
                } else {
                    self.expr(obj, line)?;
                    self.emit(Op::Attr(a.clone()), line);
                }
            }
            Expr::Index(o, i) => {
                self.expr(o, line)?;
                self.expr(i, line)?;
                self.emit(Op::Index, line);
            }
            Expr::Func(i) => {
                self.emit(Op::Const(V::Func(*i)), line);
            }
            Expr::Fmt(parts) => {
                for p in parts {
                    self.expr(p, line)?;
                    self.emit(Op::ToStr, line);
                }
                self.emit(Op::Concat(parts.len()), line);
            }
        }
        Ok(())
    }
}

/// Parses and compiles a program in `lang` (Python or JavaScript); an error names its line.
pub fn compile(lang: Language, source: &str) -> R<Vm> {
    if source.chars().count() > netlabcraft_model::board::MAX_PROGRAM {
        return Err(format!("the program is longer than {} characters", netlabcraft_model::board::MAX_PROGRAM));
    }
    let py = match lang {
        Language::Python => true,
        Language::JavaScript => false,
        Language::BoardScript => return Err("BoardScript has its own compiler".into()),
    };
    let ast = crate::lang_parse::parse(source, py)?;
    if ast.funcs.len() > MAX_FUNCS {
        return Err(format!("more than {MAX_FUNCS} functions"));
    }
    let mut c = Comp { code: Vec::new(), loops: Vec::new(), declared_global: Vec::new(), in_func: false, tmp: 0 };
    c.block(&ast.body)?;
    c.emit(Op::End, 0);
    let mut funcs = Vec::new();
    for (i, f) in ast.funcs.iter().enumerate() {
        let entry = c.here();
        funcs.push(FuncInfo { name: f.name.clone(), params: f.params.clone(), entry });
        c.in_func = true;
        c.declared_global.clear();
        c.loops.clear();
        let body = ast.funcs.get(i).map(|f| f.body.clone()).unwrap_or_default();
        c.block(&body)?;
        c.emit(Op::Const(V::None), f.line);
        c.emit(Op::Return, f.line);
        c.in_func = false;
    }
    let find = |name: &str| funcs.iter().position(|f| f.name == name);
    let (loop_fn, setup_fn) = (find("loop"), find("setup"));
    let frames = vec![Frame { locals: BTreeMap::new(), ret: 0 }];
    Ok(Vm {
        py,
        code: c.code,
        funcs,
        loop_fn,
        setup_fn,
        pc: 0,
        stack: Vec::new(),
        frames,
        lists: Vec::new(),
        want_gc: false,
        pending_loop: false,
        setup_done: false,
        line: 1,
        steps: 0,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct Frame {
    locals: BTreeMap<String, V>,
    ret: usize,
}

/// What a run slice ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Paused: runs again after this many microseconds of simulated time.
    Sleep(u64),
    /// The program ended.
    End,
    /// Stopped by an error on this line.
    Error(usize, String),
}

enum Flow {
    Next,
    Sleep(u64),
    End,
}

/// A program running: its stack, its frames and its lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Vm {
    py: bool,
    code: Vec<(Op, usize)>,
    funcs: Vec<FuncInfo>,
    loop_fn: Option<usize>,
    setup_fn: Option<usize>,
    pc: usize,
    stack: Vec<V>,
    frames: Vec<Frame>,
    lists: Vec<Option<Vec<V>>>,
    want_gc: bool,
    pending_loop: bool,
    setup_done: bool,
    /// The source line of the operation that ran last (from 1).
    pub line: usize,
    /// Operations executed since it started.
    pub steps: u64,
}

impl Eq for Vm {}

fn cmp_simple(a: &V, b: &V) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (V::Str(x), V::Str(y)) => Some(x.cmp(y)),
        _ => {
            let (x, y) = (as_f64(a)?, as_f64(b)?);
            x.partial_cmp(&y)
        }
    }
}

fn as_f64(v: &V) -> Option<f64> {
    match v {
        V::Int(n) => Some(*n as f64),
        V::Float(f) => Some(*f),
        V::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// A number: whole or fractional.
#[derive(Clone, Copy)]
enum N {
    I(i64),
    F(f64),
}

impl N {
    fn f(self) -> f64 {
        match self {
            N::I(i) => i as f64,
            N::F(f) => f,
        }
    }
}

fn range_len(a: i64, b: i64, c: i64) -> i64 {
    if c == 0 {
        return 0;
    }
    let span = i128::from(b) - i128::from(a);
    let step = i128::from(c);
    let n = if (c > 0 && span > 0) || (c < 0 && span < 0) { (span.abs() + step.abs() - 1) / step.abs() } else { 0 };
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl Vm {
    fn type_name(v: &V) -> &'static str {
        match v {
            V::None => "None",
            V::Bool(_) => "bool",
            V::Int(_) => "int",
            V::Float(_) => "float",
            V::Str(_) => "str",
            V::List(_) => "list",
            V::Range(..) => "range",
            V::Func(_) => "function",
        }
    }

    fn name_error(&self, n: &str) -> String {
        if self.py { format!("NameError: name '{n}' is not defined") } else { format!("ReferenceError: {n} is not defined") }
    }

    fn push(&mut self, v: V) -> R<()> {
        if self.stack.len() >= MAX_STACK {
            return Err("the expression is too deep for the board's stack".into());
        }
        self.stack.push(v);
        Ok(())
    }

    fn pop(&mut self) -> R<V> {
        self.stack.pop().ok_or_else(|| "internal error: the stack is empty".to_string())
    }

    fn take(&mut self, n: usize) -> R<Vec<V>> {
        let at = self.stack.len().checked_sub(n).ok_or("internal error: the stack is empty")?;
        Ok(self.stack.split_off(at))
    }

    // ---- lists ----

    fn cells(&self) -> usize {
        self.lists.iter().flatten().map(Vec::len).sum()
    }

    fn live(&self) -> usize {
        self.lists.iter().flatten().count()
    }

    fn pressure(&mut self) {
        if self.live() * 4 >= MAX_LISTS * 3 || self.cells() * 4 >= MAX_CELLS * 3 {
            self.want_gc = true;
        }
    }

    fn alloc(&mut self, items: Vec<V>) -> R<V> {
        if self.cells().saturating_add(items.len()) > MAX_CELLS {
            return Err(format!("MemoryError: lists hold more than {MAX_CELLS} values"));
        }
        let slot = match self.lists.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.lists.len() < MAX_LISTS => {
                self.lists.push(None);
                self.lists.len() - 1
            }
            None => return Err(format!("MemoryError: more than {MAX_LISTS} lists alive")),
        };
        if let Some(s) = self.lists.get_mut(slot) {
            *s = Some(items);
        }
        self.pressure();
        Ok(V::List(slot))
    }

    fn list(&self, h: usize) -> R<&Vec<V>> {
        self.lists.get(h).and_then(Option::as_ref).ok_or_else(|| "internal error: a list is gone".to_string())
    }

    fn list_mut(&mut self, h: usize) -> R<&mut Vec<V>> {
        self.lists.get_mut(h).and_then(Option::as_mut).ok_or_else(|| "internal error: a list is gone".to_string())
    }

    fn grow_ok(&self, extra: usize) -> R<()> {
        if self.cells().saturating_add(extra) > MAX_CELLS {
            return Err(format!("MemoryError: lists hold more than {MAX_CELLS} values"));
        }
        Ok(())
    }

    /// Frees the lists nothing can reach.
    fn gc(&mut self) {
        self.want_gc = false;
        let mut seen = vec![false; self.lists.len()];
        let mut todo: Vec<usize> = Vec::new();
        for v in self.stack.iter().chain(self.frames.iter().flat_map(|f| f.locals.values())) {
            if let V::List(h) = v {
                todo.push(*h);
            }
        }
        while let Some(h) = todo.pop() {
            match seen.get_mut(h) {
                Some(s) if !*s => *s = true,
                _ => continue,
            }
            if let Some(Some(items)) = self.lists.get(h) {
                for v in items {
                    if let V::List(k) = v {
                        todo.push(*k);
                    }
                }
            }
        }
        for (i, l) in self.lists.iter_mut().enumerate() {
            if !seen.get(i).copied().unwrap_or(false) {
                *l = None;
            }
        }
    }

    // ---- display and truth ----

    fn fmt_float(&self, f: f64) -> String {
        if f.is_nan() {
            return if self.py { "nan" } else { "NaN" }.to_string();
        }
        if f.is_infinite() {
            return match (self.py, f > 0.0) {
                (true, true) => "inf",
                (true, false) => "-inf",
                (false, true) => "Infinity",
                (false, false) => "-Infinity",
            }
            .to_string();
        }
        let s = format!("{f}");
        if self.py && !s.contains('.') { format!("{s}.0") } else { s }
    }

    fn show(&self, v: &V, repr: bool, depth: usize) -> String {
        match v {
            V::None => if self.py { "None" } else { "undefined" }.to_string(),
            V::Bool(b) => match (self.py, *b) {
                (true, true) => "True",
                (true, false) => "False",
                (false, true) => "true",
                (false, false) => "false",
            }
            .to_string(),
            V::Int(n) => n.to_string(),
            V::Float(f) => self.fmt_float(*f),
            V::Str(s) => {
                if repr {
                    format!("'{s}'")
                } else {
                    s.clone()
                }
            }
            V::List(h) => {
                if depth >= 3 {
                    return "[...]".to_string();
                }
                let Some(items) = self.lists.get(*h).and_then(Option::as_ref) else { return "[]".to_string() };
                let parts: Vec<String> = items.iter().take(40).map(|x| self.show(x, true, depth + 1)).collect();
                let more = if items.len() > 40 { ", ..." } else { "" };
                format!("[{}{more}]", parts.join(", "))
            }
            V::Range(a, b, c) => {
                if *c == 1 {
                    format!("range({a}, {b})")
                } else {
                    format!("range({a}, {b}, {c})")
                }
            }
            V::Func(i) => format!("<function {}>", self.funcs.get(*i).map_or("?", |f| f.name.as_str())),
        }
    }

    fn truthy(&self, v: &V) -> bool {
        match v {
            V::None => false,
            V::Bool(b) => *b,
            V::Int(n) => *n != 0,
            V::Float(f) => *f != 0.0 && !f.is_nan(),
            V::Str(s) => !s.is_empty(),
            V::List(h) => !self.py || self.lists.get(*h).and_then(Option::as_ref).is_some_and(|l| !l.is_empty()),
            V::Range(a, b, c) => range_len(*a, *b, *c) > 0,
            V::Func(_) => true,
        }
    }

    fn num(&self, v: &V) -> R<N> {
        match v {
            V::Int(n) => Ok(N::I(*n)),
            V::Float(f) => Ok(N::F(*f)),
            V::Bool(b) => Ok(N::I(i64::from(*b))),
            other => Err(format!("TypeError: {} is not a number", Self::type_name(other))),
        }
    }

    /// JavaScript has one kind of number: a whole-valued float is a whole number.
    fn norm(&self, v: V) -> V {
        match v {
            V::Float(f) if !self.py && f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => V::Int(f as i64),
            other => other,
        }
    }

    fn values_eq(&self, a: &V, b: &V) -> bool {
        self.values_eq_at(a, b, 0)
    }

    /// Equality; lists that hold themselves are compared only so deep.
    fn values_eq_at(&self, a: &V, b: &V, depth: usize) -> bool {
        match (a, b) {
            (V::List(x), V::List(y)) => {
                if x == y {
                    return true;
                }
                if !self.py || depth > 8 {
                    return false;
                }
                match (self.lists.get(*x).and_then(Option::as_ref), self.lists.get(*y).and_then(Option::as_ref)) {
                    (Some(p), Some(q)) => p.len() == q.len() && p.iter().zip(q).all(|(m, n)| self.values_eq_at(m, n, depth + 1)),
                    _ => false,
                }
            }
            (V::Str(x), V::Str(y)) => x == y,
            (V::None, V::None) => true,
            (V::Func(x), V::Func(y)) => x == y,
            _ => match (as_f64(a), as_f64(b)) {
                (Some(x), Some(y)) => match (a, b) {
                    (V::Int(p), V::Int(q)) => p == q,
                    _ => x == y,
                },
                _ => false,
            },
        }
    }

    fn contains(&self, hay: &V, needle: &V) -> R<bool> {
        match hay {
            V::Str(s) => match needle {
                V::Str(n) => Ok(s.contains(n.as_str())),
                _ => Err("TypeError: 'in <string>' needs text on the left".into()),
            },
            V::List(h) => Ok(self.list(*h)?.iter().any(|x| self.values_eq(x, needle))),
            V::Range(a, b, c) => Ok(match needle {
                V::Int(n) => (0..range_len(*a, *b, *c)).any(|i| a.saturating_add(i.saturating_mul(*c)) == *n),
                _ => false,
            }),
            other => Err(format!("TypeError: argument of type '{}' is not iterable", Self::type_name(other))),
        }
    }

    fn binop(&mut self, op: Bop, x: V, y: V) -> R<V> {
        use std::cmp::Ordering::{Greater, Less};
        match op {
            Bop::Eq => return Ok(V::Bool(self.values_eq(&x, &y))),
            Bop::Ne => return Ok(V::Bool(!self.values_eq(&x, &y))),
            Bop::In => return Ok(V::Bool(self.contains(&y, &x)?)),
            Bop::NotIn => return Ok(V::Bool(!self.contains(&y, &x)?)),
            Bop::Lt | Bop::Gt | Bop::Le | Bop::Ge => {
                let Some(o) = cmp_simple(&x, &y) else {
                    if matches!((&x, &y), (V::Float(_), _) | (_, V::Float(_))) && as_f64(&x).is_some() && as_f64(&y).is_some() {
                        return Ok(V::Bool(false));
                    }
                    return Err(format!("TypeError: can't compare {} and {}", Self::type_name(&x), Self::type_name(&y)));
                };
                return Ok(V::Bool(match op {
                    Bop::Lt => o == Less,
                    Bop::Gt => o == Greater,
                    Bop::Le => o != Greater,
                    _ => o != Less,
                }));
            }
            _ => {}
        }
        // Text and lists.
        match (op, &x, &y) {
            (Bop::Add, V::Str(a), V::Str(b)) => return self.text(format!("{a}{b}")),
            (Bop::Add, V::Str(_), _) | (Bop::Add, _, V::Str(_)) if !self.py => {
                let s = format!("{}{}", self.show(&x, false, 0), self.show(&y, false, 0));
                return self.text(s);
            }
            (Bop::Add, V::Str(_), _) | (Bop::Add, _, V::Str(_)) => {
                return Err("TypeError: can only join text to text: use str(...) around the number".into());
            }
            (Bop::Add, V::List(a), V::List(b)) => {
                let mut items = self.list(*a)?.clone();
                items.extend(self.list(*b)?.iter().cloned());
                return self.alloc(items);
            }
            (Bop::Mul, V::Str(s), V::Int(n)) | (Bop::Mul, V::Int(n), V::Str(s)) => {
                let n = usize::try_from((*n).max(0)).unwrap_or(0);
                if s.chars().count().saturating_mul(n) > MAX_STR {
                    return Err(format!("the text would be longer than {MAX_STR} characters"));
                }
                return Ok(V::Str(s.repeat(n)));
            }
            (Bop::Mul, V::List(h), V::Int(n)) | (Bop::Mul, V::Int(n), V::List(h)) => {
                let base = self.list(*h)?.clone();
                let n = usize::try_from((*n).max(0)).unwrap_or(0);
                if base.len().saturating_mul(n) > MAX_CELLS {
                    return Err(format!("MemoryError: lists hold more than {MAX_CELLS} values"));
                }
                let mut items = Vec::new();
                for _ in 0..n {
                    items.extend(base.iter().cloned());
                }
                return self.alloc(items);
            }
            _ => {}
        }
        let (a, b) = (self.num(&x)?, self.num(&y)?);
        let overflow = || "OverflowError: the number is too large".to_string();
        let r = match op {
            Bop::Add | Bop::Sub | Bop::Mul => match (a, b) {
                (N::I(p), N::I(q)) => V::Int(
                    match op {
                        Bop::Add => p.checked_add(q),
                        Bop::Sub => p.checked_sub(q),
                        _ => p.checked_mul(q),
                    }
                    .ok_or_else(overflow)?,
                ),
                _ => {
                    let (p, q) = (a.f(), b.f());
                    V::Float(match op {
                        Bop::Add => p + q,
                        Bop::Sub => p - q,
                        _ => p * q,
                    })
                }
            },
            Bop::Div => {
                let (p, q) = (a.f(), b.f());
                if q == 0.0 && self.py {
                    return Err("ZeroDivisionError: division by zero".into());
                }
                V::Float(p / q)
            }
            Bop::FloorDiv => match (a, b) {
                (N::I(p), N::I(q)) => {
                    if q == 0 {
                        return Err("ZeroDivisionError: division by zero".into());
                    }
                    let d = p.checked_div(q).ok_or_else(overflow)?;
                    let r = p.checked_rem(q).ok_or_else(overflow)?;
                    V::Int(if r != 0 && ((r < 0) != (q < 0)) { d - 1 } else { d })
                }
                _ => {
                    let q = b.f();
                    if q == 0.0 {
                        return Err("ZeroDivisionError: division by zero".into());
                    }
                    V::Float((a.f() / q).floor())
                }
            },
            Bop::Mod => match (a, b) {
                (N::I(p), N::I(q)) => {
                    if q == 0 {
                        if self.py {
                            return Err("ZeroDivisionError: modulo by zero".into());
                        }
                        V::Float(f64::NAN)
                    } else {
                        let r = p.checked_rem(q).ok_or_else(overflow)?;
                        V::Int(if self.py && r != 0 && ((r < 0) != (q < 0)) { r + q } else { r })
                    }
                }
                _ => {
                    let (p, q) = (a.f(), b.f());
                    if q == 0.0 && self.py {
                        return Err("ZeroDivisionError: modulo by zero".into());
                    }
                    let r = p % q;
                    V::Float(if self.py && r != 0.0 && ((r < 0.0) != (q < 0.0)) { r + q } else { r })
                }
            },
            Bop::Pow => match (a, b) {
                (N::I(p), N::I(q)) if q >= 0 => V::Int(u32::try_from(q).ok().and_then(|e| p.checked_pow(e)).ok_or_else(overflow)?),
                _ => V::Float(a.f().powf(b.f())),
            },
            _ => return Err("internal error: unknown operator".into()),
        };
        if let V::Float(f) = r
            && self.py
            && f.is_infinite()
        {
            return Err("OverflowError: the number is too large".into());
        }
        Ok(self.norm(r))
    }

    fn text(&self, s: String) -> R<V> {
        if s.chars().count() > MAX_STR {
            return Err(format!("the text would be longer than {MAX_STR} characters"));
        }
        Ok(V::Str(s))
    }

    fn len_of(&self, v: &V) -> R<i64> {
        match v {
            V::Str(s) => Ok(i64::try_from(s.chars().count()).unwrap_or(i64::MAX)),
            V::List(h) => Ok(i64::try_from(self.list(*h)?.len()).unwrap_or(i64::MAX)),
            V::Range(a, b, c) => Ok(range_len(*a, *b, *c)),
            other => Err(format!("TypeError: {} has no length", Self::type_name(other))),
        }
    }

    fn index(&self, o: &V, i: &V) -> R<V> {
        let i = match i {
            V::Int(n) => *n,
            V::Float(f) if !self.py && f.fract() == 0.0 => *f as i64,
            V::Bool(b) => i64::from(*b),
            other => return Err(format!("TypeError: an index is a whole number, not {}", Self::type_name(other))),
        };
        let len = self.len_of(o)?;
        let at = if i < 0 && self.py { i.checked_add(len).unwrap_or(-1) } else { i };
        if at < 0 || at >= len {
            if self.py {
                return Err("IndexError: index out of range".into());
            }
            return Ok(V::None);
        }
        let at_u = usize::try_from(at).unwrap_or(0);
        Ok(match o {
            V::Str(s) => V::Str(s.chars().nth(at_u).map(String::from).unwrap_or_default()),
            V::List(h) => self.list(*h)?.get(at_u).cloned().unwrap_or(V::None),
            V::Range(a, _, c) => V::Int(a.saturating_add(at.saturating_mul(*c))),
            _ => V::None,
        })
    }

    fn store_index(&mut self, o: V, i: V, v: V) -> R<()> {
        let V::List(h) = o else { return Err(format!("TypeError: {} does not support item assignment", Self::type_name(&o))) };
        let V::Int(i) = i else { return Err("TypeError: an index is a whole number".into()) };
        let len = i64::try_from(self.list(h)?.len()).unwrap_or(i64::MAX);
        let at = if i < 0 && self.py { i.checked_add(len).unwrap_or(-1) } else { i };
        if at == len && !self.py {
            self.grow_ok(1)?;
            self.list_mut(h)?.push(v);
            self.pressure();
            return Ok(());
        }
        if at < 0 || at >= len {
            return Err("IndexError: list assignment index out of range".into());
        }
        let at = usize::try_from(at).unwrap_or(0);
        if let Some(slot) = self.list_mut(h)?.get_mut(at) {
            *slot = v;
        }
        Ok(())
    }

    // ---- names ----

    fn load(&self, n: &str) -> R<V> {
        if let Some(v) = self.frames.last().and_then(|f| f.locals.get(n)) {
            return Ok(v.clone());
        }
        if let Some(v) = self.frames.first().and_then(|f| f.locals.get(n)) {
            return Ok(v.clone());
        }
        match n {
            "HIGH" => Ok(V::Int(1)),
            "LOW" => Ok(V::Int(0)),
            "Infinity" => Ok(V::Float(f64::INFINITY)),
            "NaN" => Ok(V::Float(f64::NAN)),
            "undefined" => Ok(V::None),
            _ => Err(self.name_error(n)),
        }
    }

    fn store(&mut self, n: &str, v: V, how: Store) -> R<()> {
        let last_has = self.frames.last().is_some_and(|f| f.locals.contains_key(n));
        let first_has = self.frames.first().is_some_and(|f| f.locals.contains_key(n));
        let to_first = match how {
            Store::Global => true,
            Store::Local => false,
            Store::Assign => !last_has && first_has,
        };
        let frame = if to_first { self.frames.first_mut() } else { self.frames.last_mut() };
        let Some(frame) = frame else { return Err("internal error: no frame".into()) };
        if !frame.locals.contains_key(n) && frame.locals.len() >= MAX_LOCALS {
            return Err(format!("more than {MAX_LOCALS} variables"));
        }
        frame.locals.insert(n.to_string(), v);
        self.pressure();
        Ok(())
    }

    // ---- calls ----

    fn call_user(&mut self, i: usize, args: Vec<V>) -> R<()> {
        let Some(f) = self.funcs.get(i) else { return Err("internal error: no such function".into()) };
        let (entry, params, name) = (f.entry, f.params.clone(), f.name.clone());
        if self.frames.len() >= MAX_FRAMES {
            return Err(format!("RecursionError: calls nested more than {MAX_FRAMES} deep"));
        }
        if self.py && args.len() != params.len() {
            return Err(format!("TypeError: {name}() takes {} arguments but {} were given", params.len(), args.len()));
        }
        let mut locals = BTreeMap::new();
        for (k, p) in params.iter().enumerate() {
            locals.insert(p.clone(), args.get(k).cloned().unwrap_or(V::None));
        }
        self.frames.push(Frame { locals, ret: self.pc });
        self.pc = entry;
        Ok(())
    }

    fn pin_of(&self, v: &V, prefix: char) -> R<String> {
        match v {
            V::Int(n) if *n >= 0 => Ok(format!("{prefix}{n}")),
            V::Str(s) => {
                let p = s.trim().to_ascii_uppercase();
                if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() { Ok(format!("{prefix}{p}")) } else { Ok(p) }
            }
            _ => Err("TypeError: a pin is a number (3) or a name (\"D3\", \"A0\")".into()),
        }
    }

    fn int_arg(&self, v: &V) -> R<i64> {
        match v {
            V::Int(n) => Ok(*n),
            V::Bool(b) => Ok(i64::from(*b)),
            V::Float(f) if f.is_finite() => Ok(f.clamp(-9.0e18, 9.0e18) as i64),
            other => Err(format!("TypeError: {} is not a whole number", Self::type_name(other))),
        }
    }

    fn arity(&self, name: &str, args: &[V], lo: usize, hi: usize) -> R<()> {
        if args.len() < lo || args.len() > hi {
            return Err(if lo == hi {
                format!("TypeError: {name}() takes {lo} argument(s), {} given", args.len())
            } else {
                format!("TypeError: {name}() takes {lo} to {hi} arguments, {} given", args.len())
            });
        }
        Ok(())
    }

    fn call_name(&mut self, name: &str, argc: usize, io: &mut dyn Io, now: u64, out: &mut Vec<String>, requests: &mut usize) -> R<Flow> {
        let args = self.take(argc)?;
        if let Ok(v) = self.load(name) {
            return match v {
                V::Func(i) => {
                    self.call_user(i, args)?;
                    Ok(Flow::Next)
                }
                other => Err(format!("TypeError: '{}' is not a function", Self::type_name(&other))),
            };
        }
        self.builtin(name, args, io, now, out, requests)
    }

    fn done(&mut self, v: V) -> R<Flow> {
        self.push(v)?;
        Ok(Flow::Next)
    }

    fn to_float(&self, v: &V, strict: bool) -> R<V> {
        Ok(match v {
            V::Int(n) => V::Float(*n as f64),
            V::Float(f) => V::Float(*f),
            V::Bool(b) => V::Float(f64::from(u8::from(*b))),
            V::Str(s) => match s.trim().parse::<f64>() {
                Ok(f) => V::Float(f),
                Err(_) if strict => return Err(format!("ValueError: could not convert '{s}' to float")),
                Err(_) => V::Float(f64::NAN),
            },
            other => return Err(format!("TypeError: float() can't take {}", Self::type_name(other))),
        })
    }

    fn to_int(&self, v: &V, strict: bool) -> R<V> {
        Ok(match v {
            V::Int(n) => V::Int(*n),
            V::Bool(b) => V::Int(i64::from(*b)),
            V::Float(f) if f.is_finite() => V::Int(f.trunc().clamp(-9.0e18, 9.0e18) as i64),
            V::Float(f) if strict => return Err(format!("ValueError: cannot turn {} into a whole number", self.fmt_float(*f))),
            V::Float(_) => V::Float(f64::NAN),
            V::Str(s) => {
                let t = s.trim();
                if strict {
                    V::Int(t.parse::<i64>().map_err(|_| format!("ValueError: invalid whole number '{s}'"))?)
                } else {
                    // parseInt reads the digits at the start and ignores the rest.
                    let digits: String = t
                        .chars()
                        .enumerate()
                        .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                        .map(|(_, c)| c)
                        .collect();
                    match digits.parse::<i64>() {
                        Ok(n) => V::Int(n),
                        Err(_) => V::Float(f64::NAN),
                    }
                }
            }
            other => return Err(format!("TypeError: int() can't take {}", Self::type_name(other))),
        })
    }

    fn pick(&self, name: &str, args: &[V], want_max: bool) -> R<V> {
        let items: Vec<V> = match args {
            [V::List(h)] => self.list(*h)?.clone(),
            [V::Range(a, b, c)] => (0..range_len(*a, *b, *c).min(1000)).map(|i| V::Int(a.saturating_add(i.saturating_mul(*c)))).collect(),
            [] => return Err(format!("TypeError: {name}() needs something to compare")),
            many => many.to_vec(),
        };
        let mut best: Option<V> = None;
        for v in items {
            best = match best {
                None => Some(v),
                Some(b) => match cmp_simple(&v, &b) {
                    Some(std::cmp::Ordering::Greater) if want_max => Some(v),
                    Some(std::cmp::Ordering::Less) if !want_max => Some(v),
                    Some(_) => Some(b),
                    None => return Err(format!("TypeError: {name}() can't compare these values")),
                },
            };
        }
        best.ok_or_else(|| format!("ValueError: {name}() of an empty list"))
    }

    fn seconds_us(&self, v: &V, per_unit: f64) -> R<u64> {
        let x = self.num(v)?.f();
        if !x.is_finite() {
            return Err("ValueError: a pause is a finite number".into());
        }
        let us = (x * per_unit).clamp(0.0, MAX_SLEEP_US as f64);
        Ok((us as u64).clamp(MIN_SLEEP_US, MAX_SLEEP_US))
    }

    fn request(&self, requests: &mut usize) -> R<()> {
        *requests += 1;
        if *requests > MAX_REQUESTS {
            return Err(format!("more than {MAX_REQUESTS} requests without a pause"));
        }
        Ok(())
    }

    fn builtin(&mut self, name: &str, args: Vec<V>, io: &mut dyn Io, now: u64, out: &mut Vec<String>, requests: &mut usize) -> R<Flow> {
        let a0 = args.first().cloned().unwrap_or(V::None);
        match name {
            "print" | "console.log" | "console.info" | "console.warn" | "console.error" => {
                let line: Vec<String> = args.iter().map(|v| self.show(v, false, 0)).collect();
                out.push(line.join(" ").chars().take(MAX_STR).collect());
                self.done(V::None)
            }
            "len" => {
                self.arity(name, &args, 1, 1)?;
                let n = self.len_of(&a0)?;
                self.done(V::Int(n))
            }
            "str" | "String" => {
                self.arity(name, &args, 0, 1)?;
                let s = self.show(&a0, false, 0);
                let v = self.text(s)?;
                self.done(v)
            }
            "int" | "parseInt" => {
                self.arity(name, &args, 1, 2)?;
                let v = self.to_int(&a0, name == "int")?;
                self.done(v)
            }
            "float" | "parseFloat" | "Number" | "Number.parseFloat" => {
                self.arity(name, &args, 1, 1)?;
                let v = self.to_float(&a0, name == "float")?;
                let v = self.norm(v);
                self.done(v)
            }
            "bool" | "Boolean" => {
                self.arity(name, &args, 0, 1)?;
                let t = self.truthy(&a0);
                self.done(V::Bool(t))
            }
            "abs" | "math.fabs" | "Math.abs" => {
                self.arity(name, &args, 1, 1)?;
                let v = match self.num(&a0)? {
                    N::I(n) => V::Int(n.checked_abs().ok_or("OverflowError: the number is too large")?),
                    N::F(f) => V::Float(f.abs()),
                };
                self.done(v)
            }
            "min" | "Math.min" => {
                let v = self.pick(name, &args, false)?;
                self.done(v)
            }
            "max" | "Math.max" => {
                let v = self.pick(name, &args, true)?;
                self.done(v)
            }
            "sum" => {
                self.arity(name, &args, 1, 1)?;
                let items = match &a0 {
                    V::List(h) => self.list(*h)?.clone(),
                    V::Range(a, b, c) => (0..range_len(*a, *b, *c).min(1000)).map(|i| V::Int(a.saturating_add(i.saturating_mul(*c)))).collect(),
                    _ => return Err("TypeError: sum() needs a list".into()),
                };
                let mut total = V::Int(0);
                for x in items {
                    total = self.binop(Bop::Add, total, x)?;
                }
                self.done(total)
            }
            "range" => {
                self.arity(name, &args, 1, 3)?;
                let ints: Vec<i64> = args.iter().map(|v| self.int_arg(v)).collect::<R<_>>()?;
                let (a, b, c) = match ints.as_slice() {
                    [n] => (0, *n, 1),
                    [a, b] => (*a, *b, 1),
                    [a, b, c] => (*a, *b, *c),
                    _ => (0, 0, 1),
                };
                if c == 0 {
                    return Err("ValueError: range() step must not be 0".into());
                }
                self.done(V::Range(a, b, c))
            }
            "list" => {
                self.arity(name, &args, 0, 1)?;
                let items = match &a0 {
                    V::None => Vec::new(),
                    V::List(h) => self.list(*h)?.clone(),
                    V::Range(a, b, c) => {
                        let n = range_len(*a, *b, *c);
                        if n > i64::try_from(MAX_CELLS).unwrap_or(0) {
                            return Err(format!("MemoryError: lists hold more than {MAX_CELLS} values"));
                        }
                        (0..n).map(|i| V::Int(a.saturating_add(i.saturating_mul(*c)))).collect()
                    }
                    V::Str(s) => s.chars().map(|c| V::Str(c.to_string())).collect(),
                    _ => return Err("TypeError: list() needs a list, a range or text".into()),
                };
                let v = self.alloc(items)?;
                self.done(v)
            }
            "sorted" => {
                self.arity(name, &args, 1, 1)?;
                let mut items = self.iter_items(&a0)?;
                self.sort_items(&mut items)?;
                let v = self.alloc(items)?;
                self.done(v)
            }
            "round" | "Math.round" => {
                self.arity(name, &args, 1, 2)?;
                let x = self.num(&a0)?;
                let digits = match args.get(1) {
                    Some(d) => Some(self.int_arg(d)?.clamp(-9, 9)),
                    None => None,
                };
                let v = match (x, digits) {
                    (N::I(n), _) => V::Int(n),
                    (N::F(f), _) if !f.is_finite() => V::Float(f),
                    (N::F(f), None) if name == "Math.round" => V::Int((f + 0.5).floor().clamp(-9.0e18, 9.0e18) as i64),
                    (N::F(f), None) => V::Int(f.round_ties_even().clamp(-9.0e18, 9.0e18) as i64),
                    (N::F(f), Some(d)) => {
                        let k = 10f64.powi(i32::try_from(d).unwrap_or(0));
                        V::Float((f * k).round_ties_even() / k)
                    }
                };
                self.done(v)
            }
            "math.sqrt" | "Math.sqrt" => {
                self.arity(name, &args, 1, 1)?;
                let x = self.num(&a0)?.f();
                if x < 0.0 && self.py {
                    return Err("ValueError: math domain error".into());
                }
                let v = self.norm(V::Float(x.sqrt()));
                self.done(v)
            }
            "math.floor" | "Math.floor" | "math.ceil" | "Math.ceil" | "Math.trunc" | "math.trunc" => {
                self.arity(name, &args, 1, 1)?;
                let x = self.num(&a0)?;
                let v = match x {
                    N::I(n) => V::Int(n),
                    N::F(f) if !f.is_finite() => V::Float(f),
                    N::F(f) => {
                        let r = if name.ends_with("floor") {
                            f.floor()
                        } else if name.ends_with("ceil") {
                            f.ceil()
                        } else {
                            f.trunc()
                        };
                        V::Int(r.clamp(-9.0e18, 9.0e18) as i64)
                    }
                };
                self.done(v)
            }
            "math.pow" | "Math.pow" => {
                self.arity(name, &args, 2, 2)?;
                let b = args.get(1).cloned().unwrap_or(V::None);
                let v = self.binop(Bop::Pow, a0, b)?;
                self.done(v)
            }
            "time.sleep" | "sleep" => {
                self.arity(name, &args, 1, 1)?;
                let us = self.seconds_us(&a0, 1_000_000.0)?;
                self.push(V::None)?;
                Ok(Flow::Sleep(us))
            }
            "delay" => {
                self.arity(name, &args, 1, 1)?;
                let us = self.seconds_us(&a0, 1_000.0)?;
                self.push(V::None)?;
                Ok(Flow::Sleep(us))
            }
            "time.time" | "time.monotonic" => self.done(V::Float(now as f64 / 1_000_000.0)),
            "millis" | "Date.now" => self.done(V::Int(i64::try_from(now / 1_000).unwrap_or(i64::MAX))),
            "digitalRead" | "analogRead" | "customRead" => {
                self.arity(name, &args, 1, 1)?;
                let pin = self.pin_of(&a0, if name == "analogRead" { 'A' } else { 'D' })?;
                if !io.has_pin(&pin) {
                    return Err(format!("the board has no pin {pin}"));
                }
                let v = io.read(&pin)?;
                self.done(V::Int(v))
            }
            "channel" => {
                self.arity(name, &args, 2, 2)?;
                let pin = self.pin_of(&a0, 'A')?;
                if !io.has_pin(&pin) {
                    return Err(format!("the board has no pin {pin}"));
                }
                let n = args.get(1).map_or(Ok(0), |v| self.int_arg(v))?;
                let v = io.read_channel(&pin, n)?;
                self.done(V::Int(v))
            }
            "digitalWrite" | "analogWrite" | "customWrite" => {
                self.arity(name, &args, 2, 2)?;
                let pin = self.pin_of(&a0, 'D')?;
                if !io.has_pin(&pin) {
                    return Err(format!("the board has no pin {pin}"));
                }
                let level = match args.get(1) {
                    Some(V::Bool(b)) => i64::from(*b),
                    Some(v) => self.int_arg(v)?,
                    None => 0,
                };
                io.write(&pin, level)?;
                self.done(V::None)
            }
            "rgb" => {
                self.arity(name, &args, 3, 3)?;
                let mut c = [0i64; 3];
                for (slot, v) in c.iter_mut().zip(&args) {
                    *slot = self.int_arg(v)?.clamp(0, 255);
                }
                self.done(V::Int(c[0] * 65_536 + c[1] * 256 + c[2]))
            }
            "http_get" | "httpGet" => {
                self.arity(name, &args, 1, 1)?;
                let V::Str(path) = &a0 else { return Err("TypeError: the path is text".into()) };
                self.request(requests)?;
                let body = io.get(path)?;
                let first: String = body.lines().next().unwrap_or_default().chars().take(MAX_STR).collect();
                self.done(V::Str(first))
            }
            "report" => {
                self.arity(name, &args, 1, 1)?;
                self.request(requests)?;
                let v = self.int_arg(&a0)?;
                io.report(v)?;
                self.done(V::None)
            }
            other => Err(self.name_error(other)),
        }
    }

    fn iter_items(&self, v: &V) -> R<Vec<V>> {
        match v {
            V::List(h) => Ok(self.list(*h)?.clone()),
            V::Range(a, b, c) => {
                let n = range_len(*a, *b, *c);
                if n > i64::try_from(MAX_CELLS).unwrap_or(0) {
                    return Err(format!("MemoryError: lists hold more than {MAX_CELLS} values"));
                }
                Ok((0..n).map(|i| V::Int(a.saturating_add(i.saturating_mul(*c)))).collect())
            }
            V::Str(s) => Ok(s.chars().map(|c| V::Str(c.to_string())).collect()),
            other => Err(format!("TypeError: '{}' is not iterable", Self::type_name(other))),
        }
    }

    fn sort_items(&self, items: &mut [V]) -> R<()> {
        let all_text = items.iter().all(|v| matches!(v, V::Str(_)));
        let all_num = items.iter().all(|v| matches!(v, V::Int(_) | V::Float(_) | V::Bool(_)));
        if !all_text && !all_num {
            return Err("TypeError: a list to sort holds only numbers or only text".into());
        }
        items.sort_by(|a, b| cmp_simple(a, b).unwrap_or(std::cmp::Ordering::Equal));
        Ok(())
    }

    fn method(&mut self, m: &str, argc: usize) -> R<Flow> {
        let args = self.take(argc)?;
        let obj = self.pop()?;
        let a0 = args.first().cloned().unwrap_or(V::None);
        match (&obj, m) {
            (V::List(h), "append" | "push") => {
                self.arity(m, &args, 1, 1)?;
                self.grow_ok(1)?;
                self.list_mut(*h)?.push(a0);
                self.pressure();
                let n = self.list(*h)?.len();
                self.done(if m == "push" { V::Int(i64::try_from(n).unwrap_or(0)) } else { V::None })
            }
            (V::List(h), "pop") => {
                self.arity(m, &args, 0, 1)?;
                let len = self.list(*h)?.len();
                let at = match args.first() {
                    Some(i) => {
                        let i = self.int_arg(i)?;
                        let at = if i < 0 { i64::try_from(len).unwrap_or(0).checked_add(i).unwrap_or(-1) } else { i };
                        usize::try_from(at).ok().filter(|a| *a < len)
                    }
                    None => len.checked_sub(1),
                };
                match at {
                    Some(a) => {
                        let v = self.list_mut(*h)?.remove(a);
                        self.done(v)
                    }
                    None if self.py => Err("IndexError: pop from an empty list or out of range".into()),
                    None => self.done(V::None),
                }
            }
            (V::List(h), "insert") => {
                self.arity(m, &args, 2, 2)?;
                self.grow_ok(1)?;
                let i = self.int_arg(&a0)?;
                let len = self.list(*h)?.len();
                let at = usize::try_from(i.max(0)).unwrap_or(0).min(len);
                let v = args.get(1).cloned().unwrap_or(V::None);
                self.list_mut(*h)?.insert(at, v);
                self.pressure();
                self.done(V::None)
            }
            (V::List(h), "reverse") => {
                self.list_mut(*h)?.reverse();
                self.done(V::None)
            }
            (V::List(h), "sort") => {
                let mut items = self.list(*h)?.clone();
                self.sort_items(&mut items)?;
                *self.list_mut(*h)? = items;
                self.done(V::None)
            }
            (V::List(h), "clear") => {
                self.list_mut(*h)?.clear();
                self.done(V::None)
            }
            (V::List(_), "includes") => {
                let r = self.contains(&obj, &a0)?;
                self.done(V::Bool(r))
            }
            (V::List(h), "indexOf") => {
                let at = self.list(*h)?.iter().position(|x| self.values_eq(x, &a0));
                self.done(V::Int(at.map_or(-1, |i| i64::try_from(i).unwrap_or(-1))))
            }
            (V::List(h), "join") => {
                let sep = match &a0 {
                    V::Str(s) => s.clone(),
                    _ => ",".to_string(),
                };
                let parts: Vec<String> = self.list(*h)?.iter().map(|x| self.show(x, false, 0)).collect();
                let v = self.text(parts.join(sep.as_str()))?;
                self.done(v)
            }
            (V::Str(s), "upper" | "toUpperCase") => self.done(V::Str(s.to_uppercase())),
            (V::Str(s), "lower" | "toLowerCase") => self.done(V::Str(s.to_lowercase())),
            (V::Str(s), "strip" | "trim") => self.done(V::Str(s.trim().to_string())),
            (V::Str(s), "startswith" | "startsWith") => {
                let V::Str(p) = &a0 else { return Err("TypeError: startswith needs text".into()) };
                self.done(V::Bool(s.starts_with(p.as_str())))
            }
            (V::Str(s), "endswith" | "endsWith") => {
                let V::Str(p) = &a0 else { return Err("TypeError: endswith needs text".into()) };
                self.done(V::Bool(s.ends_with(p.as_str())))
            }
            (V::Str(s), "includes") => {
                let V::Str(p) = &a0 else { return Err("TypeError: includes needs text".into()) };
                self.done(V::Bool(s.contains(p.as_str())))
            }
            (V::Str(s), "find" | "indexOf") => {
                let V::Str(p) = &a0 else { return Err("TypeError: find needs text".into()) };
                let at = s.find(p.as_str()).map(|b| s.get(..b).map_or(0, |x| x.chars().count()));
                self.done(V::Int(at.map_or(-1, |i| i64::try_from(i).unwrap_or(-1))))
            }
            (V::Str(s), "replace") => {
                self.arity(m, &args, 2, 2)?;
                let (V::Str(from), Some(V::Str(to))) = (&a0, args.get(1)) else { return Err("TypeError: replace needs two texts".into()) };
                if from.is_empty() {
                    return self.done(V::Str(s.clone()));
                }
                let v = self.text(s.replace(from.as_str(), to))?;
                self.done(v)
            }
            (V::Str(s), "split") => {
                let parts: Vec<V> = match &a0 {
                    V::Str(sep) if !sep.is_empty() => s.split(sep.as_str()).take(MAX_CELLS).map(|x| V::Str(x.to_string())).collect(),
                    _ => s.split_whitespace().take(MAX_CELLS).map(|x| V::Str(x.to_string())).collect(),
                };
                let v = self.alloc(parts)?;
                self.done(v)
            }
            (V::Str(sep), "join") => {
                let items = self.iter_items(&a0)?;
                let mut parts = Vec::new();
                for x in &items {
                    let V::Str(t) = x else { return Err("TypeError: join needs a list of text".into()) };
                    parts.push(t.clone());
                }
                let v = self.text(parts.join(sep.as_str()))?;
                self.done(v)
            }
            (V::Int(_) | V::Float(_), "toFixed") => {
                let digits = usize::try_from(self.int_arg(&a0)?.clamp(0, 20)).unwrap_or(0);
                let x = self.num(&obj)?.f();
                self.done(V::Str(format!("{x:.digits$}")))
            }
            (_, "toString") => {
                let v = self.text(self.show(&obj, false, 0))?;
                self.done(v)
            }
            _ => Err(format!("AttributeError: {} has no method {m}", Self::type_name(&obj))),
        }
    }

    fn step(&mut self, op: Op, io: &mut dyn Io, now: u64, out: &mut Vec<String>, requests: &mut usize) -> R<Flow> {
        self.pc += 1;
        match op {
            Op::Const(v) => self.push(v)?,
            Op::Load(n) => {
                let v = self.load(&n)?;
                self.push(v)?;
            }
            Op::Store(n, how) => {
                let v = self.pop()?;
                self.store(&n, v, how)?;
            }
            Op::StoreIndex => {
                let v = self.pop()?;
                let i = self.pop()?;
                let o = self.pop()?;
                self.store_index(o, i, v)?;
            }
            Op::List(n) => {
                let items = self.take(n)?;
                let l = self.alloc(items)?;
                self.push(l)?;
            }
            Op::Index => {
                let i = self.pop()?;
                let o = self.pop()?;
                let v = self.index(&o, &i)?;
                self.push(v)?;
            }
            Op::Len => {
                let v = self.pop()?;
                let n = self.len_of(&v)?;
                self.push(V::Int(n))?;
            }
            Op::ToIter => {
                let v = self.pop()?;
                if !matches!(v, V::List(_) | V::Range(..) | V::Str(_)) {
                    return Err(format!("TypeError: '{}' is not iterable", Self::type_name(&v)));
                }
                self.push(v)?;
            }
            Op::ToStr => {
                let v = self.pop()?;
                let s = self.show(&v, false, 0);
                self.push(V::Str(s))?;
            }
            Op::Concat(n) => {
                let parts = self.take(n)?;
                let mut s = String::new();
                for p in parts {
                    if let V::Str(t) = p {
                        s.push_str(&t);
                    }
                }
                let v = self.text(s)?;
                self.push(v)?;
            }
            Op::Bin(b) => {
                let y = self.pop()?;
                let x = self.pop()?;
                let r = self.binop(b, x, y)?;
                self.push(r)?;
            }
            Op::Un(u) => {
                let x = self.pop()?;
                let r = match u {
                    Uop::Not => V::Bool(!self.truthy(&x)),
                    Uop::Pos => match self.num(&x)? {
                        N::I(n) => V::Int(n),
                        N::F(f) => V::Float(f),
                    },
                    Uop::Neg => match self.num(&x)? {
                        N::I(n) => V::Int(n.checked_neg().ok_or("OverflowError: the number is too large")?),
                        N::F(f) => V::Float(-f),
                    },
                };
                self.push(r)?;
            }
            Op::Attr(a) => {
                let o = self.pop()?;
                let v = match (&o, a.as_str()) {
                    (V::Str(_) | V::List(_), "length") if !self.py => V::Int(self.len_of(&o)?),
                    _ => return Err(format!("AttributeError: {} has no attribute {a}", Self::type_name(&o))),
                };
                self.push(v)?;
            }
            Op::Jump(t) => self.pc = t,
            Op::JumpFalse(t) => {
                let v = self.pop()?;
                if !self.truthy(&v) {
                    self.pc = t;
                }
            }
            Op::JumpFalseKeep(t) => {
                let v = self.pop()?;
                if !self.truthy(&v) {
                    self.push(v)?;
                    self.pc = t;
                }
            }
            Op::JumpTrueKeep(t) => {
                let v = self.pop()?;
                if self.truthy(&v) {
                    self.push(v)?;
                    self.pc = t;
                }
            }
            Op::Pop => {
                self.pop()?;
            }
            Op::CallName(n, argc) => return self.call_name(&n, argc, io, now, out, requests),
            Op::CallMethod(m, argc) => return self.method(&m, argc),
            Op::CallValue(argc) => {
                let args = self.take(argc)?;
                match self.pop()? {
                    V::Func(i) => self.call_user(i, args)?,
                    other => return Err(format!("TypeError: '{}' is not a function", Self::type_name(&other))),
                }
            }
            Op::Return => {
                if self.frames.len() <= 1 {
                    return Err("internal error: return outside a function".into());
                }
                let v = self.pop()?;
                let f = self.frames.pop().ok_or("internal error: no frame")?;
                self.pc = f.ret;
                self.push(v)?;
            }
            Op::End => return self.end(),
        }
        Ok(Flow::Next)
    }

    /// The end of the top-level code: `setup` and `loop`, when the program has them.
    fn end(&mut self) -> R<Flow> {
        self.stack.clear();
        let at = self.pc.saturating_sub(1);
        let Some(lf) = self.loop_fn else { return Ok(Flow::End) };
        self.pc = at;
        if !self.pending_loop {
            self.pending_loop = true;
            return Ok(Flow::Sleep(LOOP_GAP_US));
        }
        self.pending_loop = false;
        let f = match self.setup_fn {
            Some(s) if !self.setup_done => {
                self.setup_done = true;
                s
            }
            _ => lf,
        };
        self.call_user(f, Vec::new())?;
        Ok(Flow::Next)
    }

    /// Runs from where it is until a pause, the end, an error or the step cap. Whatever the
    /// program printed is added to `out`.
    pub fn run(&mut self, io: &mut dyn Io, now: u64, out: &mut Vec<String>) -> Out {
        let mut requests = 0usize;
        for _ in 0..MAX_STEPS {
            if self.want_gc {
                self.gc();
            }
            let Some((op, line)) = self.code.get(self.pc).cloned() else { return Out::End };
            if line > 0 {
                self.line = line;
            }
            self.steps = self.steps.saturating_add(1);
            match self.step(op, io, now, out, &mut requests) {
                Ok(Flow::Next) => {}
                Ok(Flow::Sleep(us)) => return Out::Sleep(us),
                Ok(Flow::End) => return Out::End,
                Err(e) => return Out::Error(self.line, e),
            }
        }
        Out::Error(self.line, format!("{MAX_STEPS} steps without a pause: put a sleep or a delay in the loop"))
    }

    /// The top-level variables, as text, for the Programming tab.
    pub fn variables(&self) -> Vec<(String, String)> {
        let Some(f) = self.frames.first() else { return Vec::new() };
        f.locals
            .iter()
            .filter(|(k, v)| !k.starts_with('$') && !matches!(v, V::Func(_)))
            .take(64)
            .map(|(k, v)| (k.clone(), self.show(v, true, 0)))
            .collect()
    }

    /// Whether the variable is a whole number (so the tab shows a number, not text).
    pub fn is_whole(&self, name: &str) -> Option<i64> {
        match self.frames.first().and_then(|f| f.locals.get(name)) {
            Some(V::Int(n)) => Some(*n),
            _ => None,
        }
    }
}
