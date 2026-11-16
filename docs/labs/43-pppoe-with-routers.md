# Lab 43 — PPPoE with routers: an access concentrator and two clients

**Course:** CCNA 3 follow-up (WAN access) · **Time:** 60 minutes

## Objectives

- Build the provider side of PPPoE on a router: a `bba-group`, a virtual template, a pool of
  addresses, local accounts, and `pppoe enable` on the customer-facing port.
- Build the customer side on a router: a dialer interface with `encapsulation ppp`,
  `ip address negotiated` and the CHAP name and secret, tied to an Ethernet port by a pool number,
  and a default route out of the dialer.
- Read `show pppoe session` and `show pppoe summary` on both sides, see the host route the
  concentrator holds for each customer, and send traffic from one customer to the other through
  the provider.
- Switch the concentrator from CHAP to PAP, end every session with `clear pppoe all`, and see
  which of the two sides dials again by itself.

## Topology

```text
 CPE1 ── Fa0/1 ─┐
                ├─ S1 ── Fa0/24 ── ISP ── Lo0 10.43.255.1
 CPE2 ── Fa0/2 ─┘
```

| Device | Model | Port | Address |
|---|---|---|---|
| S1 | Switch-24 | | |
| ISP | Router-2 | G0/0 (to S1 Fa0/24) | none: it answers PPPoE |
| ISP | | Loopback 0 | 10.43.255.1/32, the concentrator's own address on every session |
| CPE1 | Router-2 | G0/0 (to S1 Fa0/1) | negotiated, from the pool 10.43.0.2 to 10.43.0.9 |
| CPE2 | Router-2 | G0/0 (to S1 Fa0/2) | negotiated |

The two customer routers dial through the switch, because discovery and the PPP session are
Ethernet frames. ISP has no address on its Ethernet port at all: the sessions take the address of
Loopback 0 through `ip unnumbered`.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Router-2", "name": "CPE1"}
device.add {"model": "Router-2", "name": "CPE2"}
link.add {"a": "ISP", "a_port": "g0/0", "b": "S1", "b_port": "fa0/24"}
link.add {"a": "CPE1", "a_port": "g0/0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "CPE2", "a_port": "g0/0", "b": "S1", "b_port": "fa0/2"}
```

## Part 1 — The provider side

The accounts are local users (the passwords are lab values). The pool is an `ip local pool`. The
group names the virtual template every session is built from, and the template says what a
session looks like: whose address it uses, where the customer's address comes from, and what the
customer must prove.

```console ISP
enable
configure terminal
username ann password lab-pass-1
username bob password lab-pass-2
ip local pool CUSTOMERS 10.43.0.2 10.43.0.9
interface loopback 0
ip address 10.43.255.1 255.255.255.255
exit
bba-group pppoe CUSTOMERS-GROUP
virtual-template 1
exit
interface virtual-template 1
ip unnumbered loopback 0
peer default ip address pool CUSTOMERS
ppp authentication chap
mtu 1492
exit
interface g0/0
no shutdown
pppoe enable group CUSTOMERS-GROUP
end
```

```verify
ISP# show running-config
  expect: bba-group pppoe CUSTOMERS-GROUP
  expect: interface Virtual-Template1
  expect: ip unnumbered Loopback0
  expect: peer default ip address pool CUSTOMERS
  expect: pppoe enable group CUSTOMERS-GROUP
ISP# show pppoe summary
  expect: Concentrator: 0 sessions open
  expect: group CUSTOMERS-GROUP, template Virtual-Template1
```

## Part 2 — The first customer

CPE1's dialer needs four things: PPP, an address that is negotiated, the name and secret the
concentrator's CHAP challenge is answered with, and a pool number. The port joins the pool with
`pppoe-client dial-pool-number`, and that is what makes the router dial. The default route leaves
by the dialer.

```console CPE1
enable
configure terminal
interface dialer 1
encapsulation ppp
ip address negotiated
ppp chap hostname ann
ppp chap password lab-pass-1
dialer pool 1
mtu 1492
exit
ip route 0.0.0.0 0.0.0.0 dialer 1
interface g0/0
no shutdown
pppoe-client dial-pool-number 1
end
```

```verify
CPE1# show pppoe session
  expect: Dialer1 over GigabitEthernet0/0
  expect: 10.43.0.2
  expect: 10.43.255.1
  expect: chap
CPE1# show ip interface brief
  expect: Dialer1
  expect: 10.43.0.2
ISP# show pppoe session
  expect: Virtual-Access1
  expect: 10.43.0.2
ISP# show ip route
  expect: Virtual-Access1
ping CPE1 10.43.255.1 => success
ping ISP 10.43.0.2 => success
```

Read the two sides together. CPE1's dialer has the address the pool gave and its peer is the
concentrator's own address (`10.43.255.1`). ISP has a host route for `10.43.0.2` out of a
`Virtual-Access` interface: it exists as long as the session does, and a PADT removes it.

## Part 3 — A second customer, and traffic between them

CPE2's port is brought up first. The switch port it joins goes through spanning tree's listening
and learning states, 30 seconds, before it forwards anything, so wait for it (a dialer that asks
too early hears nobody and says so):

```console CPE2
enable
configure terminal
interface g0/0
no shutdown
end
```

```lab
sim.advance {"seconds": 35}
```

Now CPE2's dialer, with bob's account, and the port joins the pool:

```console CPE2
configure terminal
interface dialer 1
encapsulation ppp
ip address negotiated
ppp chap hostname bob
ppp chap password lab-pass-2
dialer pool 1
exit
ip route 0.0.0.0 0.0.0.0 dialer 1
interface g0/0
pppoe-client dial-pool-number 1
end
```

```verify
ISP# show pppoe summary
  expect: Concentrator: 2 sessions open
CPE2# show pppoe session
  expect: 10.43.0.3
ping CPE1 10.43.0.3 => success
ping CPE2 10.43.0.2 => success
```

CPE1 has no route to `10.43.0.3` except the default route. The packet goes into its session,
ISP takes it out of that session, finds the host route of the other session and sends it down
that one. A wrong secret gets no session: change CPE2's secret and watch the dial fail with the
reason in the console output.

## Part 4 — PAP

Make the concentrator ask for PAP. The existing sessions stay up, because the authentication was
agreed when they were built; a new session uses PAP, and the dialer sends the name and secret
from `ppp pap sent-username` (the account has to exist on the ISP either way).

```console ISP
configure terminal
interface virtual-template 1
ppp authentication pap
end
```

```console CPE1
configure terminal
interface dialer 1
ppp pap sent-username ann password lab-pass-1
end
```

## Part 5 — Ending sessions

`clear pppoe all` ends every session of the router with a PADT. The concentrator forgets them, and
each customer's dialer loses its session, its address and its connected routes:

```console ISP
clear pppoe all
```

```verify
CPE1# show pppoe session
  expect: dialed: 0
CPE2# show pppoe session
  expect: dialed: 0
ISP# show pppoe summary
  expect: Concentrator: 0 sessions open
ping CPE1 10.43.255.1 => failure
```

A client does not dial again by itself when its session is ended from the far side: it dials when
its dialer or port is set up again. Dial CPE1 again, now with PAP (the concentrator asks for it):

```console CPE1
configure terminal
interface g0/0
no pppoe-client dial-pool-number
pppoe-client dial-pool-number 1
end
```

```verify
CPE1# show pppoe session
  expect: Dialer1 over GigabitEthernet0/0
  expect: pap
ping CPE1 10.43.255.1 => success
```
