# Lab 62 — Troubleshooting: a remote-access VPN client that cannot connect

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Set up a router as a remote-access VPN server: a user, authentication and authorization
  lists, an address pool, a client group with its key, a transform set, a dynamic crypto map
  and a crypto map on the outside interface.
- Read what the VPN Client says when the server is missing a piece, and what it says when
  the client's own entries are wrong (group, key, user, password).
- Tell a server that is set up wrongly (every client fails) from a client that is set up wrongly
  (one client fails).

## Topology

```text
 PC1 (outside) ── (G0/0) R1 (G0/1) ── PC2 (inside)
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 (outside) | 203.0.113.1/24 |
| R1 | | G0/1 (inside) | 10.0.0.1/24 |
| PC1 | PC | Fa0 | 203.0.113.2/24 (the teleworker's computer) |
| PC2 | PC | Fa0 | 10.0.0.10/24 (a server at the office) |

PC1 gets an address from the pool `10.62.0.1` to `10.62.0.5` when it connects, and reaches PC2
through an encrypted tunnel to R1. The group name, key, user name and password in this lab are
made-up ones.

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R1", "b_port": "g0/1"}
```

```console PC1
ipconfig 203.0.113.2 255.255.255.0 203.0.113.1
```

```console PC2
ipconfig 10.0.0.10 255.255.255.0 10.0.0.1
```

```console R1
enable
configure terminal
interface g0/0
ip address 203.0.113.1 255.255.255.0
no shutdown
interface g0/1
ip address 10.0.0.1 255.255.255.0
no shutdown
end
```

## Part 1 — A server that is missing pieces

A colleague built the server from a checklist and skipped two lines: the transform set the
dynamic map should use, and the address pool. Everything else is there.

```console R1
configure terminal
username ann password pw-lab62
aaa authentication login vpn-auth local
aaa authorization network vpn-auth local
crypto isakmp client configuration group team
key key-lab62
exit
crypto ipsec transform-set TS esp-aes 256 esp-sha-hmac
exit
crypto dynamic-map DM 10
reverse-route
exit
crypto map CM client authentication list vpn-auth
crypto map CM isakmp authorization list vpn-auth
crypto map CM client configuration address respond
crypto map CM 10 ipsec-isakmp dynamic DM
interface g0/0
crypto map CM
end
```

Every client fails, whatever it types. The VPN Client's error is the server's refusal, and it
names the first piece that is missing, in the order the server checks them: first what the
dynamic map offers for the traffic (its transform set), then who the client is, then the address
pool.

```verify
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "key-lab62", "username": "ann", "password": "pw-lab62"}
  expect: no transform set
run vpnclient.status {"device": "PC1"}
  expect: "state":"disconnected"
R1# show crypto isakmp sa
  expect: No IKE peers yet
```

Give the dynamic map its transform set. The next refusal is about the pool, which the client
only meets once it has proved who it is.

```console R1
configure terminal
crypto dynamic-map DM 10
set transform-set TS
end
```

```verify
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "key-lab62", "username": "ann", "password": "pw-lab62"}
  expect: no address pool
```

```console R1
configure terminal
ip local pool VPNPOOL 10.62.0.1 10.62.0.5
crypto isakmp client configuration group team
pool VPNPOOL
end
```

## Part 2 — A client with the wrong entries

The server is complete now. A client that types something wrong is refused, and the server
does not say which of the entries it was: that would help someone guessing. Each of these four
attempts gets no tunnel, and the router has no session after any of them.

```verify
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "wrong-key", "username": "ann", "password": "pw-lab62"}
  expect: error:
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "nobody", "key": "key-lab62", "username": "ann", "password": "pw-lab62"}
  expect: error:
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "key-lab62", "username": "ann", "password": "wrong"}
  expect: error:
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "key-lab62", "username": "bob", "password": "pw-lab62"}
  expect: error:
run vpnclient.status {"device": "PC1"}
  expect: "state":"disconnected"
R1# show crypto isakmp sa
  reject: remote-access client
```

A client at an address where nothing answers is a different failure: the request gets no
answer at all.

```verify
run vpnclient.connect {"device": "PC1", "host": "203.0.113.77", "group": "team", "key": "key-lab62", "username": "ann", "password": "pw-lab62"}
  expect: did not answer
```

## Part 3 — The right entries

```verify
run vpnclient.connect {"device": "PC1", "host": "203.0.113.1", "group": "team", "key": "key-lab62", "username": "ann", "password": "pw-lab62"}
  expect: "state":"connected"
  expect: "address":"10.62.0.1"
  reject: key-lab62
  reject: pw-lab62
ping PC1 10.0.0.10 => success
R1# show crypto isakmp sa
  expect: remote-access client
  expect: group team, user ann
R1# show crypto ipsec sa
  expect: Remote-access client 203.0.113.2
R1# show ip local pool
  expect: 10.62.0.1
run vpnclient.disconnect {"device": "PC1"}
  expect: "state":"disconnected"
```

## Reflection

1. Which of the failures in Part 1 affect every client, and which only the client that types
   something wrong? How can you tell from the client's side?
2. Why does the server not tell the client whether the group, the key, the user or the password
   was wrong?
3. What does `reverse-route` do for the inside network?
4. Why must the pool not overlap with the inside network?
