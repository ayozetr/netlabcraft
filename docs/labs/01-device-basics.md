# Lab 1 — Secure a new switch and router

**Course:** CCNA 1 (Introduction to Networks) · **Time:** 30 minutes · **Difficulty:** first lab

## Objectives

- Name a switch and a router, protect their consoles and privileged mode, and hide the
  passwords in the configuration.
- Show a login banner.
- Give the switch a management address and a default gateway.
- Turn on SSH on the router with a local user, so it can be managed only over an encrypted
  session.
- Save the configuration and check it.

## Topology

```text
 PC1 ──Fa0/1── S1 ──G0/1── R1 (G0/0)
```

| Device | Model | Interface | Address | Mask | Gateway |
|---|---|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1 | 255.255.255.0 | — |
| S1 | Switch-24 | VLAN 1 | 192.168.1.2 | 255.255.255.0 | 192.168.1.1 |
| PC1 | PC | Fa0 | 192.168.1.10 | 255.255.255.0 | 192.168.1.1 |

Build it from an empty lab (Insert menu, or these commands in the command palette, the CLI or
MCP):

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
```

## Part 1 — Address the PC

On PC1's Desktop, open the Command Prompt (or its Terminal tab) and set the address:

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

## Part 2 — Basic switch settings

Open S1's CLI tab. The switch starts with no name and no passwords.

```console S1
enable
configure terminal
hostname S1
no ip domain-lookup
enable secret Class123
line console 0
password Cisco123
login
exit
service password-encryption
banner motd #Authorised access only#
interface vlan 1
ip address 192.168.1.2 255.255.255.0
no shutdown
exit
ip default-gateway 192.168.1.1
end
copy running-config startup-config
```

- `enable secret` stores a hashed password for privileged mode.
- `service password-encryption` scrambles the plain passwords (here the console one) in the
  configuration listing.
- A switch is managed through a VLAN interface: VLAN 1 here.

## Part 3 — Router settings and SSH

```console R1
enable
configure terminal
hostname R1
enable secret Class123
service password-encryption
banner motd #Authorised access only#
line console 0
password Cisco123
login
exit
interface g0/0
description Link to S1
ip address 192.168.1.1 255.255.255.0
no shutdown
exit
ip domain-name lab.local
username admin secret Admin123
crypto key generate rsa general-keys modulus 1024
ip ssh version 2
line vty 0 4
login local
transport input ssh
exit
end
copy running-config startup-config
```

SSH needs a domain name and an RSA key pair (the key's name comes from the host and domain
names), a local user, and the VTY lines set to log in with that user and to refuse Telnet.

## Verification

The passwords are hidden, the banner and the VTY settings are in place, and the
configuration is saved:

```verify
R1# show running-config
  expect: hostname R1
  expect: enable secret
  expect: service password-encryption
  expect: transport input ssh
  expect: login local
  reject: password Cisco123
R1# show ip ssh
  expect: version 2
R1# show ip interface brief
  expect: 192.168.1.1
R1# show startup-config
  expect: hostname R1
S1# show running-config
  expect: hostname S1
  expect: ip default-gateway 192.168.1.1
  reject: password Cisco123
```

Every device reaches every other:

```verify
ping PC1 192.168.1.1 => success
ping PC1 192.168.1.2 => success
ping R1 192.168.1.2 => success
```

From PC1, log in to R1 over SSH (the banner shows, then R1 asks for `admin`'s password),
look around and leave. Telnet is refused, because the VTY
lines take SSH only:

```verify
PC1> ssh -l admin 192.168.1.1
  expect: Open
  expect: Authorised access only
PC1> Admin123
PC1> show users
  expect: vty
  expect: admin
PC1> exit
  expect: closed
PC1> telnet 192.168.1.1
  expect: refused
```

## Reflection

1. Why is `enable secret` preferred over `enable password`?
2. What would a person see if they connected to R1 with Telnet, and why?
3. Why does the switch need a default gateway, when it only forwards frames?
