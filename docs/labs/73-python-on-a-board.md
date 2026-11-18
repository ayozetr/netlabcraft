# Lab 73 — Python on a single-board computer

**Course:** IoT follow-up · **Time:** 30 minutes

## Objectives

- Choose the language of a board's program and write a Python program that reads a sensor and
  switches a device.
- Read what the program printed, its variables and the line it stopped at.
- See that the language is checked when the program is saved, that a run-time error names its line,
  and that a loop that never pauses is stopped instead of freezing the simulator.

## Topology

```text
 Temp1 (A0) ── Board1 (Board SBC) ── (D0) Fan1
```

| Device | Model | Pin | Role |
|---|---|---|---|
| Board1 | Board SBC | | runs the Python program |
| Temp1 | IoT sensor | A0 | the reading is a temperature in degrees |
| Fan1 | IoT device | D0 | on while the room is hot |

A Board SBC runs BoardScript or Python; a Board MCU runs BoardScript or JavaScript (lab 74). The
Python is a documented subset ([board-languages.md](../board-languages.md)): variables, numbers,
text, lists, `if`, `while`, `for`, `def`, `print` and `time.sleep` in simulated time.

```lab
device.add {"model": "Board-SBC", "name": "Board1"}
device.add {"model": "IoT-Sensor", "name": "Temp1"}
device.add {"model": "IoT-Device", "name": "Fan1"}
board.wire {"device": "Board1", "pin": "A0", "thing": "Temp1"}
board.wire {"device": "Board1", "pin": "D0", "thing": "Fan1"}
iot.configure {"device": "Temp1", "value": 22}
```

## Part 1 — Choose the language and run

`board.program` takes the language and the text together (in the app: the Language list on the
board's Programming tab). Pins are read with `analogRead("A0")` and written with
`digitalWrite("D0", 1)`; `time.sleep(2)` waits two seconds of simulated time.

```lab
board.program {"device": "Board1", "language": "python", "source": "import time\nlimit = 28\nwhile True:\n    t = analogRead(\"A0\")\n    digitalWrite(\"D0\", 1 if t > limit else 0)\n    print(\"temperature\", t)\n    time.sleep(2)\n", "run": true}
sim.advance {"seconds": 3}
```

```verify
run board.status {"device": "Board1"}
  expect: "language":"python"
  expect: "state":"running"
  expect: temperature 22
  expect: "limit":28
run iot.configure {"device": "Fan1"}
  expect: "on":false
```

## Part 2 — The room gets hot

```lab
iot.configure {"device": "Temp1", "value": 35}
sim.advance {"seconds": 4}
```

```verify
run board.status {"device": "Board1"}
  expect: temperature 35
run iot.configure {"device": "Fan1"}
  expect: "on":true
```

## Part 3 — Mistakes

Type these as `board.program` sources (they are refused or stop, so they are not part of the script
above):

- `print(` is refused when it is saved: `line 1: unexpected the end of the line`.
- `print(y)` saves, runs, and stops: `line 1: NameError: name 'y' is not defined`.
- `while True:` followed by `pass` stops after 20000 steps: `steps without a pause`; a loop needs a
  `time.sleep` in it.
- `language: "javascript"` is refused on this board: `this board runs boardscript or python`.
