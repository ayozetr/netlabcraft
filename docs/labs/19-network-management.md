# Lab 19 — Network management: CDP, LLDP, NTP, syslog, SNMP and TFTP backups

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Discover neighbours with CDP and LLDP, and turn CDP off where it shouldn't be heard.
- Set the clocks from an NTP server, and timestamp log messages.
- Send log messages to a syslog server, filtered by severity.
- Read a router's name and uptime from a PC with SNMP.
- Back up a configuration to a TFTP server and restore it.

## Topology

```text
 Server1 (NTP, syslog, TFTP) ─┐
                              S1 ──G0/1── (G0/0) R1 (G0/1) ──── (G0/0) R2
 PC1 (SNMP manager) ──────────┘
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 10.0.12.1/30 |
| R2 | Router-2 | G0/0 | 10.0.12.2/30 |
| S1 | Switch-24 | VLAN 1 | 192.168.1.2/24 |
| Server1 | Server | G0 | 192.168.1.10/24 |
| PC1 | PC | Fa0 | 192.168.1.20/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "Server", "name": "Server1"}
device.add {"model": "PC", "name": "PC1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "R1", "a_port": "g0/1", "b": "R2", "b_port": "g0/0"}
link.add {"a": "Server1", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "PC1", "b": "S1", "b_port": "fa0/2"}
```

Server1's Services tab: turn on NTP, Syslog and TFTP (here with the equivalent command).

```lab
server.services {"device": "Server1", "services": {"ntp": {"on": true}, "syslog": {"on": true}, "tftp": {"on": true}}}
```

```console Server1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC1
ipconfig 192.168.1.20 255.255.255.0 192.168.1.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.0.12.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 10.0.12.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.0.12.2 255.255.255.252
no shutdown
exit
ip route 192.168.1.0 255.255.255.0 10.0.12.1
end
```

```console S1
enable
configure terminal
interface vlan 1
ip address 192.168.1.2 255.255.255.0
no shutdown
exit
ip default-gateway 192.168.1.1
end
```

## Part 1 — CDP and LLDP

CDP is on by default on every interface. Wait for the first advertisements (every 60 s):

```lab
sim.advance {"seconds": 65}
```

```verify
R1# show cdp neighbors
  expect: S1
  expect: R2
R1# show cdp neighbors detail
  expect: 10.0.12.2
```

R2's link leads towards a provider in a real network: CDP shouldn't be sent there. Turn it
off on that interface only, and use the standard LLDP instead everywhere:

```console R1
configure terminal
interface g0/1
no cdp enable
exit
lldp run
end
```

```console R2
configure terminal
lldp run
end
```

```console S1
configure terminal
lldp run
end
```

```lab
sim.advance {"seconds": 200}
```

```verify
R1# show cdp neighbors
  expect: S1
  reject: R2
R1# show lldp neighbors
  expect: R2
  expect: S1
```

## Part 2 — NTP and timestamps

```console R1
configure terminal
ntp server 192.168.1.10
service timestamps log datetime msec
end
```

```console S1
configure terminal
ntp server 192.168.1.10
end
```

```lab
sim.advance {"seconds": 70}
```

```verify
R1# show ntp status
  expect: Clock synchronised with 192.168.1.10
R1# show ntp associations
  expect: 192.168.1.10
S1# show ntp status
  expect: Clock synchronised
```

## Part 3 — Syslog

Send warnings and anything more severe (levels 0–4) to Server1. Interface changes are
level 5 (notifications), so let everything up to level 5 through:

```console R1
configure terminal
logging host 192.168.1.10
logging trap notifications
end
```

Make some news: turn R1's link to R2 off and on.

```console R1
configure terminal
interface g0/1
shutdown
no shutdown
end
```

```lab
sim.advance {"seconds": 5}
```

```verify
R1# show logging
  expect: Syslog servers: 192.168.1.10
  expect: GigabitEthernet0/1: link down
run server.syslog {"device": "Server1"}
  expect: GigabitEthernet0/1
  expect: 192.168.1.1
```

## Part 4 — SNMP

A read-only community lets a manager read the router's MIB; nothing can be changed with it.

```console R1
configure terminal
snmp-server community NetView ro
snmp-server location Main closet, rack 2
snmp-server contact noc@lab.local
end
```

From PC1's Desktop → MIB Browser (or the equivalent command), read the router's name
(sysName, 1.3.6.1.2.1.1.5.0) and location (sysLocation, 1.3.6.1.2.1.1.6.0):

```verify
run snmp.request {"device": "PC1", "target": "192.168.1.1", "community": "NetView", "oid": "1.3.6.1.2.1.1.5.0"}
  expect: R1
run snmp.request {"device": "PC1", "target": "192.168.1.1", "community": "NetView", "oid": "1.3.6.1.2.1.1.6.0"}
  expect: Main closet, rack 2
run snmp.request {"device": "PC1", "target": "192.168.1.1", "community": "public", "oid": "1.3.6.1.2.1.1.5.0"}
  expect: did not answer
```

The default `public` community isn't configured, so the router doesn't answer at all.

## Part 5 — Back up and restore with TFTP

```console R1
copy running-config tftp
192.168.1.10
R1-backup.cfg
```

```verify
run topology.get {}
  expect: R1-backup.cfg
```

Break something — remove the default route — then restore the backup. Copying into the
running configuration merges the file into it, so the missing line comes back:

```console R1
configure terminal
no ip route 0.0.0.0 0.0.0.0 10.0.12.2
end
```

```verify
ping R1 10.0.12.2 => success
R1# show ip route
  expect: No default route
```

```console R1
copy tftp running-config
192.168.1.10
R1-backup.cfg
```

```verify
R1# show running-config
  expect: ip route 0.0.0.0 0.0.0.0 10.0.12.2
R1# show ip route
  expect: S*   0.0.0.0/0
```

## Reflection

1. Why turn CDP off towards a provider, and why might LLDP be preferred in a mixed-vendor
   network?
2. Why do log messages need correct clocks?
3. What could someone do with a read-write SNMP community they guessed?
