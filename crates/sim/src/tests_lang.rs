//! The Python and JavaScript subsets boards run: what they compute, how they pause, the board
//! functions they reach, and that hostile programs end with an error instead of a hang or a panic.

use std::collections::BTreeMap;

use netlabcraft_model::board::Language;

use crate::lang::{Out, Vm, compile};
use crate::script::Io;

#[derive(Default)]
struct Fake {
    pins: BTreeMap<String, i64>,
    written: Vec<(String, i64)>,
    reports: Vec<i64>,
}

impl Io for Fake {
    fn read(&mut self, pin: &str) -> Result<i64, String> {
        Ok(self.pins.get(pin).copied().unwrap_or(0))
    }
    fn read_channel(&mut self, pin: &str, channel: i64) -> Result<i64, String> {
        Ok(self.pins.get(pin).copied().unwrap_or(0) + channel * 1000)
    }
    fn write(&mut self, pin: &str, level: i64) -> Result<(), String> {
        self.written.push((pin.to_string(), level));
        Ok(())
    }
    fn get(&mut self, path: &str) -> Result<String, String> {
        Ok(format!("answer {path}\nsecond line"))
    }
    fn report(&mut self, value: i64) -> Result<(), String> {
        self.reports.push(value);
        Ok(())
    }
    fn has_pin(&self, pin: &str) -> bool {
        ["D0", "D1", "D2", "D3", "A0", "A1"].contains(&pin)
    }
}

/// Runs a program until it ends, errors, or has paused `max_sleeps` times; returns its output and
/// how it ended.
fn run(lang: Language, src: &str, io: &mut Fake, max_sleeps: usize) -> (Vec<String>, Out) {
    let mut vm: Vm = compile(lang, src).unwrap_or_else(|e| panic!("does not compile: {e}\n{src}"));
    let mut out = Vec::new();
    let mut now = 0u64;
    let mut sleeps = 0;
    loop {
        match vm.run(io, now, &mut out) {
            Out::Sleep(us) => {
                sleeps += 1;
                now += us;
                if sleeps >= max_sleeps {
                    return (out, Out::Sleep(us));
                }
            }
            other => return (out, other),
        }
    }
}

fn py(src: &str) -> Vec<String> {
    let (out, end) = run(Language::Python, src, &mut Fake::default(), 100);
    assert_eq!(end, Out::End, "{src}");
    out
}

fn js(src: &str) -> Vec<String> {
    let (out, end) = run(Language::JavaScript, src, &mut Fake::default(), 100);
    assert_eq!(end, Out::End, "{src}");
    out
}

fn py_error(src: &str) -> (usize, String) {
    match run(Language::Python, src, &mut Fake::default(), 100).1 {
        Out::Error(line, why) => (line, why),
        other => panic!("expected an error, got {other:?} for {src}"),
    }
}

fn js_error(src: &str) -> (usize, String) {
    match run(Language::JavaScript, src, &mut Fake::default(), 100).1 {
        Out::Error(line, why) => (line, why),
        other => panic!("expected an error, got {other:?} for {src}"),
    }
}

#[test]
fn python_arithmetic_text_and_lists() {
    assert_eq!(py("x = 7\nprint(x // 2, x % 3, x / 2, 2 ** 10, -7 // 2, -7 % 3)"), ["3 1 3.5 1024 -4 2"]);
    assert_eq!(py("print(1 + 2 * 3, (1 + 2) * 3, 10 - 4 - 3, 2 ** 3 ** 2, 7 % -3)"), ["7 9 3 512 -2"]);
    assert_eq!(py("print(3 < 4, 3 == 3.0, 'a' < 'b', not 0, 1 and 2, 0 or 'x', None)"), ["True True True True 2 x None"]);
    assert_eq!(py("n = ['a', 'b']\nn.append('c')\nprint(len(n), n, f'n={len(n)} {3.14159:.2f} {{x}}')"), ["3 ['a', 'b', 'c'] n=3 3.14 {x}"]);
    assert_eq!(
        py("s = 'Hello'\nprint(s.upper(), s.lower(), s[1], s[-1], len(s), s + '!', s * 2, 'ell' in s)"),
        ["HELLO hello e o 5 Hello! HelloHello True"]
    );
    assert_eq!(
        py(
            "print(int('42') + 1, float('1.5') * 2, str(7) + 'x', int(3.9), round(2.5), round(3.5), round(3.14159, 2), abs(-4), min(3, 1, 2), max([4, 9, 2]), sum([1, 2, 3]))"
        ),
        ["43 3.0 7x 3 2 4 3.14 4 1 9 6"]
    );
    assert_eq!(
        py("a = [3, 1, 2]\na.sort()\nb = a + [9]\nprint(a, b, sorted([5, 4]), a[0], 'x,y'.split(','), '-'.join(['a', 'b']))"),
        ["[1, 2, 3] [1, 2, 3, 9] [4, 5] 1 ['x', 'y'] a-b"]
    );
    assert_eq!(py("a = [1, 2, 3]\na[1] = 20\na[-1] += 5\nprint(a, a.pop(), a)"), ["[1, 20] 8 [1, 20]"]);
}

#[test]
fn python_control_flow_functions_and_scopes() {
    let src = "def fact(n):\n    if n <= 1:\n        return 1\n    return n * fact(n - 1)\ntotal = 0\nfor i in range(5):\n    if i == 3:\n        continue\n    total += i\nprint(fact(5), total)\n";
    assert_eq!(py(src), ["120 7"]);
    assert_eq!(py("i = 0\nwhile True:\n    i += 1\n    if i > 4:\n        break\nprint(i)"), ["5"]);
    assert_eq!(py("for x in [1, 2]:\n    for y in 'ab':\n        print(x, y)"), ["1 a", "1 b", "2 a", "2 b"]);
    assert_eq!(py("for i in range(10, 0, -4):\n    print(i)"), ["10", "6", "2"]);
    assert_eq!(py("x = 5\nif x > 9: print('big')\nelif x > 3: print('mid')\nelse: print('small')"), ["mid"]);
    assert_eq!(py("n = 1\ndef bump():\n    global n\n    n = n + 1\nbump()\nbump()\nprint(n)"), ["3"]);
    assert_eq!(py("n = 1\ndef shadow():\n    n = 50\n    return n\nprint(shadow(), n)"), ["50 1"]);
    assert_eq!(py("f = lambda a, b: a * b\nprint(f(6, 7), 'y' if 1 else 'n')"), ["42 y"]);
    assert_eq!(py("import time\nfrom math import floor\nimport math\nprint(math.floor(2.7), math.sqrt(16), math.pi > 3)"), ["2 4.0 True"]);
    assert_eq!(py("# only a comment\n\n\nprint('ok')   # trailing\n"), ["ok"]);
}

#[test]
fn javascript_arithmetic_functions_and_scopes() {
    let src = "function fact(n) { return n <= 1 ? 1 : n * fact(n - 1); }\nlet t = 0;\nfor (let i = 0; i < 5; i++) { if (i === 3) continue; t += i; }\nconst sq = x => x * x;\nconsole.log(fact(5), t, sq(4), 7 / 2, 6 / 2, `v=${t}`);";
    assert_eq!(js(src), ["120 7 16 3.5 3 v=7"]);
    assert_eq!(js("console.log(1 + '2', '3' + 4, 7 % -3, 2 ** 10, !0, 1 && 2, 0 || 'x', null, 10 / 4)"), ["12 34 1 1024 true 2 x undefined 2.5"]);
    assert_eq!(
        js("const a = [1, 2]; a.push(3); a[0] = 9; console.log(a, a.length, a.indexOf(3), a.includes(7), a.join('-'))"),
        ["[9, 2, 3] 3 2 false 9-2-3"]
    );
    assert_eq!(
        js(
            "let s = 'abc'; console.log(s.length, s.toUpperCase(), s[1], s[9], parseInt('12px'), Number('x'), Math.floor(2.7), Math.max(1, 5), Math.round(2.5))"
        ),
        ["3 ABC b undefined 12 NaN 2 5 3"]
    );
    assert_eq!(js("let n = 0; while (n < 3) n++; do_nothing(); function do_nothing() { n += 10 } console.log(n)"), ["13"]);
    assert_eq!(js("for (const x of [1, 2, 3]) { if (x === 2) break; console.log(x); }"), ["1"]);
    assert_eq!(js("let c = 0; const add = (a, b) => { c += a + b; return c; };\nconsole.log(add(1, 2), add(3, 4), c)"), ["3 10 10"]);
    assert_eq!(
        js(
            "/* block\ncomment */ let x = 5 // line\nif (x > 3) { console.log('big') } else if (x > 1) { console.log('mid') } else { console.log('small') }"
        ),
        ["big"]
    );
    assert_eq!(js("var i = 0; for (;;) { i += 2; if (i >= 6) break; } console.log(i)"), ["6"]);
}

#[test]
fn programs_pause_in_simulated_time() {
    let mut vm = compile(Language::Python, "import time\nprint('a')\ntime.sleep(2)\nprint('b')\ndelay(500)\nprint('c')").unwrap();
    let (mut io, mut out) = (Fake::default(), Vec::new());
    assert_eq!(vm.run(&mut io, 0, &mut out), Out::Sleep(2_000_000));
    assert_eq!(out, ["a"]);
    assert_eq!(vm.run(&mut io, 2_000_000, &mut out), Out::Sleep(500_000));
    assert_eq!(vm.run(&mut io, 2_500_000, &mut out), Out::End);
    assert_eq!(out, ["a", "b", "c"]);
    // A pause is at least a millisecond and at most an hour.
    let mut vm = compile(Language::JavaScript, "delay(0); delay(1e12);").unwrap();
    assert_eq!(vm.run(&mut io, 0, &mut out), Out::Sleep(1_000));
    assert_eq!(vm.run(&mut io, 0, &mut out), Out::Sleep(3_600_000_000));
}

#[test]
fn a_loop_function_runs_again_and_again_after_setup() {
    let src = "let n = 0;\nfunction setup() { console.log('setup'); }\nfunction loop() { n++; console.log('tick', n); delay(100); }";
    let (out, end) = run(Language::JavaScript, src, &mut Fake::default(), 8);
    assert!(matches!(end, Out::Sleep(_)), "never ends");
    assert_eq!(&out[..4], ["setup", "tick 1", "tick 2", "tick 3"], "{out:?}");
    let (out, _) = run(Language::Python, "n = 0\ndef loop():\n    global n\n    n += 1\n    print(n)\n    time.sleep(1)\n", &mut Fake::default(), 7);
    assert_eq!(out, ["1", "2", "3"]);
}

#[test]
fn the_board_functions_reach_the_pins() {
    let mut io = Fake::default();
    io.pins.insert("A0".into(), 31);
    io.pins.insert("D1".into(), 1);
    let src = "t = analogRead('A0')\nprint(t, digitalRead(1), customRead('d1'), channel('A0', 1), rgb(255, 0, 16))\ndigitalWrite('D0', 1)\nanalogWrite(2, 128)\ncustomWrite('D3', True)\nprint(http_get('/x'))\nreport(t)";
    let (out, end) = run(Language::Python, src, &mut io, 5);
    assert_eq!(end, Out::End);
    assert_eq!(out, ["31 1 1 1031 16711696", "answer /x"]);
    assert_eq!(io.written, [("D0".to_string(), 1), ("D2".to_string(), 128), ("D3".to_string(), 1)]);
    assert_eq!(io.reports, [31]);
    let (out, _) = run(Language::JavaScript, "digitalWrite(HIGH, LOW); console.log(analogRead(0), httpGet('/y'))", &mut io, 5);
    assert_eq!(out, ["31 answer /y"]);
    assert_eq!(io.written.last(), Some(&("D1".to_string(), 0)));
    assert!(py_error("digitalRead('D9')").1.contains("no pin D9"));
    assert!(py_error("digitalWrite(1.5, 1)").1.contains("pin"));
}

#[test]
fn errors_name_their_line() {
    assert_eq!(py_error("x = 1\nprint(y)").0, 2);
    assert!(py_error("x = 1\nprint(y)").1.contains("NameError"));
    assert_eq!(py_error("a = [1]\n\nprint(a[5])").0, 3);
    assert!(py_error("print(1 / 0)").1.contains("ZeroDivisionError"));
    assert!(py_error("print('a' + 1)").1.contains("TypeError"));
    assert!(py_error("def f(a):\n    return a\nf(1, 2)").1.contains("takes 1"));
    assert!(py_error("print(int('x'))").1.contains("ValueError"));
    assert_eq!(js_error("let a = 1;\nconsole.log(b);").0, 2);
    assert!(js_error("console.log(b)").1.contains("ReferenceError"));
    assert!(js_error("let a = 1; a();").1.contains("not a function"));
    // Syntax errors come with the compiler.
    for (lang, src, line) in [
        (Language::Python, "x = 1\nclass A:\n    pass", 2),
        (Language::Python, "if x\n    pass", 1),
        (Language::Python, "def f(a=1):\n    pass", 1),
        (Language::Python, "print(x=1)", 1),
        (Language::Python, "x = 1\n  y = 2", 2),
        (Language::Python, "import os", 1),
        (Language::Python, "return 1", 1),
        (Language::JavaScript, "let x = 1;\nswitch (x) {}", 2),
        (Language::JavaScript, "let x = ;", 1),
        (Language::JavaScript, "function f( { }", 1),
        (Language::JavaScript, "x = 'never closed", 1),
        (Language::JavaScript, "if (1) {", 1),
        (Language::JavaScript, "a & b", 1),
    ] {
        let e = compile(lang, src).err().unwrap_or_else(|| panic!("should not compile: {src}"));
        assert!(e.starts_with(&format!("line {line}:")), "{src}: {e}");
    }
}

#[test]
fn hostile_programs_end_with_an_error() {
    assert!(py_error("while True:\n    pass").1.contains("steps without a pause"));
    assert!(js_error("while (true) {}").1.contains("steps without a pause"));
    assert!(py_error("def f():\n    return f()\nf()").1.contains("RecursionError"));
    assert!(py_error("a = [1]\nwhile True:\n    a.append(1)").1.contains("MemoryError"));
    assert!(py_error("s = 'x'\nwhile True:\n    s = s + s").1.contains("longer than"));
    assert!(py_error("x = 2 ** 62\nprint(x * x)").1.contains("too large"));
    assert!(py_error("print(2 ** 100)").1.contains("too large"));
    assert!(py_error("print('x' * 100000)").1.contains("longer than"));
    assert!(py_error("print([0] * 100000)").1.contains("MemoryError"));
    assert!(py_error("for i in range(10):\n    a = [[1], [2], [3]] * 700").1.contains("MemoryError"));
    assert!(py_error("print(list(range(100000)))").1.contains("MemoryError"));
    assert!(py_error("for i in range(0, 5, 0):\n    pass").1.contains("step"));
    assert!(py_error("print(-(-9223372036854775807 - 1))").1.contains("too large"));
    // Many short-lived lists are collected, so a loop can build them for a long time.
    let (out, end) = run(Language::Python, "n = 0\nfor i in range(600):\n    x = [i, i]\n    n += len(x)\nprint(n)", &mut Fake::default(), 1);
    assert_eq!((out, end), (vec!["1200".to_string()], Out::End));
    // Variables, requests and deep nesting.
    let many: String = (0..70).map(|i| format!("v{i} = {i}\n")).collect();
    assert!(py_error(&many).1.contains("variables"));
    let reqs = "for i in range(20):\n    http_get('/a')";
    assert!(py_error(reqs).1.contains("requests"));
    let deep = format!("print({}1{})", "(".repeat(2000), ")".repeat(2000));
    assert!(compile(Language::Python, &deep).is_err());
    let deep = format!("let x = {}1{};", "[".repeat(2000), "]".repeat(2000));
    assert!(compile(Language::JavaScript, &deep).is_err());
    let neg = format!("print({}1)", "-".repeat(5000));
    assert!(compile(Language::Python, &neg).is_err());
    let nots = format!("print({}1)", "not ".repeat(5000));
    assert!(compile(Language::Python, &nots).is_err());
    let blocks: String = (0..60).map(|i| format!("{}if 1:\n", "  ".repeat(i))).collect::<String>() + &" ".repeat(120) + "pass\n";
    assert!(compile(Language::Python, &blocks).is_err(), "blocks nested too deeply");
    let braces = "{".repeat(3000);
    assert!(compile(Language::JavaScript, &braces).is_err());
    let ifs = "if (1) ".repeat(900);
    assert!(compile(Language::JavaScript, &ifs).is_err());
    let chain = format!("let x = 1{};", " + 1".repeat(1500));
    assert!(compile(Language::JavaScript, &chain).is_err(), "a chain of operators is bounded");
    let calls = format!("f{};", "()".repeat(3000));
    assert!(compile(Language::JavaScript, &calls).is_err());
    // A list that holds itself is not an endless comparison.
    assert_eq!(py("a = [1]\na.append(a)\nb = [1]\nb.append(b)\nprint(a == b, a == a, len(a))"), ["False True 2"]);
    assert!(compile(Language::Python, &"x = 1\n".repeat(2000)).is_err(), "longer than a board keeps");
    let funcs: String = (0..80).map(|i| format!("def f{i}():\n    pass\n")).collect();
    assert!(compile(Language::Python, &funcs).is_err(), "too many functions");
}

/// Every prefix and every single-character damage of programs in both languages compiles or
/// errors, and what compiles runs to an end, an error or a pause, never a hang or a panic.
#[test]
fn damaged_programs_never_panic() {
    let programs = [
        (
            Language::Python,
            "import time\nlimit = 30\nxs = [1, 2, 3]\ndef f(a, b):\n    return a * b if a else -b\nwhile True:\n    t = analogRead('A0')\n    for x in xs:\n        print(f'{x}:{t:.1f}', f(x, t))\n    digitalWrite('D0', 1 if t > limit else 0)\n    time.sleep(2)\n",
        ),
        (
            Language::JavaScript,
            "const limit = 30;\nlet xs = [1, 2, 3];\nconst g = (a, b) => a * b;\nfunction loop() {\n  const t = analogRead('A0');\n  for (let i = 0; i < xs.length; i++) { console.log(`${xs[i]}:${t}`, g(xs[i], t)); }\n  digitalWrite('D0', t > limit ? 1 : 0);\n  delay(2000);\n}\n",
        ),
    ];
    for (lang, src) in programs {
        let chars: Vec<char> = src.chars().collect();
        for cut in 0..=chars.len() {
            let text: String = chars.iter().take(cut).collect();
            exercise(lang, &text);
            let mut damaged = chars.clone();
            if let Some(c) = damaged.get_mut(cut) {
                *c = ['(', ')', '"', '\n', ' ', '{', '}', ':', '[', '$', '\\', '0'][cut % 12];
            }
            exercise(lang, &damaged.into_iter().collect::<String>());
        }
    }
    for junk in [
        "",
        "\0",
        "\u{feff}x",
        "é = 1",
        "😀",
        "'''",
        "\"\"\"",
        "`${",
        "f'{'",
        "f'{x'",
        "x = (",
        "0x",
        "1e",
        "1e+",
        ".5.5",
        "\\",
        "a.b.c.d()",
        "[]()",
        "()()",
        "x[",
        "lambda",
        "lambda:",
        "def",
        "for",
        "for x in",
        "{{{{",
        "))))",
        "if 1:\n\telse",
    ] {
        exercise(Language::Python, junk);
        exercise(Language::JavaScript, junk);
    }
}

fn exercise(lang: Language, text: &str) {
    if let Ok(mut vm) = compile(lang, text) {
        let (mut io, mut out) = (Fake::default(), Vec::new());
        for step in 0..4u64 {
            if !matches!(vm.run(&mut io, step * 10_000_000, &mut out), Out::Sleep(_)) {
                break;
            }
        }
    }
}
