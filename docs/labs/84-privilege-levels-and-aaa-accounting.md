# Lab 84 — Privilege levels, AAA authorization and accounting

**Course:** CCNA 3 follow-up (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Give a router privilege levels of its own: move commands to level 7 (`privilege exec level 7 …`),
  protect the level with a secret (`enable secret level 7`) and reach it with `enable 7`.
- Let a TACACS+ server decide the level a user logs in at (`aaa authorization exec`) and refuse
  single commands (`aaa authorization commands 15`).
- Account for what people do (`aaa accounting exec` and `commands`) and read the records on the
  server (`aaa.records`).

## Topology

```text
 PC1 ──Fa0/1┐
            S1 ──Gi0/1── R1
 SRV1 ─Fa0/2┘
```

| Device | Model | Address |
|---|---|---|
| R1 | Router-2 | 192.168.84.1/24 on Gi0/0 |
| S1 | Switch-24 | no address |
| PC1 | PC | 192.168.84.10/24 |
| SRV1 | Server | 192.168.84.5/24 (TACACS+) |

Every password in this lab is a made-up one.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Switch-24", "name": "S1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "Server", "name": "SRV1"}
link.add {"a": "R1", "a_port": "g0/0", "b": "S1", "b_port": "g0/1"}
link.add {"a": "PC1", "a_port": "fa0", "b": "S1", "b_port": "fa0/1"}
link.add {"a": "SRV1", "b": "S1", "b_port": "fa0/2"}
```

```console PC1
ipconfig 192.168.84.10 255.255.255.0 192.168.84.1
```

```console SRV1
ipconfig 192.168.84.5 255.255.255.0 192.168.84.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.84.1 255.255.255.0
no shutdown
exit
```

## Part 1 — Levels on the console

Two levels exist unless you add more: 1 (user mode) and 15 (privileged mode). Move two commands to
level 7, and give level 7 a secret of its own.

```console R1
enable secret top-lab84
enable secret level 7 seven-lab84
privilege exec level 7 show running-config
privilege exec level 7 configure terminal
privilege configure level 7 ip domain-name
end
disable
```

In user mode `show running-config` is not there. `enable 7` asks for the level-7 secret, and the
session is at level 7: the three moved commands work, everything else of level 15 does not.

```verify
R1> show running-config
  expect: Not understood
R1> enable 7
R1> seven-lab84
R1# show privilege
  expect: level is 7
R1# reload
  expect: needs privilege level 15
R1# configure terminal
R1# ip domain-name lab84.test
R1# interface g0/0
  expect: needs privilege level 15
R1# end
```

`enable` asks for the level-15 secret and gives all of it.

```verify
R1# enable
R1# top-lab84
R1# show privilege
  expect: level is 15
R1# show aaa
  expect: Privilege rules: 3
```

## Part 2 — A TACACS+ server decides

SRV1 holds two users. `teacher` has a profile at level 15 and may not `reload`; `ops` has level 7.
R1 asks the server for the level of every login and for permission for every command of level 15.
Both kinds of record go back to the server.

```lab
server.services {"device": "SRV1", "services": {"aaa": {"on": true, "clients": [{"name": "R1", "address": "192.168.84.1", "secret": "lab84-key"}], "users": [{"name": "teacher", "password": "pt-lab84"}, {"name": "ops", "password": "ops-lab84"}]}}}
aaa.profile {"device": "SRV1", "user": "teacher", "privilege": 15, "deny": ["reload"]}
aaa.profile {"device": "SRV1", "user": "ops", "privilege": 7}
```

```console R1
configure terminal
aaa new-model
tacacs-server host 192.168.84.5 key lab84-key
aaa authentication login default group tacacs+ local
aaa authorization exec default group tacacs+ local
aaa authorization commands 15 default group tacacs+ none
aaa accounting exec default start-stop group tacacs+
aaa accounting commands 15 default start-stop group tacacs+
line vty 0 4
login authentication default
end
```

`teacher` logs in at level 15 (the prompt is `#`), runs a command and is refused another:

```verify
PC1> telnet 192.168.84.1
  expect: ZZZ
PC1> teacher
  expect: ZZZ
PC1> pt-lab84
  expect: ZZZ
PC1> show privilege
  expect: ZZZ
  expect: level is 15
PC1> show running-config
  expect: ZZZ
  expect: aaa authorization exec default
PC1> reload
  expect: ZZZ
  expect: Command authorization failed
PC1> exit
  expect: ZZZ
```

`ops` lands at level 7 although it typed the same kind of password:

```verify
PC1> telnet 192.168.84.1
  expect: ZZZ
PC1> ops
  expect: ZZZ
PC1> ops-lab84
  expect: ZZZ
PC1> show privilege
  expect: ZZZ
  expect: level is 7
PC1> exit
  expect: ZZZ
```

## Part 3 — The records

The server kept a start and a stop record for each login and for the one command it allowed, and
none for the refused `reload`.

```verify
run aaa.records {"device": "SRV1"}
  expect: "detail":"exec"
  expect: "detail":"command: show running-config"
  expect: "user":"teacher"
  expect: "protocol":"TACACS+"
  reject: reload
R1# show aaa accounting
  expect: acknowledged
```

## What you learned

- Privilege levels are a list of commands per level; `privilege` moves a command, `enable N` asks
  for the secret of level N.
- Authorization asks the server what the user may have (`priv-lvl`) or do (a command); a refusal
  is final unless a later method (`none`, `local`) is reached because no server answered.
- Accounting records the start and stop of a login and of each command of a level; the Server shows
  them (`aaa.records`).
