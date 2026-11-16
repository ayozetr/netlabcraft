# Lab 47 — Dial-up: a modem, the telephone network and an access server

**Course:** CCNA 3 follow-up (WAN access) · **Time:** 45 minutes

## Objectives

- Give the ports of a telephone cloud their numbers and read the pair a call makes in it.
- Set up an access server: an `ip local pool`, a `bba-group`, a virtual template with
  `ppp authentication chap`, and `modem inout` on its modem line.
- Dial from a Dial-up PC with a number and an account, read what the call gave the computer, and
  reach the access server over it.
- See a busy line, a number that does not exist, a wrong password and a line that no longer
  answers, and what each leaves behind.

## Topology

```text
 PC1 ── Phone0 ── Ph1 ─┐
                       ├─ PSTN1 ── Ph0 ── Phone0 ── AS1 ── Lo0 10.47.0.1
 PC2 ── Phone0 ── Ph2 ─┘
```

| Device | Model | Port | Number or address |
|---|---|---|---|
| PSTN1 | Telephone Cloud | Phone0 (to AS1) | 5550100 |
| PSTN1 | | Phone1 (to PC1) | 5550101 |
| PSTN1 | | Phone2 (to PC2) | 5550102 |
| AS1 | Access Server | Phone0 | the modem line, answers calls |
| AS1 | | Loopback 0 | 10.47.0.1/32, the address every call uses as its own (`ip unnumbered`) |
| PC1, PC2 | Dial-up PC | Phone0 | from the pool 10.47.0.2 to 10.47.0.9 |

The simulator carries PPP over the call as it carries it over a PPPoE session (discovery, LCP,
CHAP or PAP and IPCP in session frames): it does not model modem tones or asynchronous framing,
so the access server is set up with the same `bba-group` and virtual template as a PPPoE
concentrator, and `modem inout` takes the place of `pppoe enable` on a modem line.

```lab
device.add {"model": "Telephone Cloud", "name": "PSTN1"}
device.add {"model": "Access Server", "name": "AS1"}
device.add {"model": "Dial-up PC", "name": "PC1"}
device.add {"model": "Dial-up PC", "name": "PC2"}
link.add {"a": "AS1", "a_port": "ph0", "b": "PSTN1", "b_port": "ph0"}
link.add {"a": "PC1", "a_port": "ph0", "b": "PSTN1", "b_port": "ph1"}
link.add {"a": "PC2", "a_port": "ph0", "b": "PSTN1", "b_port": "ph2"}
```

## Part 1 — The numbers

A telephone cloud has eight phone ports and no pairs at first: a call makes one. Each port gets a
number (spaces, dashes and brackets are ignored when it is typed, and one port has each number).

```lab
cloud.number {"device": "PSTN1", "port": "ph0", "number": "555-0100"}
cloud.number {"device": "PSTN1", "port": "ph1", "number": "5550101"}
cloud.number {"device": "PSTN1", "port": "ph2", "number": "5550102"}
```

```verify
run cloud.number {"device": "PSTN1"}
  expect: "number":"5550100"
  expect: "calls":[]
```

## Part 2 — The access server

The accounts are local users (the passwords are lab values). The pool is an `ip local pool`. The
virtual template says what a call looks like: whose address it uses, where the caller's address
comes from, and what the caller must prove. `modem inout` makes the line answer.

```console AS1
enable
configure terminal
username ann password lab-pass-1
username bob password lab-pass-2
ip local pool CALLERS 10.47.0.2 10.47.0.9
interface loopback 0
ip address 10.47.0.1 255.255.255.255
exit
bba-group pppoe LINES
virtual-template 1
exit
interface virtual-template 1
ip unnumbered loopback 0
peer default ip address pool CALLERS
ppp authentication chap
exit
interface ph0
no shutdown
modem inout group LINES
end
```

```verify
AS1# show running-config
  expect: modem inout group LINES
  expect: peer default ip address pool CALLERS
  expect: ppp authentication chap
```

## Part 3 — The first call

PC1 dials 5550100 with ann's account. The call joins `Phone1` to `Phone0` in the cloud for as long
as it lasts, and PPP runs over that circuit.

```verify
run dialup.connect {"device": "PC1", "number": "(555) 0100", "username": "ann", "password": "lab-pass-1"}
  expect: "state":"connected"
  expect: "number":"5550100"
  expect: "carrier":true
  expect: "address":"10.47.0.2"
  expect: "server":"AS1"
  expect: "auth":"chap"
  reject: lab-pass-1
run cloud.number {"device": "PSTN1"}
  expect: ["Phone1","Phone0"]
AS1# show pppoe session
  expect: Virtual-Access1
  expect: 10.47.0.2
ping PC1 10.47.0.1 => success
ping AS1 10.47.0.2 => success
```

## Part 4 — What goes wrong

The line to the access server is in use, so PC2 hears a busy signal. A number nobody has, a
number that is the caller's own, a phone with no modem line behind it and a wrong password fail too, and a failed call leaves no pair in the cloud:

```verify
run dialup.connect {"device": "PC2", "number": "5550100", "username": "bob", "password": "lab-pass-2"}
  expect: is busy
run dialup.connect {"device": "PC2", "number": "5559999", "username": "bob", "password": "lab-pass-2"}
  expect: is not a number on the telephone network
run dialup.connect {"device": "PC2", "number": "5550102", "username": "bob", "password": "lab-pass-2"}
  expect: own number
run dialup.connect {"device": "PC2", "number": "5550101", "username": "bob", "password": "lab-pass-2"}
  expect: no answer
```

## Part 5 — Hanging up, and a second caller

Hanging up closes PPP and parts the pair, so the line is free again:

```verify
run dialup.disconnect {"device": "PC1"}
  expect: "state":"disconnected"
  expect: "carrier":false
run dialup.connect {"device": "PC2", "number": "5550100", "username": "bob", "password": "wrong"}
  expect: PPP failed
run dialup.connect {"device": "PC2", "number": "5550100", "username": "bob", "password": "lab-pass-2"}
  expect: "address":"10.47.0.2"
ping PC2 10.47.0.1 => success
```

PC2 gets the first free address of the pool, `10.47.0.2`, because PC1 gave it back. A line that
is switched off for calls no longer answers, and a caller hears nothing:

```console AS1
configure terminal
interface ph0
no modem inout
end
```

```verify
run dialup.disconnect {"device": "PC2"}
  expect: "state":"disconnected"
run dialup.connect {"device": "PC1", "number": "5550100", "username": "ann", "password": "lab-pass-1"}
  expect: no answer
```
