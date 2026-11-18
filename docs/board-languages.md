# Board languages: BoardScript, Python and JavaScript

> **Last reviewed:** 2026-11-18 · **Last updated:** 2026-11-18 · **Change:** minor (final consistency pass: the review line names only the latest change) · **Target:** the programmable boards of the reference app (Python on the single-board computer, JavaScript on the microcontroller)

A programmable board runs one program, written in one of three languages. The language is part of the
board: it is chosen on the **Programming** tab (the Language list, shown when the board runs more than one),
by `board.program` with `language`, and saved in the lab file (`board.language`, left out for BoardScript).

| Board | Languages |
|---|---|
| Board SBC (single-board computer) | BoardScript, Python |
| Board MCU (microcontroller board) | BoardScript, JavaScript |
| PLC | BoardScript |

Python and JavaScript are **documented subsets** written for the simulator, not the real interpreters (see
[decisions.md](decisions.md)). A program that uses something outside the subset is refused when it is saved,
with the line, or stops with the line when it runs. Both run in **simulated time**, on the same machine, with the
same limits:

| Limit | Value |
|---|---|
| Program text | 8000 characters |
| Operations per run slice (between two pauses) | 20000: a loop with no `sleep` or `delay` stops with an error |
| Calls nested | 24 deep |
| Variables in one scope | 64 |
| Text value | 400 characters |
| Lists alive, values in all lists | 128 and 2000 (lists nothing reaches are collected) |
| HTTP requests and reports per slice | 8 |
| One pause | 1 ms to 1 hour of simulated time |
| Functions, nesting of blocks and expressions | 64, 40 and 30 |

Whole numbers are 64-bit and **checked**: a result that does not fit is an `OverflowError`, never a wrap (Python's
unbounded integers are not modelled). Errors show in the board's **Output** with their line, for example
`stopped: line 3: NameError: name 'y' is not defined`, and in `board.status` as `message`.

## The board's functions

The same functions exist in both languages, named as a Python or JavaScript user expects. A pin is a number (`3` is
`D3` for `digitalRead` and `digitalWrite`, `A3` for `analogRead`) or a name (`"D3"`, `"A0"`, any case).

| Function | What it does |
|---|---|
| `digitalRead(pin)` | The wired thing's state as 0 or 1 (a sensor's reading above 0 is 1). |
| `analogRead(pin)` | The whole reading of the wired sensor (or the state of a device). |
| `digitalWrite(pin, level)` | Switches the wired smart device: any level but 0 (or `True`/`true`, `HIGH`) is on. |
| `analogWrite(pin, level)` | A level for a dimmable LED (0-100), a servo (0-180) or an RGB LED (`0xRRGGBB`); a number pin means `D`. |
| `customRead(pin)`, `customWrite(pin, value)` | The IoT custom cable: the same as `read` and `write` of BoardScript, by pin name. |
| `channel(pin, n)` | A second reading (a humiture sensor's humidity is channel 1); `channel(pin, 0)` is the first. |
| `rgb(r, g, b)` | Three levels 0-255 as one number `0xRRGGBB`, what an RGB LED takes. |
| `delay(ms)` | Waits that many milliseconds of simulated time. |
| `time.sleep(s)` (Python), `sleep(s)` | Waits that many seconds (a fraction is fine). |
| `millis()`, `Date.now()`, `time.time()` | Simulated time: milliseconds, milliseconds, seconds. |
| `http_get(path)` / `httpGet(path)` | An HTTP GET to the board's IoT server (Programming tab); the first line of the answer. |
| `report(value)` | Registers the board with its IoT server as a sensor reading that value. |
| `HIGH`, `LOW` | 1 and 0. |

A program that defines `loop()` (either language) has it called again and again after the top-level code, 10 ms of
simulated time apart plus whatever the function waits; `setup()`, if defined next to `loop`, is called once first.
A program without `loop` runs once and stops (BoardScript, unlike them, starts over a second after its end).

## Python subset

Statements: assignment (`x = 1`, `a[i] = v`, `x += 1` and the other augmented forms), `if`/`elif`/`else`, `while`,
`for NAME in list | string | range(...)`, `break`, `continue`, `pass`, `def f(a, b):` with `return`, `global`,
`import time`/`import math`/`from time import …` (accepted; the names are always there), one-line bodies
(`if x: y`), `lambda a, b: expr`.

Values: whole numbers, floats, text (`'…'`, `"…"`, `"""…"""`, f-strings with `{expr}` and `{x:.2f}`), `True`, `False`,
`None`, lists (`[1, 2]`), `range(...)`.

Operators: `+ - * / // % **`, comparisons, `and or not`, `in`, `not in`, `is`, `is not`, `a if c else b`, indexing
(negative too), calls.

Built-in names: `print len str int float bool abs min max sum round range list sorted`, `math.sqrt floor ceil pow pi`,
`time.sleep time.time`, and the board functions above. Methods: lists `append pop insert reverse sort clear`; text
`upper lower strip startswith endswith find replace split join`.

Left out: dictionaries, tuples and unpacking, classes, `try`, `with`, `yield`, comprehensions, keyword and default
arguments, closures over a function's local variables, other imports, `input` and files.

```python
import time
limit = 28
while True:
    t = analogRead("A0")
    digitalWrite("D0", 1 if t > limit else 0)
    print("temperature", t, f"({t / 10:.1f} tens)")
    time.sleep(2)
```

## JavaScript subset

Statements: `let`/`const`/`var` (function-scoped here), assignment and `+=`-style forms, `++`/`--`, `if`/`else if`/`else`,
`while`, `for (init; test; step)`, `for (const x of list)`, `break`, `continue`, `function f(a) { … }` (hoisted),
`return`, blocks, optional semicolons, `//` and `/* */` comments.

Values: numbers (whole or fractional: a fractional result that is whole is a whole number, as in JavaScript), text
(`'…'`, `"…"`, template literals with `${expr}`), `true`, `false`, `null` and `undefined` (the same value here), arrays.

Operators: `+ - * / % **`, `== === != !==` (the same, strict), `< > <= >=`, `&& || !`, `a ? b : c`, indexing, `.length`,
arrow functions `x => …`, `(a, b) => { … }` and `function` expressions.

Built-in names: `console.log`, `parseInt parseFloat Number String Boolean`, `Math.floor ceil round trunc abs min max sqrt
pow PI`, `Date.now`, and the board functions above. Methods: arrays `push pop insert reverse sort join includes indexOf
clear`; text `toUpperCase toLowerCase trim startsWith endsWith includes indexOf replace split`; numbers `toFixed`.

Left out: objects and classes, `switch`, `try`/`throw`, `do … while`, `async`, `new`, `typeof`, bitwise operators, closures over a function's local variables (a function sees globals and its own
variables), getters, labels and regular expressions.

```javascript
const limit = 28;
let count = 0;
function loop() {
  const t = analogRead("A0");
  digitalWrite("D0", t > limit ? 1 : 0);
  count++;
  console.log(`reading ${t}, run ${count}`);
  delay(2000);
}
```

## Where the code is

`crates/sim/src/lang_parse.rs` (lexer and the two parsers, one syntax tree), `crates/sim/src/lang.rs` (compiler to a
flat list of operations, the machine, the built-ins), `crates/sim/src/script.rs` (the board machine both languages
and BoardScript share), `crates/sim/src/tests_lang.rs` and `crates/engine/src/tests_lang.rs` (tests, including
hostile programs). Labs: [73](labs/73-python-on-a-board.md) and [74](labs/74-javascript-on-a-microcontroller.md).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-18 | minor | Final consistency pass: the review line names only the latest change; the revision rows are in date order, newest first. |
| 2026-11-18 | minor | Created: Python for the single-board computer and JavaScript for the microcontroller board, as bounded subsets on one machine, with the board functions, the language list on the Programming tab and `board.language` in the lab file. |
