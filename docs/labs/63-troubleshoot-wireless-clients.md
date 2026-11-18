# Lab 63 — Troubleshooting: four laptops that will not join the Wi-Fi

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 45 minutes

## Objectives

- Start from a WPA2-PSK network and four laptops that cannot join it, each for a different
  reason: a mistyped network name, a WPA setting against a WPA2 network, TKIP against AES,
  and a laptop out of the access point's range.
- Read the reason the simulator gives for a refused join (`problem`), and notice when it gives
  none, which is what a name that matches nothing and a distance that is too long have in common.
- Repair each laptop, and widen the access point's coverage as the last resort.

## Topology

```text
 SRV1 ── AP1 ))) Laptop1, Laptop2, Laptop3        Laptop4 (far away)
```

| Device | Model | Address |
|---|---|---|
| AP1 | AccessPoint | 10.63.0.2/24 (VLAN 1) |
| SRV1 | Server | 10.63.0.10/24 |
| Laptop1 to Laptop4 | Laptop-WiFi | 10.63.0.21 to 10.63.0.24 /24 |

The network is `Campus`, WPA2-PSK with AES. The pass phrase in this lab is a made-up one. The
first three laptops stand about 180 workspace units from the access point, well inside the
default range of 250 m (450 units); Laptop4 stands 800 units away.

```lab
device.add {"model": "AccessPoint", "name": "AP1", "x": 200, "y": 0}
device.add {"model": "Server", "name": "SRV1", "x": 200, "y": -120}
device.add {"model": "Laptop-WiFi", "name": "Laptop1", "x": 300, "y": 150}
device.add {"model": "Laptop-WiFi", "name": "Laptop2", "x": 100, "y": 150}
device.add {"model": "Laptop-WiFi", "name": "Laptop3", "x": 200, "y": 180}
device.add {"model": "Laptop-WiFi", "name": "Laptop4", "x": 1000, "y": 0}
link.add {"a": "SRV1", "b": "AP1", "b_port": "g0"}
```

```console SRV1
ipconfig 10.63.0.10 255.255.255.0
```

```console Laptop1
ipconfig 10.63.0.21 255.255.255.0
```

```console Laptop2
ipconfig 10.63.0.22 255.255.255.0
```

```console Laptop3
ipconfig 10.63.0.23 255.255.255.0
```

```console Laptop4
ipconfig 10.63.0.24 255.255.255.0
```

```console AP1
enable
configure terminal
interface vlan 1
ip address 10.63.0.2 255.255.255.0
no shutdown
end
```

## Part 1 — The network and what each laptop was given

```lab
device.wireless {"device": "AP1", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "aes"}
```

The help desk gave each user the settings on a note, and each note has its own mistake.

```lab
device.wireless {"device": "Laptop1", "ssid": "Campuss", "mode": "wpa2-psk", "key": "lab63-passphrase"}
device.wireless {"device": "Laptop2", "ssid": "Campus", "mode": "wpa-psk", "key": "lab63-passphrase"}
device.wireless {"device": "Laptop3", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "tkip"}
device.wireless {"device": "Laptop4", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase"}
```

```verify
ping Laptop1 10.63.0.10 => failure
ping Laptop2 10.63.0.10 => failure
ping Laptop3 10.63.0.10 => failure
ping Laptop4 10.63.0.10 => failure
```

Nobody is on the network. Applying the same settings again shows what each laptop is told:

## Part 2 — Read each reason

Laptop2 and Laptop3 *found* the network and were refused, so the access point says why: the
mode, then the encryption, then the pass phrase are compared in that order.

```verify
run device.wireless {"device": "Laptop2", "ssid": "Campus", "mode": "wpa-psk", "key": "lab63-passphrase"}
  expect: the network uses WPA2-PSK
run device.wireless {"device": "Laptop3", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "tkip"}
  expect: encrypts with AES
```

Laptop1 and Laptop4 get no reason, because they were not refused: Laptop1 asks for a network
called `Campuss`, which no access point offers (names are compared exactly), and Laptop4 is
too far from any access point to hear one. Both show no access point and no problem:

```verify
run device.wireless {"device": "Laptop1", "ssid": "Campuss", "mode": "wpa2-psk", "key": "lab63-passphrase"}
  expect: "associated_with":null
  expect: "problem":null
run device.wireless {"device": "Laptop4", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase"}
  expect: "associated_with":null
  expect: "problem":null
```

"No access point and no reason" means the problem is before the security: the name, the
range, or a radio that is off. A reason means the laptop is in range, the name matches, and a
setting is wrong.

## Part 3 — Repair

```lab
device.wireless {"device": "Laptop1", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase"}
device.wireless {"device": "Laptop2", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase"}
device.wireless {"device": "Laptop3", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "aes"}
```

```verify
ping Laptop1 10.63.0.10 => success
ping Laptop2 10.63.0.10 => success
ping Laptop3 10.63.0.10 => success
ping Laptop4 10.63.0.10 => failure
```

Laptop4 cannot be fixed on the laptop. Either it moves closer, or the access point reaches
farther: the coverage is a setting of the radio, up to 2000 m (3600 units). Raise it to 500 m
(900 units), which is more than the 800 units that separate them. A real access point cannot
reach farther by being told to; the setting here stands for a stronger antenna or a second
access point.

```lab
device.wireless {"device": "AP1", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "aes", "coverage_m": 500}
```

```verify
ping Laptop4 10.63.0.10 => success
run device.wireless {"device": "AP1", "ssid": "Campus", "mode": "wpa2-psk", "key": "lab63-passphrase", "encryption": "aes", "coverage_m": 500}
  expect: "coverage_m":500
```

## Reflection

1. A laptop says "no access point" and gives no reason. What are the three first things to check?
2. A laptop gets a reason. Which setting is compared first, and why do the reasons arrive in
   that order?
3. Why is raising the coverage of an access point a poor fix for a laptop that is far away?
4. Which of the four faults would also exist on a real network, and which only here?
