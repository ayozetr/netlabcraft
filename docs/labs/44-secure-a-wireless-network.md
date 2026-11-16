# Lab 44 — Secure a wireless network

**Course:** CCNA 1 and 2 follow-up (wireless security) · **Time:** 60 minutes

## Objectives

- Set up an access point with each kind of authentication its Config tab offers: WEP, WPA2-PSK
  and an enterprise mode that asks a RADIUS server, and say what each one needs on the host.
- Read why a wireless client does not associate (`problem`) and fix it: a wrong key, a host set to
  another mode, a host set to another encryption type, a wrong password.
- Explain why WEP and TKIP are only here to be recognised and replaced.
- Change the coverage range of an access point and see which hosts still hear it.

## Topology

```text
 Laptop1 ~~~~ AP1 ── g0 ── SRV1
 Laptop2 ~~~~/
```

| Device | Model | Address |
|---|---|---|
| AP1 | AccessPoint | 10.44.0.2/24 (VLAN 1, for its RADIUS requests) |
| SRV1 | Server | 10.44.0.10/24, RADIUS (AAA service) |
| Laptop1 | Laptop-WiFi | 10.44.0.21/24 |
| Laptop2 | Laptop-WiFi | 10.44.0.22/24 |

Every key and password in this lab is a made-up one. The laptops stand about 180 workspace units
from the access point, well inside the default coverage range of 250 m (450 units).

```lab
device.add {"model": "AccessPoint", "name": "AP1", "x": 200, "y": 0}
device.add {"model": "Server", "name": "SRV1", "x": 200, "y": -120}
device.add {"model": "Laptop-WiFi", "name": "Laptop1", "x": 300, "y": 150}
device.add {"model": "Laptop-WiFi", "name": "Laptop2", "x": 100, "y": 150}
link.add {"a": "SRV1", "b": "AP1", "b_port": "g0"}
```

```console SRV1
ipconfig 10.44.0.10 255.255.255.0
```

```console Laptop1
ipconfig 10.44.0.21 255.255.255.0
```

```console Laptop2
ipconfig 10.44.0.22 255.255.255.0
```

```console AP1
enable
configure terminal
interface vlan 1
ip address 10.44.0.2 255.255.255.0
no shutdown
end
```

## Part 1 — WEP

A WEP key is 5 or 13 ASCII characters, or 10 or 26 hexadecimal digits; `abcde` and
`6162636465` are the same key. On a real network WEP is broken within minutes, so use it only
to see what it looks like.

```lab
device.wireless {"device": "AP1", "ssid": "Office", "mode": "wep", "wep_key": "abcde"}
device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wep", "wep_key": "6162636465"}
```

```verify
ping Laptop1 10.44.0.10 => success
run device.wireless {"device": "Laptop2", "ssid": "Office", "mode": "wep", "wep_key": "fghij"}
  expect: wrong WEP key
run device.wireless {"device": "Laptop2", "ssid": "Office"}
  expect: needs a WEP key
ping Laptop2 10.44.0.10 => failure
```

Laptop2 is the failing client: its key is wrong, then it has no key at all. Fix it.

```lab
device.wireless {"device": "Laptop2", "ssid": "Office", "mode": "wep", "wep_key": "abcde"}
```

```verify
ping Laptop2 10.44.0.10 => success
```

## Part 2 — WPA2-PSK, and the settings that must match

Move the network to WPA2-PSK with AES. Both laptops are still set for WEP, so neither joins.

```lab
device.wireless {"device": "AP1", "ssid": "Office", "mode": "wpa2-psk", "key": "lab44-passphrase", "encryption": "aes"}
```

```verify
ping Laptop1 10.44.0.10 => failure
run device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa-psk", "key": "lab44-passphrase"}
  expect: the network uses WPA2-PSK
run device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa2-psk", "key": "lab44-passphrase", "encryption": "tkip"}
  expect: encrypts with AES
run device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa2-psk", "key": "lab44-wrong-one"}
  expect: wrong WPA2 passphrase
```

WPA against WPA2, TKIP against AES and a wrong pass phrase each fail with their own reason.
Fix both laptops.

```lab
device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa2-psk", "key": "lab44-passphrase"}
device.wireless {"device": "Laptop2", "ssid": "Office", "mode": "wpa2-psk", "key": "lab44-passphrase", "encryption": "aes"}
```

```verify
ping Laptop1 10.44.0.10 => success
ping Laptop2 10.44.0.10 => success
events: 4-way handshake
```

## Part 3 — WPA2 with a RADIUS server

The access point asks its own RADIUS server, so it needs an address of its own that the server
knows as a client, with a shared secret.

```lab
server.services {"device": "SRV1", "services": {"aaa": {"on": true, "clients": [{"name": "AP1", "address": "10.44.0.2", "secret": "lab44-secret"}], "users": [{"name": "ana", "password": "ana-lab44"}]}}}
device.wireless {"device": "AP1", "ssid": "Office", "mode": "wpa2-enterprise", "radius_ip": "10.44.0.10", "radius_secret": "lab44-secret"}
```

```verify
run device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa2-enterprise", "user": "ana", "password": "wrong-lab44"}
  expect: refused the login
run device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa-enterprise", "user": "ana", "password": "ana-lab44"}
  expect: the network uses WPA2-Enterprise
```

```lab
device.wireless {"device": "Laptop1", "ssid": "Office", "mode": "wpa2-enterprise", "user": "ana", "password": "ana-lab44"}
```

```verify
ping Laptop1 10.44.0.10 => success
ping Laptop2 10.44.0.10 => failure
```

Laptop2 still has the WPA2-PSK settings, so it is turned away: the network is not the same
network any more.

## Part 4 — Coverage range

An access point's range is set in metres (250 m by default). Shrink it and the laptops, 180 units
(100 m) away, are out of range.

```lab
device.wireless {"device": "AP1", "ssid": "Office", "mode": "wpa2-enterprise", "radius_ip": "10.44.0.10", "radius_secret": "lab44-secret", "coverage_m": 50}
```

```verify
ping Laptop1 10.44.0.10 => failure
```

```lab
device.wireless {"device": "AP1", "ssid": "Office", "mode": "wpa2-enterprise", "radius_ip": "10.44.0.10", "radius_secret": "lab44-secret", "coverage_m": 250}
```

```verify
ping Laptop1 10.44.0.10 => success
```

## Reflection

1. Why is a WEP key worth nothing against someone who records the traffic, while a WPA2
   pass phrase is only as good as its length?
2. What does the access point need so that the RADIUS server answers it, and what does the host
   need instead?
3. Why does the reference app (and this one) have no WPA3 to pick?
