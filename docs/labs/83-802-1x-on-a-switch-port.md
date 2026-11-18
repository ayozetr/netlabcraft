# Lab 83 — 802.1X on a switch port: nothing passes until the host has logged in

**Course:** CCNA 3 follow-up (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Turn on 802.1X on a switch (`dot1x system-auth-control`) and make a port ask its host for a
  login (`authentication port-control auto`).
- See that an unauthorized port carries EAPOL and nothing else, and read the EAPOL exchange and the
  RADIUS request the switch makes for the host in Simulation mode.
- Log a PC in with `dot1x.supplicant`, refuse one with the wrong password, and read `show dot1x`.
- Give a host that never answers a guest VLAN and a host the server refuses an auth-fail VLAN.

## Topology

```text
 PC1 ──Fa0/1┐
            S1 ──Gi0/1── SRV1 (RADIUS)
```

| Device | Model | Address |
|---|---|---|
| PC1 | PC | 10.83.0.1/24 |
| S1 | Switch-24 | 10.83.0.2/24 (VLAN 1), the authenticator |
| SRV1 | Server | 10.83.0.10/24 (RADIUS) |

Every password in this lab is a made-up one.

```lab
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "SRV1"}
link.add {"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "SRV1", "b": "S1", "b_port": "g0/1"}
server.services {"device": "SRV1", "services": {"aaa": {"on": true, "clients": [{"name": "S1", "address": "10.83.0.2", "secret": "lab83-secret"}], "users": [{"name": "ana", "password": "ana-lab83"}]}}}
```

```console PC1
ipconfig 10.83.0.1 255.255.255.0
```

```console SRV1
ipconfig 10.83.0.10 255.255.255.0
```

## Part 1 — Switch and port

The switch is a pass-through authenticator: it asks the host for an identity with EAPOL and sends
every answer on to the RADIUS server inside an Access-Request. For that it needs an address of its
own, a RADIUS server, and a method for 802.1X logins.

```console S1
enable
configure terminal
vlan 99
exit
aaa new-model
radius-server host 10.83.0.10 key lab83-secret
aaa authentication dot1x default group radius
dot1x system-auth-control
interface vlan 1
ip address 10.83.0.2 255.255.255.0
no shutdown
exit
interface fa0/1
switchport mode access
dot1x pae authenticator
authentication port-control auto
end
```

PC1 has no login yet, so nobody answers the switch.

```lab
sim.advance {"seconds": 10}
```

```verify
PC1> ping -n 1 10.83.0.10
  reject: reply from
S1# show dot1x all
  expect: Controlled ports: 1
  expect: UNAUTHORIZED
```

## Part 2 — A host with a login

`dot1x.supplicant` gives PC1 a login for its wired port. The PC sends an EAPOL-Start, the switch
asks for an identity, and the rest is the same PEAP tunnel a Wi-Fi client uses. Look at the event
list: EAPOL between the PC and the switch, RADIUS between the switch and the server.

```lab
dot1x.supplicant {"device": "PC1", "user": "ana", "password": "ana-lab83"}
sim.advance {"seconds": 10}
```

```verify
S1# show dot1x interface fa0/1
  expect: AUTHORIZED (the server accepted the login)
  expect: Identity:       ana
PC1> ping -n 1 10.83.0.10
  expect: reply from 10.83.0.10
events: EAPOL
run dot1x.status {"device": "PC1"}
  expect: "authenticated":true
```

## Part 3 — A wrong password

```lab
dot1x.supplicant {"device": "PC1", "user": "ana", "password": "not-the-password"}
sim.advance {"seconds": 10}
```

```verify
S1# show dot1x interface fa0/1
  expect: UNAUTHORIZED (the server refused the login)
PC1> ping -n 1 10.83.0.10
  reject: reply from
```

## Part 4 — Guest and auth-fail VLANs

A port can still put a host somewhere useful. Set both VLANs to 99 and run the login again.

```console S1
configure terminal
interface fa0/1
authentication event fail action authorize vlan 99
authentication event no-response action authorize vlan 99
end
dot1x re-authenticate interface fa0/1
```

```lab
sim.advance {"seconds": 10}
```

```verify
S1# show dot1x interface fa0/1
  expect: auth-fail VLAN 99
  expect: Access VLAN:    99
```

A host with no supplicant gets the guest VLAN once the switch has asked three times (30 seconds
each, `dot1x timeout tx-period`).

```lab
dot1x.supplicant {"device": "PC1", "clear": true}
sim.advance {"seconds": 100}
```

```verify
S1# show dot1x interface fa0/1
  expect: guest VLAN 99
```

## What you learned

- 802.1X keeps a port shut until the host has logged in; only EAPOL passes.
- The switch relays EAP to RADIUS; the server's accept or reject opens the port or keeps it shut.
- A guest VLAN is for hosts that never answer, an auth-fail VLAN for hosts the server refused.
- `show dot1x all`, `show dot1x interface` and `debug dot1x events` show where a login is.
