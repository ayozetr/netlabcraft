# Lab 75 — Reverse Telnet through an octal card

**Course:** CCNA 3 follow-up (device management) · **Time:** 40 minutes

## Objectives

- Fit an octal asynchronous card in a modular router and cable one of its lines to the Console
  port of another router with an octal cable.
- Set a line up for reverse Telnet with `line`, `transport input telnet` and `modem inout`.
- Reach the far console from another router with `telnet <address> <2000 + line>`.
- Read `show line` and `show users`, close a session with the escape sequence and cut one with
  `clear line`.

## Topology

```text
 R3 ── Gi0/0 ──── Gi0/0 ── R1 (octal card, line 1) ═══ octal cable ═══ Console ── R2
     10.75.0.2/24         10.75.0.1/24
```

R1 is the terminal server: a modular router with an octal card (`Card-8A`, eight lines).
Line 1 is cabled to R2's Console port. A line is reached with Telnet to the router's address and
TCP port 2000 plus the line number (2001 is line 1): the public convention for asynchronous
lines. The session is R2's own console, with its banner and console password.

```lab
device.add {"model": "Router-M2", "name": "R1"}
device.add {"model": "Router-3", "name": "R2"}
device.add {"model": "Router-3", "name": "R3"}
device.power {"device": "R1", "on": false}
device.module.add {"device": "R1", "slot": "0/1", "module": "Card-8A"}
device.power {"device": "R1", "on": true}
link.add {"a": "R1", "b": "R2", "cable": "octal", "line": 0}
link.add {"a": "R1", "a_port": "g0/0", "b": "R3", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.75.0.1 255.255.255.0
no shutdown
end
```

```console R3
enable
configure terminal
interface g0/0
ip address 10.75.0.2 255.255.255.0
no shutdown
end
```

## Part 1 — The lines

`show line` lists the console and vty lines and, under them, one row per line of the octal card:
its state, the protocols it takes, who uses it and the device it is cabled to.

```verify
R1# show line
  expect: Cabled to
  expect: idle
  expect: no cable
```

A line takes no connection until `transport input telnet` is set on it:

```verify
R3# telnet 10.75.0.1 2001
  expect: refused
  expect: transport input telnet
```

## Part 2 — Set the line up

`line 1 8` edits the eight lines together; `line 1` edits one. `modem inout` and `no exec` are the
settings a terminal server's lines usually carry (they are kept and shown, they change nothing
here).

```console R1
configure terminal
line 1 8
transport input telnet
modem inout
no exec
end
```

```verify
R1# show running-config
  expect: line 1
  expect: transport input telnet
  expect: modem inout
  expect: no exec
```

## Part 3 — Reach the far console

From R3, Telnet to R1's address with port 2001. What is typed after that runs on R2:

```verify
R3# telnet 10.75.0.1 2001
  expect: Open
R3# show users
  expect: con 0
R1# show users
  expect: reverse Telnet from R3
  expect: console of R2
R1# show line
  expect: in use
```

Only one session can hold a line. A second try, from R1's own console, is refused:

```verify
R1# telnet 10.75.0.1 2001
  expect: refused
  expect: clear line 1
```

## Part 4 — Close it

The escape sequence (Ctrl-Shift-6, then x; typed here as `^^x`) closes the session and frees the
line. `exit` at the first level of the far console does the same. `clear line` on the terminal
server cuts a session from its side:

```verify
R3# ^^x
  expect: closed
R1# show line
  expect: idle
R3# telnet 10.75.0.1 2001
  expect: Open
R1# clear line 1
  expect: cleared
R3# show users
  expect: clear line
R1# show line
  expect: idle
```

## Part 5 — A console password

R2's console password is asked first, as it would be on a real console:

```console R2
enable
configure terminal
line console 0
password lab-pass-1
login
end
```

```verify
R3# telnet 10.75.0.1 2001
  expect: password protected
R3# wrong-password
  expect: invalid
R3# lab-pass-1
R3# show users
  expect: con 0
R3# ^^x
  expect: closed
```
