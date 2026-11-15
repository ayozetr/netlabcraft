# Lab 35 — Troubleshooting: port security that shuts the wrong port

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Secure two access ports, then find two mistakes in the configuration from the symptoms:
  a port that goes err-disabled for its own user, and a port that stays up but silently drops
  one of its two hosts.
- Read `show port-security`, `show interfaces status err-disabled` and `show interfaces`.
- Recover an err-disabled port the right way (cause first, then `shutdown` and `no shutdown`),
  and set up automatic recovery with `errdisable recovery`.

## Topology

```text
         ┌─ Fa0/1 ── PC1
 SRV ─ Fa0/10 ── S1 ─┤
         └─ Fa0/2 ── Hub1 ──┬── PC2
                            └── PC3
```

| Device | Model | Address |
|---|---|---|
| S1 | Switch-24 | — |
| SRV | PC (S1 Fa0/10) | 192.168.35.10/24 |
| PC1 | PC (S1 Fa0/1) | 192.168.35.11/24 |
| PC2 | PC (through Hub1, on S1 Fa0/2) | 192.168.35.12/24 |
| PC3 | PC (through Hub1, on S1 Fa0/2) | 192.168.35.13/24 |

PC1 is one employee's computer. PC2 and PC3 share a hub on one wall socket (the policy says
one computer per socket, but the hub is there). SRV is a server and needs no port security.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Hub-8", "name": "Hub1"}
device.add {"model": "PC", "name": "SRV"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
link.add {"a": "SRV", "b": "S1", "b_port": "fa0/10"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "Hub1", "b": "S1", "b_port": "fa0/2"}
link.add {"a": "PC2", "b": "Hub1"}
link.add {"a": "PC3", "b": "Hub1"}
```

```console SRV
ipconfig 192.168.35.10 255.255.255.0
```

```console PC1
ipconfig 192.168.35.11 255.255.255.0
```

```console PC2
ipconfig 192.168.35.12 255.255.255.0
```

```console PC3
ipconfig 192.168.35.13 255.255.255.0
```

## Part 1 — Before port security

Nothing is secured yet; every computer reaches the server.

```verify
ping PC1 192.168.35.10 => success
ping PC2 192.168.35.10 => success
ping PC3 192.168.35.10 => success
```

## Part 2 — The change

A colleague secures the two user ports "like the guide says": at most one MAC address, on
both. Fa0/1 gets a fixed address typed by hand, Fa0/2 keeps the first address it sees and
counts the others without shutting the port (`restrict`).

```console S1
enable
configure terminal
interface fa0/1
switchport mode access
switchport port-security
switchport port-security maximum 1
switchport port-security mac-address 0a00.5e00.0001
switchport port-security violation shutdown
interface fa0/2
switchport mode access
switchport port-security
switchport port-security maximum 1
switchport port-security violation restrict
end
```

## Part 3 — Diagnose

Order matters on the shared socket: PC2 speaks first and becomes the learned address.

```verify
ping PC1 192.168.35.10 => failure
ping PC2 192.168.35.10 => success
ping PC3 192.168.35.10 => failure
```

PC1 cannot reach anything, and PC3 cannot while PC2 can. Two different problems. The
summary of every secured port comes first:

```verify
S1# show port-security
  expect: err-disabled
  expect: restrict
  expect: secure-up
  expect: (configured)
```

Fa0/1 is err-disabled, Fa0/2 is up. The err-disabled list gives the cause, and the interface
itself says it too:

```verify
S1# show interfaces status err-disabled
  expect: Fa0/1
  expect: err-disabled
  expect: psecure-violation
  reject: Fa0/2
S1# show interfaces fa0/1
  expect: err-disabled by port security
S1# show errdisable recovery
  expect: Recovery interval: 300 s
  expect: never (shutdown / no shutdown)
```

### Fa0/1: the allowed address is not PC1's

The port allows one address, the one typed by hand, and PC1's own MAC address is not that
one. Its first frame was therefore "one address too many", and violation mode `shutdown`
turned the port off. `show port-security` lists the configured address next to the port:
compare it with PC1's (`ipconfig` on PC1). Typing addresses by hand is how this happens;
`sticky` makes the port learn the first address instead.

Resetting the port without removing the cause would only shut it again at the next frame.
Remove the cause first, then bounce the port (`shutdown`, `no shutdown`):

```console S1
configure terminal
interface fa0/1
no switchport port-security
switchport port-security
switchport port-security maximum 1
switchport port-security mac-address sticky
shutdown
no shutdown
end
```

The port starts again through spanning tree's listening and learning states, so wait before
testing:

```lab
sim.advance {"seconds": 35}
```

```verify
ping PC1 192.168.35.10 => success
S1# show port-security
  expect: sticky
S1# show interfaces status err-disabled
  expect: No port is err-disabled.
S1# show running-config
  expect: switchport port-security mac-address sticky
```

### Fa0/2: two computers, one allowed address

The port is up and PC2 works, so nothing was shut. With `restrict`, any frame from an address
beyond the maximum is dropped and counted, and the port carries on for the one that got in
first. PC3 is that second address. The Violations column of `show port-security` counts what
was dropped. The fix depends on the policy: remove the hub, or allow two computers on that
port.

```console S1
configure terminal
interface fa0/2
switchport port-security maximum 2
end
```

```verify
ping PC2 192.168.35.10 => success
ping PC3 192.168.35.10 => success
S1# show running-config
  expect: switchport port-security maximum 2
  expect: switchport port-security violation restrict
```

## Part 4 — Recover by itself

For the future, S1 can bring a port that port security shut back by itself after a while.
This is no substitute for finding the cause: if the cause is still there, the port is shut
again at the next offending frame.

```console S1
configure terminal
errdisable recovery cause psecure-violation
errdisable recovery interval 30
end
```

```verify
S1# show errdisable recovery
  expect: psecure-violation    yes
  expect: Recovery interval: 30 s
S1# show port-security
  reject: err-disabled
ping PC1 192.168.35.10 => success
ping PC2 192.168.35.10 => success
ping PC3 192.168.35.10 => success
```

## Reflection

1. Why did the same "one address" rule shut Fa0/1 and only count violations on Fa0/2? Which
   violation mode would you choose for an office, and which for a server room?
2. Why is it wrong to type `shutdown` and `no shutdown` on an err-disabled port before looking
   for the cause?
3. What does `sticky` do that a hand-typed address does not, and what does it not protect
   against?
4. What does the 30-second recovery interval risk on a port whose cause is still there?
