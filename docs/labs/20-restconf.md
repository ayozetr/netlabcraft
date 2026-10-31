# Lab 20 — Automation: RESTCONF from the Programming tab

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Turn on RESTCONF on a router, with HTTPS and a local user allowed to log in.
- Read the router's interfaces as JSON (YANG model `ietf-interfaces`) with a `GET`.
- Change the configuration with `PATCH` and `POST`, and remove it with `DELETE`.
- Check every change in the router's running configuration.

## Topology

```text
 PC1 (Programming tab) ── S1 ── (G0/0) R1
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| S1 | Switch-24 | — | — |
| PC1 | PC | Fa0 | 192.168.1.10/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/1"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

## Part 1 — RESTCONF on the router

RESTCONF runs over HTTPS and logs users in with the router's own user list. The user needs
privilege 15 to change the configuration.

```console R1
enable
configure terminal
hostname R1
username netops privilege 15 secret Auto2026
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
exit
ip http secure-server
ip http authentication local
restconf
end
```

## Part 2 — Read with GET

On PC1, Desktop → Programming: method `GET`, the URL below, header
`Accept: application/yang-data+json`, user `netops` and password `Auto2026`, then **Send**.

```verify
run pc.http_request {"pc": "PC1", "method": "GET", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces", "headers": {"Accept": "application/yang-data+json"}, "user": "netops", "password": "Auto2026"}
  expect: "status":200
  expect: GigabitEthernet0/0
  expect: 192.168.1.1
run pc.http_request {"pc": "PC1", "method": "GET", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces", "headers": {"Accept": "application/yang-data+json"}, "user": "netops", "password": "wrong"}
  expect: "status":401
```

A wrong password is refused with status 401.

## Part 3 — Change with PATCH and POST

`PATCH` merges the body into what is there: give G0/0 a description.

```lab
pc.http_request {"pc": "PC1", "method": "PATCH", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces/interface=GigabitEthernet0%2F0", "headers": {"Content-Type": "application/yang-data+json"}, "user": "netops", "password": "Auto2026", "body": {"ietf-interfaces:interface": {"name": "GigabitEthernet0/0", "description": "Managed by RESTCONF"}}}
```

`POST` creates something new: a loopback interface with an address.

```lab
pc.http_request {"pc": "PC1", "method": "POST", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces", "headers": {"Content-Type": "application/yang-data+json"}, "user": "netops", "password": "Auto2026", "body": {"ietf-interfaces:interface": {"name": "Loopback7", "type": "iana-if-type:softwareLoopback", "enabled": true, "ietf-ip:ipv4": {"address": [{"ip": "10.7.7.7", "netmask": "255.255.255.255"}]}}}}
```

```verify
R1# show running-config
  expect: description Managed by RESTCONF
  expect: interface Loopback7
  expect: ip address 10.7.7.7 255.255.255.255
ping PC1 10.7.7.7 => success
```

## Part 4 — Remove with DELETE

```lab
pc.http_request {"pc": "PC1", "method": "DELETE", "url": "https://192.168.1.1/restconf/data/ietf-interfaces:interfaces/interface=Loopback7", "user": "netops", "password": "Auto2026"}
```

```verify
R1# show running-config
  reject: Loopback7
ping PC1 10.7.7.7 => failure
```

## Reflection

1. Which HTTP method would replace G0/0's whole configuration rather than merge into it?
2. Why should RESTCONF use HTTPS and not plain HTTP?
3. How would a script change the description on fifty routers?
