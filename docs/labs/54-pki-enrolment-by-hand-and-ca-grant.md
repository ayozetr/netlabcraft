# Lab 54 — Enrolling by hand: a certificate authority that waits for the administrator

**Course:** CCNA 3 (Enterprise Networking, Security and Automation) · **Time:** 45 minutes

## Objectives

- Run a certificate authority that does not sign what it is asked: requests wait until an
  administrator grants them.
- Enrol a router with a request that has to wait, find the request on the authority, grant it,
  and fetch the certificate.
- Give the certificate the names a client checks (`fqdn` and `ip-address`), and read them back.
- Copy a certificate by hand with `enrollment terminal` when no network path exists.

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

## Part 1 — An authority that waits

Without `grant auto` the authority keeps every request until an administrator decides.

```console ISP
configure terminal
ip http server
crypto pki server ROOT
issuer-name CN=Lab Root, O=NetLab
no shutdown
end
```

```verify
ISP# show crypto pki server
  expect: Certificate authority ROOT: running
  expect: requests wait for
```

## Part 2 — A request that waits

R1 trusts the authority and asks for a certificate that also names its host and its address.
The authority answers that the request is waiting.

```console R1
configure terminal
crypto pki trustpoint LAB
enrollment url http://203.0.113.2
subject-name CN=r1.lab.test, O=NetLab
fqdn r1.lab.test
ip-address 203.0.113.1
exit
crypto pki authenticate LAB
crypto pki enroll LAB
end
```

```verify
R1# show crypto pki certificates
  reject: Certificate of trustpoint LAB
ISP# show crypto pki server
  expect: 1: CN=r1.lab.test, O=NetLab (waiting)
```

Asking again is the same request, not a second one: run `crypto pki enroll LAB` once more and
look at the list.

```console R1
configure terminal
crypto pki enroll LAB
end
```

```verify
ISP# show crypto pki server
  expect: 1: CN=r1.lab.test, O=NetLab (waiting)
```

## Part 3 — The administrator grants it

The authority's console shows the request. Granting it signs the certificate; the router fetches
it the next time it asks.

```console ISP
configure terminal
crypto pki server ROOT grant 1
end
```

```console R1
configure terminal
crypto pki enroll LAB
end
```

```verify
R1# show crypto pki certificates
  expect: Certificate of trustpoint LAB
  expect: Also for      : r1.lab.test, 203.0.113.1
R1# show running-config
  expect: fqdn r1.lab.test
  expect: ip-address 203.0.113.1
ISP# show crypto pki server
  expect: 1: CN=r1.lab.test, O=NetLab (granted)
```

The certificate carries a subjectAltName with the host name and the address. A client that checks
the name it connected to (`pc.http_request` with `verify_host`, or IKE checking an identity
against the certificate) accepts those and refuses any other.

## Part 4 — No network path: copy it by hand

A trustpoint with `enrollment terminal` has no URL: `crypto pki enroll` prints a PKCS #10
request to copy, the authority takes it with `crypto pki server ROOT request pkcs10 terminal` and
prints the certificate, and the router takes that with `crypto pki import NAME certificate`.
`crypto pki authenticate NAME` takes the authority's own certificate the same way. What is pasted
is PEM text, which differs every time, so follow this part by hand:

```text
R2(config)# crypto pki trustpoint HAND
R2(ca-trustpoint)# enrollment terminal
R2(ca-trustpoint)# subject-name CN=r2.lab.test, O=NetLab
R2(ca-trustpoint)# exit
R2(config)# crypto pki authenticate HAND
  (paste the authority's certificate, the text between BEGIN CERTIFICATE and END CERTIFICATE)
R2(config)# crypto pki enroll HAND
  (copy the request it prints)
ISP(config)# crypto pki server ROOT request pkcs10 terminal
  (paste the request; the certificate comes back)
R2(config)# crypto pki import HAND certificate
  (paste the certificate)
```

## What you learned

- An authority that does not `grant auto` holds requests; the administrator sees them in `show
  crypto pki server` and signs them with `grant`.
- Enrolling again before the grant is the same request; after it, the router gets the
  certificate.
- `fqdn` and `ip-address` end up in the certificate as names a client can check.
- `enrollment terminal` is the same exchange done by copying text.
