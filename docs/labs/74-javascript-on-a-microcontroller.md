# Lab 74 — JavaScript on a microcontroller board

**Course:** IoT follow-up · **Time:** 30 minutes

## Objectives

- Write a JavaScript program for a Board MCU with a `loop` function that the board runs again and
  again.
- Use `analogRead`, `digitalWrite`, `delay` and `console.log` and read the output.
- Know what the subset leaves out and how an error shows its line.

## Topology

```text
 Temp1 (A0) ── Board1 (Board MCU) ── (D0) Fan1
```

```lab
device.add {"model": "Board-MCU", "name": "Board1"}
device.add {"model": "IoT-Sensor", "name": "Temp1"}
device.add {"model": "IoT-Device", "name": "Fan1"}
board.wire {"device": "Board1", "pin": "A0", "thing": "Temp1"}
board.wire {"device": "Board1", "pin": "D0", "thing": "Fan1"}
iot.configure {"device": "Temp1", "value": 22}
```

## Part 1 — A loop function

When a program defines `loop`, the board calls it again and again (10 ms of simulated time apart,
plus whatever `delay` waits inside it), after the top-level code and after `setup` if there is
one. The program below counts its runs and switches the fan above 28 degrees.

```lab
board.program {"device": "Board1", "language": "javascript", "source": "const limit = 28;\nlet count = 0;\nfunction loop() {\n  const t = analogRead(\"A0\");\n  digitalWrite(\"D0\", t > limit ? 1 : 0);\n  count++;\n  console.log(`reading ${t}, run ${count}`);\n  delay(2000);\n}\n", "run": true}
sim.advance {"seconds": 7}
```

```verify
run board.status {"device": "Board1"}
  expect: "language":"javascript"
  expect: reading 22, run 1
  expect: reading 22, run 3
run iot.configure {"device": "Fan1"}
  expect: "on":false
```

## Part 2 — Heat

```lab
iot.configure {"device": "Temp1", "value": 35}
sim.advance {"seconds": 4}
```

```verify
run board.status {"device": "Board1"}
  expect: reading 35
run iot.configure {"device": "Fan1"}
  expect: "on":true
```

## Part 3 — What is left out

The subset has no objects, classes, `switch`, `try`, `async` or bitwise operators; an arrow function
cannot read the local variables of the function around it (globals are fine). Each is refused with
its line when the program is saved, for example `line 2: `switch` is not part of the JavaScript
subset the boards run`.
