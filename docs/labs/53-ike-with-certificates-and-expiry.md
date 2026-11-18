# Lab 53 — IKE with certificates, and a certificate that has expired

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 60 minutes

## Objectives

- Run a certificate authority on the provider's router and have both sites enrol with it.
- Authenticate IKE with `authentication rsa-sig` instead of a pre-shared key, and read which
  certificate each router proved itself with.
- See what the exchange carries: a certificate request in main mode messages 3 and 4, and the
  certificate and its authority's certificate in messages 5 and 6.
- Let the certificates expire, read the refusal, and repair it by enrolling again.

## Topology

```text
 PC1 ── (G0/0) R1 (G0/1) ──── (G0/0) ISP (G0/1) ──── (G0/1) R2 (G0/0) ── PC2
```

| Device | Model | Interface | Address |
|---|---|---|---|
| R1 | Router-2 | G0/0 | 192.168.1.1/24 |
| R1 | | G0/1 | 203.0.113.1/30 |
| ISP | Router-2 | G0/0 | 203.0.113.2/30 |
| ISP | | G0/1 | 198.51.100.2/30 |
| R2 | Router-2 | G0/1 | 198.51.100.1/30 |
| R2 | | G0/0 | 192.168.2.1/24 |
| PC1 | PC | Fa0 | 192.168.1.10/24 |
| PC2 | PC | Fa0 | 192.168.2.10/24 |

```lab
device.add {"model": "Router-2", "name": "R1"}
device.add {"model": "Router-2", "name": "ISP"}
device.add {"model": "Router-2", "name": "R2"}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
link.add {"a": "R1", "a_port": "g0/1", "b": "ISP", "b_port": "g0/0"}
link.add {"a": "ISP", "a_port": "g0/1", "b": "R2", "b_port": "g0/1"}
link.add {"a": "PC1", "b": "R1", "b_port": "g0/0"}
link.add {"a": "PC2", "b": "R2", "b_port": "g0/0"}
```

```console PC1
ipconfig 192.168.1.10 255.255.255.0 192.168.1.1
```

```console PC2
ipconfig 192.168.2.10 255.255.255.0 192.168.2.1
```

```console ISP
enable
configure terminal
interface g0/0
ip address 203.0.113.2 255.255.255.252
no shutdown
interface g0/1
ip address 198.51.100.2 255.255.255.252
no shutdown
end
```

```console R1
enable
configure terminal
interface g0/0
ip address 192.168.1.1 255.255.255.0
no shutdown
interface g0/1
ip address 203.0.113.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 203.0.113.2
end
```

```console R2
enable
configure terminal
interface g0/0
ip address 192.168.2.1 255.255.255.0
no shutdown
interface g0/1
ip address 198.51.100.1 255.255.255.252
no shutdown
exit
ip route 0.0.0.0 0.0.0.0 198.51.100.2
end
```

## Part 1 — The authority

The provider's router is the certificate authority. Its certificates last one day, so that a
short wait in the simulator is enough to expire them. Every name in this lab is a made-up one.

```console ISP
configure terminal
ip http server
crypto pki server ROOT
issuer-name CN=Lab Root, O=NetLab
lifetime certificate 1
grant auto
no shutdown
end
```

```verify
ISP# show crypto pki server
  expect: Certificate authority ROOT: running
```

Each site trusts the authority (`crypto pki authenticate` fetches its certificate) and asks for a
certificate of its own (`crypto pki enroll`).

```console R1
configure terminal
crypto pki trustpoint T
enrollment url http://203.0.113.2
subject-name CN=r1.lab.test, O=NetLab
exit
crypto pki authenticate T
crypto pki enroll T
end
```

```console R2
configure terminal
crypto pki trustpoint T
enrollment url http://198.51.100.2
subject-name CN=r2.lab.test, O=NetLab
exit
crypto pki authenticate T
crypto pki enroll T
end
```

```verify
R1# show crypto pki certificates
  expect: Certificate of trustpoint T
R2# show crypto pki certificates
  expect: Certificate of trustpoint T
```

## Part 2 — IKE with signatures

The policy says `authentication rsa-sig`: there is no `crypto isakmp key` anywhere. Everything
else is what the pre-shared-key labs use.

```console R1
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha256
authentication rsa-sig
group 14
exit
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
access-list 110 permit ip 192.168.1.0 0.0.0.255 192.168.2.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 198.51.100.1
set transform-set TS
match address 110
exit
interface g0/1
crypto map VPN
end
```

```console R2
configure terminal
crypto isakmp policy 10
encryption aes 256
hash sha256
authentication rsa-sig
group 14
exit
crypto ipsec transform-set TS esp-aes esp-sha-hmac
exit
access-list 110 permit ip 192.168.2.0 0.0.0.255 192.168.1.0 0.0.0.255
crypto map VPN 10 ipsec-isakmp
set peer 203.0.113.1
set transform-set TS
match address 110
exit
interface g0/1
crypto map VPN
end
```

```verify
ping PC1 192.168.2.10 => success
R1# show crypto isakmp sa
  expect: RSA signature
  expect: peer certificate CN=r2.lab.test, O=NetLab
R2# show crypto isakmp sa
  expect: peer certificate CN=r1.lab.test, O=NetLab
R1# show crypto isakmp policy
  expect: authentication rsa-sig
```

Messages 3 and 4 of main mode are in the clear and now end with a certificate request each, one
per authority the sender trusts. Messages 5 and 6 are encrypted and carry the sender's certificate,
then the authority's, and a signature over the exchange's hash.

```verify
events: CERTREQ
```

## Part 3 — The certificates expire

A day passes. The simulator's clock is each router's clock, so the certificates are now past
their end. The tunnel that is up keeps working until its keys need renewing; a new exchange does
not.

```lab
sim.advance {"seconds": 86400}
sim.advance {"seconds": 86400}
```

```console R1
clear crypto sa
```

```console R2
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => failure
R2# show crypto isakmp sa
  expect: failed
  expect: expired
```

The router that checks a certificate says so: here R2 refused R1's, and R1 only knows that the
peer refused it. Nothing is wrong with the configuration; the certificates are the problem, and
only the authority can give new ones.

## Part 4 — Enrol again

`crypto pki enroll` asks the authority again, and the new certificate replaces the old one.

```console R1
configure terminal
crypto pki enroll T
end
clear crypto sa
```

```console R2
configure terminal
crypto pki enroll T
end
clear crypto sa
```

```lab
sim.advance {"seconds": 5}
```

```verify
ping PC1 192.168.2.10 => success
R1# show crypto isakmp sa
  expect: RSA signature
```

## Revoking a certificate

An authority can also withdraw a certificate before its end: `crypto pki server ROOT revoke
SERIAL` (the serial is in `show crypto pki certificates`), and a router that sets `revocation-check
crl` on its trustpoint and fetches the list with `crypto pki crl request T` refuses it with the
name of the authority that revoked it. Try it on R2's certificate, and enrol again to repair it.

```text
R1(config)# crypto pki trustpoint T
R1(config-ca-trustpoint)# revocation-check crl
R1(config-ca-trustpoint)# exit
R1(config)# crypto pki crl request T
```

## What you learned

- `authentication rsa-sig` replaces the shared key with a certificate, a signature and a trust
  anchor: the other router's certificate must chain to an authority this router trusts.
- Each end asks for certificates from the authorities it trusts, and answers with a chain up to
  its own.
- The refusal is made, and worded, by the router that checks: read both ends.
- A certificate has a lifetime; enrolling again is the repair.
