# Lab 12 — A wireless LAN with a controller

**Course:** CCNA 2 (Switching, Routing and Wireless Essentials) · **Time:** 50 minutes

## Objectives

- Join a lightweight access point to a wireless LAN controller (WLC) over CAPWAP.
- Create two WLANs on the controller, each with WPA2-PSK and its own VLAN.
- Route the two VLANs and serve them with DHCP from a router.
- Connect wireless laptops and check they land in the right subnet.

## Topology

```text
            R1 (G0/0, router on a stick)
             │
            G0/1
 WLC1 ─Fa0/1─ S1 (PoE) ─Fa0/2─ LAP1 · · · Laptop1 (Staff), Laptop2 (Guest)
```

| VLAN | Use | Subnet | Gateway |
|---|---|---|---|
| 1 | Management (WLC, AP) | 192.168.99.0/24 | 192.168.99.1 (R1 G0/0) |
| 10 | Staff WLAN | 192.168.10.0/24 | 192.168.10.1 (R1 G0/0.10) |
| 20 | Guest WLAN | 192.168.20.0/24 | 192.168.20.1 (R1 G0/0.20) |

| Device | Model | Address |
|---|---|---|
| R1 | Router-2 | as above |
| S1 | Switch-8-PoE | — |
| WLC1 | WLC | 192.168.99.2/24 (management) |
| LAP1 | LAP | 192.168.99.3/24, powered by S1 over PoE |
| Laptop1, Laptop2 | Laptop-WiFi | DHCP |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-8-PoE", "name": "S1"}
device.add {"model": "WLC", "name": "WLC1"}
device.add {"model": "LAP", "name": "LAP1"}
device.add {"model": "Laptop-WiFi", "name": "Laptop1"}
device.add {"model": "Laptop-WiFi", "name": "Laptop2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "WLC1", "a_port": "g0/1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "LAP1", "a_port": "g0", "b": "S1", "b_port": "fa0/2"}
```

## Part 1 — Router and switch

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.99.1 255.255.255.0
no shutdown
interface g0/0.10
encapsulation dot1Q 10
ip address 192.168.10.1 255.255.255.0
interface g0/0.20
encapsulation dot1Q 20
ip address 192.168.20.1 255.255.255.0
exit
ip dhcp excluded-address 192.168.10.1 192.168.10.9
ip dhcp excluded-address 192.168.20.1 192.168.20.9
ip dhcp pool STAFF
network 192.168.10.0 255.255.255.0
default-router 192.168.10.1
ip dhcp pool GUEST
network 192.168.20.0 255.255.255.0
default-router 192.168.20.1
end
```

The access point tags each WLAN's traffic with its VLAN, so its switch port is a trunk, as is
the uplink to the router.

```console S1
enable
configure terminal
vlan 10
name Staff
vlan 20
name Guest
exit
interface g0/1
switchport mode trunk
interface fa0/2
switchport mode trunk
end
```

## Part 2 — Controller and access point

The controller's management address is in VLAN 1. In the WLC's GUI this is the
management interface page; the console takes the same settings:

```console WLC1
enable
configure terminal
interface vlan 1
ip address 192.168.99.2 255.255.255.0
no shutdown
exit
ip default-gateway 192.168.99.1
end
```

Two WLANs, each with its SSID, WPA2-PSK passphrase and VLAN (WLANs page of the GUI):

```lab
wlc.wlan {"device": "WLC1", "id": 1, "ssid": "Staff", "security": "psk", "key": "staffpass1", "vlan": 10}
wlc.wlan {"device": "WLC1", "id": 2, "ssid": "Guest", "security": "psk", "key": "guestpass1", "vlan": 20}
```

The access point needs an address in the management VLAN and the controller's address
(it would also find a controller in its own subnet by itself):

```console LAP1
enable
configure terminal
interface vlan 1
ip address 192.168.99.3 255.255.255.0
no shutdown
exit
ip default-gateway 192.168.99.1
interface g0
switchport mode trunk
end
```

```lab
lap.controller {"device": "LAP1", "address": "192.168.99.2"}
```

```verify
WLC1# show wlan
  expect: Staff
  expect: Guest
WLC1# show capwap
  expect: LAP1
ping WLC1 192.168.99.1 => success
```

## Part 3 — Wireless clients

On each laptop's Desktop → PC Wireless (or its Config tab, Wireless0), pick the SSID and
type the passphrase; then ask for an address.

```lab
device.wireless {"device": "Laptop1", "ssid": "Staff", "key": "staffpass1"}
device.wireless {"device": "Laptop2", "ssid": "Guest", "key": "guestpass1"}
```

```console Laptop1
ipconfig /renew
```

```console Laptop2
ipconfig /renew
```

```verify
Laptop1> ipconfig
  expect: 192.168.10.10
Laptop2> ipconfig
  expect: 192.168.20.10
ping Laptop1 192.168.10.1 => success
ping Laptop1 192.168.20.10 => success
```

A wrong passphrase never associates:

```lab
device.wireless {"device": "Laptop2", "ssid": "Staff", "key": "wrongpass1"}
```

```verify
ping Laptop2 192.168.10.1 => failure
```

## Reflection

1. Why does LAP1's switch port need to be a trunk?
2. What does the access point send to the controller over CAPWAP, and what stays local?
3. How would you keep Guest users from reaching the Staff subnet?
