# Lab 56 — PEAP with a server certificate the client does not trust

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Give a RADIUS server a certificate (`aaa.certificate`), and see the PEAP tunnel carry it in a
  real TLS handshake.
- Log a laptop in without checking the server, and read what the server showed.
- Turn on "Validate server certificate" (`dot1x.trust`) with a CA the server's certificate does
  not chain to, and see the login refused, with the reason.
- Trust the right certificate and log in again.

## Topology

| Device | Model | Address |
|---|---|---|
| AP1 | AccessPoint | 10.56.0.2/24 (VLAN 1) |
| SRV1 | Server | 10.56.0.10/24 (RADIUS) |
| SRV2 | Server | not connected: an unrelated server whose certificate the laptop trusts |
| Laptop1 | Laptop-WiFi | 10.56.0.21/24 |

Every password in this lab is a made-up one.

```lab
device.add {"model": "AccessPoint", "name": "AP1", "x": 200, "y": 0}
device.add {"model": "Server", "name": "SRV1", "x": 200, "y": -120}
device.add {"model": "Server", "name": "SRV2", "x": 400, "y": -120}
device.add {"model": "Laptop-WiFi", "name": "Laptop1", "x": 300, "y": 150}
link.add {"a": "SRV1", "b": "AP1", "b_port": "g0"}
```

```console SRV1
ipconfig 10.56.0.10 255.255.255.0
```

```console Laptop1
ipconfig 10.56.0.21 255.255.255.0
```

```console AP1
enable
configure terminal
interface vlan 1
ip address 10.56.0.2 255.255.255.0
no shutdown
end
```

## Part 1 — A RADIUS server with a certificate

The server answers its access point, which has a secret of its own. The server also gets a
certificate: a Server has no console, so it is set by command. Here it is self-signed (a
certificate authority in the lab can issue it instead: `"ca": "ROUTER"`), for a key pair the
simulator makes.

```lab
server.services {"device": "SRV1", "services": {"aaa": {"on": true, "clients": [{"name": "AP1", "address": "10.56.0.2", "secret": "lab56-secret"}], "users": [{"name": "ana", "password": "ana-lab56"}]}}}
aaa.certificate {"device": "SRV1", "self_signed": true, "subject": "CN=radius.lab.test, O=NetLab"}
aaa.certificate {"device": "SRV2", "self_signed": true, "subject": "CN=other.lab.test, O=Elsewhere"}
device.wireless {"device": "AP1", "ssid": "Corp", "mode": "wpa2-enterprise", "radius_ip": "10.56.0.10", "radius_secret": "lab56-secret"}
```

## Part 2 — A login that does not check the server

The laptop logs in as it always did. The tunnel is real TLS: the server's flight carries its
certificate, so it is several EAP messages long, each acknowledged by the laptop. The laptop saw
the certificate and checked nothing.

```lab
device.wireless {"device": "Laptop1", "ssid": "Corp", "mode": "wpa2-enterprise", "user": "ana", "password": "ana-lab56"}
sim.advance {"seconds": 10}
```

```verify
ping Laptop1 10.56.0.10 => success
run dot1x.status {"device": "Laptop1"}
  expect: "associated":true
  expect: "subject":"CN=radius.lab.test, O=NetLab"
  expect: "verified":false
events: PEAP
```

Anyone could have shown that certificate: nothing says the server is the company's.

## Part 3 — Validate the server, trusting the wrong authority

Turn on the check, name the server the certificate must be for, and trust only the certificate of
the unrelated SRV2.

```lab
dot1x.trust {"device": "Laptop1", "validate": true, "ca": "SRV2", "server_name": "radius.lab.test"}
sim.advance {"seconds": 10}
```

```verify
run dot1x.status {"device": "Laptop1"}
  expect: "validate":true
  expect: "associated":false
  expect: "verified":false
  expect: "trusted_cas":["CN=other.lab.test, O=Elsewhere"]
ping Laptop1 10.56.0.10 => failure
```

The laptop refuses the tunnel before it sends a password: the password never reaches a server
that cannot prove who it is. The reason is in the report (`error`) and in `debug dot1x` on the
laptop.

## Part 4 — Trust the right certificate

Trust the certificate of SRV1 itself (it is self-signed, so it is its own authority).

```lab
dot1x.trust {"device": "Laptop1", "clear": true, "ca": "SRV1"}
sim.advance {"seconds": 10}
```

```verify
run dot1x.status {"device": "Laptop1"}
  expect: "associated":true
  expect: "verified":true
  expect: "trusted_cas":["CN=radius.lab.test, O=NetLab"]
ping Laptop1 10.56.0.10 => success
```

A name that is not the certificate's is refused the same way: set `server_name` to something else
and the login fails again with the name in the reason.

```lab
dot1x.trust {"device": "Laptop1", "server_name": "elsewhere.test"}
sim.advance {"seconds": 10}
```

```verify
run dot1x.status {"device": "Laptop1"}
  expect: "associated":false
```

## What you learned

- PEAP's tunnel is TLS: the server proves who it is with a certificate, and the password only
  goes inside once the host has accepted it.
- A host that does not validate gets the encryption and none of the authentication.
- "Validate server certificate" needs the CA the server's certificate chains to, and optionally
  the name it must be for.
