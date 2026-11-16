# Lab 41 — PPPoE client and access concentrator

**Course:** CCNA 3 follow-up (WAN access) · **Time:** 45 minutes

## Objectives

- Name the two stages of PPPoE: discovery (PADI, PADO, PADR, PADS) and the PPP session that
  follows, and read each in the event list.
- Follow PPP inside the session: LCP agrees on the link, PAP or CHAP authenticates the caller, and
  IPCP gives it an address.
- See that the whole exchange crosses a switch, because it is made of Ethernet frames (ethertypes
  `0x8863` and `0x8864`), and that IP then travels as PPP packets in session frames.
- Compare CHAP (a challenge and an MD5 answer, the password never sent) with PAP (the password
  in clear text), and end a session with a PADT.

## Topology

```text
 PC1 ── Fa0/1 ─┐
 PC2 ── Fa0/2 ─┼─ S1 ── Fa0/24 ── ISP1
```

| Device | Model | Port | Address |
|---|---|---|---|
| S1 | Switch-24 | | |
| PC1 | PC | S1 Fa0/1 | from PPPoE |
| PC2 | PC | S1 Fa0/2 | from PPPoE |
| ISP1 | Server | S1 Fa0/24 | 10.41.0.1/24 (pool 10.41.0.100 to 10.41.0.109) |

ISP1 is the access concentrator: a server with its PPPoE service on, a pool of addresses and two
accounts. The computers dial from their PPPoE Dialer (`pppoe.connect`), through the switch.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "Server", "name": "ISP1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC2", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "ISP1", "b": "S1", "b_port": "fa0/24"}
```

```console ISP1
ipconfig 10.41.0.1 255.255.255.0
```

## Part 1 — The service and discovery

Switch the service on with CHAP and two accounts (the passwords are lab values):

```lab
server.services {"device": "ISP1", "services": {"pppoe": {"on": true, "auth": "chap", "pool_start": "10.41.0.100", "pool_size": 10, "users": [{"name": "ann", "password": "lab-pass-1"}, {"name": "bob", "password": "lab-pass-2"}]}}}
```

Before anyone dials, the dialer can already find the concentrator: it broadcasts a PADI, the
switch floods it, and ISP1 answers with a PADO that names itself.

```verify
run pppoe.status {"device": "PC1"}
  expect: "state":"disconnected"
  expect: "server_found":true
  expect: "server":"ISP1"
events: PPPoE PADI
events: PPPoE PADO: access concentrator ISP1
```

## Part 2 — A CHAP session

PC1 dials with ann's account. Behind the one command are the four discovery frames, LCP in both
directions, a CHAP challenge and response, and IPCP.

```verify
run pppoe.connect {"device": "PC1", "username": "ann", "password": "lab-pass-1"}
  expect: "state":"connected"
  expect: "address":"10.41.0.100"
  expect: "gateway":"10.41.0.1"
  expect: "auth":"chap"
  reject: lab-pass-1
events: PPPoE PADR
events: PPPoE PADS: session
events: LCP Configure-Request (MRU 1492, CHAP
events: CHAP Challenge
events: CHAP Response (name ann)
events: CHAP Success
events: IPCP Configure-Nak (address 10.41.0.100)
events: IPCP Configure-Ack (address 10.41.0.100)
```

Read them in the Simulation panel in this order. The concentrator asks for CHAP in its LCP
request (the authentication option), the caller never sends its password, and IPCP is a short
negotiation: the caller asks for 0.0.0.0, the concentrator answers with the address from its pool
in a Configure-Nak, and the caller asks for that one.

```verify
PC1# ipconfig /all
  expect: 10.41.0.100
  expect: by PPPoE
  expect: PPPoE login ann
  expect: PPP auth    chap
```

## Part 3 — IP inside the session

Every datagram between PC1 and its concentrator is now a PPP IPv4 packet in a session frame, with
no ARP:

```verify
ping PC1 10.41.0.1 => success
events: carrying ICMP echo request
events: carrying ICMP echo reply
```

The summary of a session frame names the session and what it carries. The session frames hold at
most 1,492 bytes of IP (the MRU in LCP), eight less than Ethernet.

## Part 4 — PAP

Make the concentrator ask for PAP and let PC2 dial with bob's account. The service change keeps
PC1's session.

```lab
server.services {"device": "ISP1", "services": {"pppoe": {"on": true, "auth": "pap", "pool_start": "10.41.0.100", "pool_size": 10, "users": [{"name": "ann", "password": "lab-pass-1"}, {"name": "bob", "password": "lab-pass-2"}]}}}
```

```verify
run pppoe.connect {"device": "PC2", "username": "bob", "password": "lab-pass-2"}
  expect: "state":"connected"
  expect: "address":"10.41.0.101"
  expect: "auth":"pap"
events: PAP Authenticate-Request (user bob)
events: PAP Authenticate-Ack
```

PAP puts the user name and the password in one packet that anyone on the path can read; CHAP never
does. Each caller has a session id of its own (`PADS: session 0x0001` and `0x0002`), and the
concentrator told the two apart by it.

## Part 5 — Ending a session

A caller ends its session with an LCP Terminate-Request and a PADT, and the address goes away:

```verify
run pppoe.disconnect {"device": "PC2"}
  expect: "state":"disconnected"
events: PPPoE PADT
```

When the concentrator ends sessions, it is the one that sends the PADT: switch its service off and
PC1 loses its session, its address and its gateway.

```lab
server.services {"device": "ISP1", "services": {"pppoe": {"on": false}}}
```

```verify
run pppoe.status {"device": "PC1"}
  expect: "state":"disconnected"
PC1# ipconfig
  expect: not set
```

Save the lab with a session up and open it again: the session is part of the file, and the
concentrator still recognises it.
