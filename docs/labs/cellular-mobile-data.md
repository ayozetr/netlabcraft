# Lab — Mobile data over a cell tower

**Course:** IoT and mobile devices (extra lab, no number: it belongs to no CCNA course) · **Time:** 40 minutes

## Objectives

- Build a small provider network: a cell tower, a central office server and the lab behind it.
- Give a smartphone and a tablet mobile data and read what each side knows about the attach.
- See why a device is refused (the APN, the data plan) and why it loses service (Cellular Data off,
  out of range).
- Watch the attach exchange in Simulation mode.

NetLabCraft's cellular network is a model of its own, not 3GPP: there is no SIM, no
authentication and no handover. A device attaches to the nearest tower in range that has a free
sector; the tower carries its frames to the central office server; the server admits it (pool,
data plan, APN) and gives it an address, and routes it to the lab.

## Topology

```text
Smartphone1 ~~~\
                 Tower1 ───── CO1 ───── Web
Tablet1     ~~~/        Et0   g0/0 g0/1   g0
```

| Device | Model | Role | Address |
|---|---|---|---|
| Tower1 | Cell Tower | radio, six sectors, backhaul Ethernet0 | none |
| CO1 | Central Office Server | the provider's data service | g0/0 10.64.0.1/24 (the pool's gateway), g0/1 192.168.60.1/24 |
| Web | Server | a web server in the lab | 192.168.60.10/24, gateway 192.168.60.1 |
| Smartphone1 | Smartphone | mobile device | from the pool |
| Tablet1 | Tablet | mobile device | from the pool |

A tower covers 400 workspace units around it; both devices are placed well inside that.

```lab
device.add {"model": "Cell Tower", "name": "Tower1", "x": 140, "y": 100}
device.add {"model": "Central Office Server", "name": "CO1", "x": 380, "y": 100}
device.add {"model": "Server", "name": "Web", "x": 620, "y": 100}
device.add {"model": "Smartphone", "name": "Smartphone1", "x": 80, "y": 240}
device.add {"model": "Tablet", "name": "Tablet1", "x": 220, "y": 240}
link.add {"a": "Tower1", "a_port": "Ethernet0", "b": "CO1", "b_port": "g0/0"}
link.add {"a": "CO1", "a_port": "g0/1", "b": "Web", "b_port": "g0"}
```

## Part 1 — The provider's side

The central office server leaves the factory with its interfaces up. Give `g0/0` an address in the
network the mobile devices will use (it becomes their gateway), give `g0/1` the lab's, and name the
pool.

```console CO1
enable
configure terminal
interface g0/0
ip address 10.64.0.1 255.255.255.0
no shutdown
interface g0/1
ip address 192.168.60.1 255.255.255.0
no shutdown
exit
cellular pool 10.64.0.0 255.255.255.0
end
```

```console Web
ipconfig 192.168.60.10 255.255.255.0 192.168.60.1
```

```verify
CO1# show cellular
  expect: 10.64.0.0/24
  expect: Data plan    : on
  expect: Smartphone1
  expect: Tablet1
```

Both devices are already on the list: they were in range and asked for no particular APN, and the
server admits any APN until one is set.

## Part 2 — The device's side

The phone's Command Prompt has a `cellular` command; the same settings are in its Settings app and
under Config, Cellular0.

```verify
Smartphone1> cellular
  expect: Cellular data: on
  expect: attached
  expect: 10.64.0.2
Tablet1> cellular
  expect: 10.64.0.3
Smartphone1> ipconfig
  expect: Cellular0
ping Smartphone1 192.168.60.10 => success
ping Web 10.64.0.2 => success
ping Tablet1 10.64.0.2 => success
```

The tablet reaches the phone through the central office server, as it would through any router: a
tower never carries one device's frames to another's sector.

## Part 3 — Watch the attach

In Simulation mode the attach is an exchange of frames of ours (protocol CELL): the device asks, the
tower carries the request to its backhaul, the server answers with an accept or a reject.

```lab
sim.clear_events {}
cell.settings {"device": "Smartphone1", "data": false}
cell.settings {"device": "Smartphone1", "data": true}
sim.advance {"seconds": 1}
```

```verify
events: Cell attach request
events: Cell attach accept
```

## Part 4 — Refusals

The provider decides which APN it serves. Make CO1 accept only `internet`: the devices, which ask for
none, are refused and lose their address.

```console CO1
configure terminal
cellular apn internet
end
```

```verify
CO1# show cellular
  expect: refused
Smartphone1> cellular
  expect: not attached
  expect: APN
ping Smartphone1 192.168.60.10 => failure
```

Give the phone the APN; the tablet keeps asking for none.

```console Smartphone1
cellular apn internet
```

```verify
Smartphone1> cellular
  expect: attached
ping Smartphone1 192.168.60.10 => success
ping Tablet1 192.168.60.10 => failure
```

A data plan that is off refuses everyone, whatever they ask for.

```console CO1
configure terminal
cellular data-plan off
end
```

```verify
ping Smartphone1 192.168.60.10 => failure
CO1# show cellular
  expect: Data plan    : off
```

```console CO1
configure terminal
cellular data-plan on
end
```

```verify
ping Smartphone1 192.168.60.10 => success
```

## Part 5 — Losing service

Cellular Data off takes the radio's connection down at once.

```console Tablet1
cellular apn internet
cellular off
```

```verify
Tablet1> cellular
  expect: Cellular Data is off
ping Tablet1 192.168.60.10 => failure
```

```console Tablet1
cellular on
```

```verify
ping Tablet1 192.168.60.10 => success
```

Walking out of coverage does the same. Move the phone far from the tower, then back.

```lab
device.move {"device": "Smartphone1", "x": 5000, "y": 5000}
```

```verify
Smartphone1> cellular
  expect: no cell tower in range
ping Smartphone1 192.168.60.10 => failure
```

```lab
device.move {"device": "Smartphone1", "x": 80, "y": 240}
```

```verify
ping Smartphone1 192.168.60.10 => success
run cell.status {"device": "Smartphone1"}
  expect: "active":"cellular"
```

## Reflection

1. Why does the central office server's `g0/0` need an address in the pool's network?
2. A phone is joined to a Wi-Fi network and has mobile data on. Which one carries its traffic, and
   what does it fall back to when the Wi-Fi goes away?
3. What would the lab need if a host in the lab had to start a conversation with a phone, and its
   gateway were not the central office server?
