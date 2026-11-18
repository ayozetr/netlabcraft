# Lab 58 — Aliases, a scheduled reload, the extended ping and telnet by name

**Course:** CCNA 2 follow-up (device administration) · **Time:** 30 minutes

## Objectives

- Give a long command a short name with `alias exec`, and see it written in the running
  configuration.
- Schedule a restart with `reload in` and `reload at`, read it with `show reload` and see that
  the device really restarts when simulated time reaches it, going back to its saved
  configuration.
- Answer the questions of the interactive `ping`, which has more options than the one-line form.
- Name a device with `ip host` and log in to it with `telnet NAME`.

## Topology

```text
 R1 ───────────── R2
 10.58.12.1/24    10.58.12.2/24
```

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "R2"}
link.add {"a": "R1", "a_port": "g0/0", "b": "R2", "b_port": "g0/0"}
```

```console R1
enable
configure terminal
interface g0/0
ip address 10.58.12.1 255.255.255.0
no shutdown
exit
ip host CORE 10.58.12.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 10.58.12.2 255.255.255.0
no shutdown
exit
line vty 0 4
password remote-lab
login
end
```

## Part 1 — An alias

`alias exec NAME text` makes `NAME` stand for `text` at the privileged prompt. The alias is kept
in the running configuration, and a line that starts with the alias keeps whatever follows it.

```console R1
configure terminal
alias exec sib show ip interface brief
end
```

```verify
R1# sib
  expect: 10.58.12.1
R1# show aliases
  expect: sib
R1# show running-config
  expect: alias exec sib show ip interface brief
```

## Part 2 — The extended ping

Typed alone, `ping` asks its questions one by one. An empty answer takes the default in square
brackets, and `abort` gives up. Answer the first five questions, then say no to the extended
commands:

```console R1
ping
ip
CORE
5
100
2
```

The target was a name, so the console says how it was resolved before the replies:

```verify
R1# n
  expect: Translating "CORE"...static [OK]
  expect: !!!
```

## Part 3 — Telnet by name

`telnet`, `connect` and `ssh -l USER` take the name too:

```verify
R1# telnet CORE
  expect: Translating "CORE"...static [OK]
  expect: Open
```

```console R1
remote-lab
exit
```

## Part 4 — A scheduled reload

Save the configuration, add a loopback that is not saved, and ask for a restart in two minutes:

```console R1
copy running-config startup-config
configure terminal
interface loopback 9
ip address 9.9.9.9 255.255.255.255
end
reload in 2 lab maintenance
```

```verify
R1# show reload
  expect: lab maintenance
```

Simulated time does not move while commands are typed. Let three minutes pass; the next line
typed at the device finds that the time has come, and the device has started again from its
saved configuration, so the loopback is gone:

```lab
sim.advance {"seconds": 180}
```

```verify
R1# show reload
  expect: scheduled reload took place
  expect: No reload is scheduled
```

```console R1
enable
```

```verify
R1# show running-config
  reject: 9.9.9.9
  expect: alias exec sib
```

## Checklist

- [ ] `sib` ran `show ip interface brief`.
- [ ] The extended ping printed the translation line before the replies.
- [ ] `telnet CORE` opened a session without typing the address.
- [ ] After the scheduled reload the unsaved loopback was gone and the alias, which was saved with the configuration, was still there.
