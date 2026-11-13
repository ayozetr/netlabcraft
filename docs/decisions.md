# Decisions

## Clean room, no reverse engineering

NetLabCraft is built from public standards and black-box observation only. The reference app's licence forbids reverse engineering, and the storytold Craft apps forbid reading vendor binaries; both rule out decompiling it or its file format.

## No `.pkt` import for now

Reading the reference app's encrypted, compressed lab format would mean either reverse engineering it or using third-party work derived from that. Out of scope until a lawyer-reviewed plan exists; labs use the open `.nlcraft` format.

## Console vocabulary, our own words

Command names and configuration syntax are kept (they are what students learn and what configs are made of); prompts follow the usual `name>` / `name#` / `name(config)#` convention. All output text, table layouts and help strings are original.

## Real frames, simulated time

Devices exchange encoded frames over a discrete-event queue in microseconds. Deterministic, wasm-safe, and captures are valid pcap.

## Routers drop the packet that starts ARP; hosts queue it

Matches what learners see on real equipment (the first ping across a router times out).

## Proxy ARP on by default on routers

Proxy ARP on by default on routers, so a static route out of an Ethernet interface works, as on common router software.

## Loop guard instead of silent hangs

A frame crossing 64 switches is dropped and the UI reports a switching loop (spanning tree normally prevents it).

## Generic device kinds

No vendor model names or pictures; ports are named with the usual `GigabitEthernet0/0` / `FastEthernet0/1` / `Serial0/0/0` convention.

## Model names describe their ports

The workspace shows names such as `Router 3GE` or `Switch 24FE`; lab files keep the stable ids (`Router-3`, `Switch-24`) and commands accept both. Vendor model names appear only in [model-equivalents.md](model-equivalents.md), to tell students which model to place for a course lab, never as names of NetLabCraft models.

## Same arrangement, own artwork

Like WordCraft with Word, the window follows the reference app's layout and workflow (where things are, what tabs a device window has, the Realtime/Simulation switch, the PDU list), observed black-box. Icons, device pictures, colours' exact values and all wording are ours, drawn in code.

## One click opens a device

One click opens a device, as in the reference app (a double click was the first version).

## Computed protocols

Spanning tree, OSPF, RIP, EtherChannel negotiation and VTP are computed from the topology the way they converge, instead of exchanging timed hellos. Labs are correct at once and deterministic. Their frames (BPDUs, hellos, updates, LACPDUs) are still sent at the usual intervals so Simulation mode and captures show them, but they report what the computation decided rather than drive it.

## Switches flush learned addresses on a topology change

Switches flush learned addresses on a topology change, as a topology-change notification makes real switches do, so no address points at a path that is gone.

## The VLAN database and VTP settings survive a reload

The VLAN database and VTP settings survive a reload, as they live apart from the configuration on real switches. A VTP client's VLANs are not written into its running-config.

## IPv6 neighbour discovery holds the packet

IPv6 neighbour discovery holds the packet, on routers too, so the first IPv6 ping isn't lost (unlike IPv4 ARP on routers).

## Both RIP versions behave classless

`network` statements are classful, as typed, but subnets are advertised as they are; automatic summarisation is stored and not simulated.

## Cable types

Links carry a kind: automatic (resolves to the right kind for the two ports, today's behaviour and the default in old files), copper straight-through, copper cross-over, fiber, console, serial DCE, serial DTE. Ports have a medium (copper, fiber, combo, serial, wireless, console, RS-232) and a cable only joins ports of its medium. Routers and switches do auto-MDIX unless `no mdix auto`; hosts and hubs don't, so a wrong copper cable leaves the link down and says why. Serial DTE puts the DCE at the far end. Console cables join a PC's RS-232 port to a device's Console port and are not network links. Copper reaches 100 m, fiber 2000 m.

## Renamed to NetLabCraft

The earlier name belongs to an unrelated company with a registered trademark in software services, and names built on "Packet" or "Tracer" lean on the reference app's name. Labs are `.nlcraft` with `"format": "netlabcraft"`; no files with the earlier name were ever shared, so none are read.

## Serial frames are captured as Ethernet II

The wire carries HDLC or PPP; the event list and pcap show the equivalent Ethernet II frame, marked with its framing, because a pcap file has one link type.

## Multilayer switching is a model, not a mode

`ip routing` and routed ports are only accepted on the Switch-ML-24 model; other switches say why.

## Control frames show what the computed protocols say

BPDUs (every 2 s), OSPF hellos (10 s) and RIP updates (30 s) go on the wire in their standard formats so Simulation mode and captures have them; receivers take them in quietly and the protocols stay computed. CDP frames are left out because their format isn't published as a standard.

## Spanning tree takes its time

Trees are still computed at once, but a port that starts forwarding waits 30 s (listening, learning) unless portfast or rapid-pvst, as students see on real equipment. Sample labs start converged.

## Documentation follows the shared Craft format

Every page carries a review line and a revision history; parity is assessed with written weights and labelled estimates.

## Releases are unsigned for now

The owner's hold on releases is lifted: GitHub Actions works and `release.yml` builds the packages ([releasing.md](releasing.md)). They are not code-signed or notarized, because the project has no certificates: Windows and macOS warn on first run. Linux users can also install from source with `packaging/linux/install.sh`.

## Powered devices start without an adapter

IP phones and lightweight APs placed in a lab have no power adapter plugged in: they boot only on a PoE switch port within its budget, or once their adapter is plugged in from the Physical tab. That is how students meet power over Ethernet in the course labs, and a dark device says why it has no power. Labs saved before power existed have no `adapter` field, and their phones and APs count as plugged in, so they open and work as before.

## SIP phones are known by their MAC address

A SIP phone registers with its MAC address as the user part, and the router finds the `voice register pool` whose `id mac` matches it. The router names the phone's number in the `P-Associated-URI` header of its `200 OK` (RFC 3455), so the phone learns its number (the pool's `number 1 dn N`) from the registration; nobody types a number into the phone.

## IoT timers run in simulated time

Things report on their interval only as simulated time advances (`sim.advance`, the play controls, waiting for a ping), never by the wall clock, so runs stay deterministic and a paused lab stays still. A thing also reports at once when its reading changes, so rules react without waiting for the timer.

## Pings don't set "don't fragment" unless asked

Host and router pings go out without DF, as on common operating systems and router software; `ping -f` on hosts and the `df-bit` option of a router's extended ping set it. Echo replies carry no DF, and GRE copies DF from the inner packet. With `ip mtu` in the path, a plain ping is fragmented and a DF ping gets "fragmentation needed", which is what MTU labs show.

## Diffie-Hellman (MODP and elliptic-curve), AES, DES, 3DES and AH are real; little is left as a stand-in

IKEv1 and IKEv2 compute a real Diffie-Hellman exchange over the MODP groups the courses use (1, 2, 5, 14, 15, 16 and 24), with big integers of our own (Montgomery multiplication, windowed exponentiation), the primes built from their RFC definition and checked against the RFC digits, and over the NIST curves P-256 and P-384 (groups 19 and 20, RFC 5903) on the same big integers, checked against the RFC's published exchanges, with peer points validated and loops bounded. Keys follow RFC 2409 and RFC 7296, and AES (128, 192 or 256 bits), DES and three-key 3DES (FIPS 46-3 and SP 800-67, tested against the published known-answer vectors) with SHA-1, SHA-256 or MD5 HMACs encrypt and authenticate IKE and ESP for real, so a wrong pre-shared key or transform fails where it would on real routers. AH (RFC 4302, tunnel mode, HMAC-SHA1-96, HMAC-MD5-96 or HMAC-SHA-256-128 over the packet with its mutable fields zeroed) is real when a transform set holds only AH transforms, and bundled with ESP in one set it goes outside ESP (RFC 4301 §4.5: ESP in tunnel mode, AH in transport mode over the ESP packet); IKE SAs have lifetimes and are replaced before they run out (RFC 2409 main mode under the old SA; RFC 7296 §1.3.2 and §2.18). Group 24 (RFC 5114 §2.3) is a real group with its subgroup checked, a transform set with no cipher keeps the stand-in cipher, and `show crypto ipsec sa` and `show crypto isakmp sa` say so. They are rare in today's course labs, and the time is better spent elsewhere; nothing here is meant to protect real traffic (DES is broken and 3DES deprecated, and the table-lookup code is not constant time).

## The CCNA lab book is our own and runs as a test

The courses' own Packet Tracer activities can't be copied, so the lab book in `docs/labs/` is written from scratch over the same topics. Each lab is also a script: its `lab`, `console` and `verify` blocks run as written in the test suite, and several over MCP, so a change that breaks a lab fails the build and the book can't drift from what the app does.

## Answer networks are sealed by one of three schemes

An activity keeps its answer network in the lab file, sealed in one of three ways, and the Activity Wizard says which in plain words. `plain` keeps it readable, for open practice labs. `salt` encrypts it under a key from the activity's salt: the salt is in the same file and the code is open, so this only keeps it out of sight of a casual look. `password` encrypts it with AES-128-CTR and an HMAC-SHA1 tag under keys from PBKDF2 of the instructor's lock password (20 000 rounds), which is as strong as the password is hard to guess. New lock passwords are kept as PBKDF2 hashes too. We would rather state the strength honestly than promise secrecy a file format can't give.

## Results are checked by replay, not by a secret

An exported results file carries an HMAC-SHA1 under a key from the activity's salt. That key is in the lab file the student has, so it catches a score edited in a text editor and nothing more: anyone who reads the open code can sign a perfect score. A symmetric key shared with the student's app can never be secret from that student, so we did not add another one. Instead a JSON results file (version 2) embeds the student's final network, in the lab file format, and `activity.verify_results` runs the activity's checks again against it, on a fresh simulator as a reopened lab has, and compares the score, the points per item and which items passed with what the file claims. A forged score fails unless the embedded network really earns it.

What this closes: claiming a score the network does not earn, with or without a valid signature. What it does not close: a student can still solve the activity, or edit the network until it earns the points, which is just solving it; one student can send another's network (the file names the student, but nothing proves who built the network); and **time claims cannot be replayed**: `time` and `elapsed_s` are taken as written, so a time limit is not enforced by the file. A CSV file cannot carry a network, so it is only checked for hand edits, and so is a version 1 file from before this change; `verify_results` says so in `replayed: false` and a message. The embedded network is hostile input: it is size-capped (4 MiB), sanitized like any lab file, and the replay runs the same simulator under its usual caps (`MAX_EVENTS_PER_RUN`, bounded pings), so a garbage or oversized network is refused with a reason and never crashes. The replay starts from a fresh simulator, so a check that depends on state a live session built up (for example a protocol that had converged) can differ from the student's window; `activity.export_results` replays its own file and reports `replay_agrees` so the student hears before handing it in.

## RSA keys, X.509 certificates and a small CA are real (not yet used by TLS, SSH or IKE)

`wire/src/rsa.rs` generates RSA keys (512 to 2048 bits, e = 65537; primes by a bounded search with trial division and six Miller-Rabin rounds over our own big integers) from a seeded generator and signs with PKCS#1 v1.5 over SHA-256 or SHA-1 (RFC 8017); `wire/src/x509.rs` builds and reads X.509 v3 certificates and PKCS #10 requests over a strict DER codec and verifies chains (RFC 5280: signatures, validity, basicConstraints and path length, trust anchors). Keys are deterministic: a router's seed is its identity, the key label, the size and how many keys it made before, so a lab replays the same bytes; that is right for teaching and wrong for protecting anything, and the arithmetic is not constant-time. A 2048-bit key is generated in about 0.1 s here, so every size is allowed; sizes below 512 bits and above 2048 are refused rather than faked (the console used to accept 360 to 4096 and only record the number). The CA (`crypto pki server`) signs automatically and over HTTP on the simulated network; there is no manual approval, revocation, certificate pasting or SCEP. Keys and certificates live in the lab file next to the device and not in `show running-config`. This stage does not change TLS, SSH, PEAP or IKE: the next entry still describes them.

## TLS uses an anonymous Diffie-Hellman suite, without certificates

The router's secure web server speaks real TLS 1.2 with `TLS_DH_anon_WITH_AES_128_CBC_SHA` over the 2048-bit MODP group: the handshake, key derivation, Finished checks and record protection are real, so the event list and captures show an encrypted session. The suite is anonymous because a certificate would mean a PKI the labs don't have yet (keys, signatures, trust); NETCONF's SSH transport likewise uses an HMAC in place of a host-key signature. Both are documented stand-ins, and certificates are in [gaps.md](gaps.md).

## PEAP's key exchange uses a toy group

The PEAP tunnel of WPA2-Enterprise has TLS's shape (records, a handshake, a PRF, AES-CBC with HMAC-SHA1) and its secrecy is real against someone watching the frames, but its Diffie-Hellman runs modulo the Mersenne prime 2^61 − 1, far too small to resist a determined attacker, and the server names itself instead of showing a certificate. It keeps the exchange quick and readable for teaching; it is not meant to protect anything.

## Boards run BoardScript, not Python or JavaScript

Programmable boards run BoardScript, a small line-based language of our own (variables, conditions, loops, pins, sleep, reports to the IoT server), instead of Python or JavaScript. Embedding either interpreter would mean a large dependency (or JavaScript, which the project doesn't write by hand) and an open door into the simulator. BoardScript compiles to steps run in simulated time with a cap on steps per slice and on variables, text, requests and log, so a runaway program stops with a message instead of hanging a lab. Course programs have to be written again in it.

## Wi-Fi things join HomeNetwork when placed

A Wi-Fi smart thing comes out of the box set for `HomeNetwork`, the network a home gateway offers from the factory, so placing it next to a gateway is enough for it to join and register, as in the course's IoT labs. Another network is set as on any Wi-Fi host (`device.wireless`).

## Taking out the last power supply switches a device off

Routers and switches leave the factory with one supply in bay 1 of their one or two bays. Taking out a supply while another is fitted changes nothing; taking out the last one switches the device off as if its power switch were used, and it can't be switched on until a supply is fitted. Labs saved before supplies existed have no `psu` field and open with the factory supply.

## IoT custom cables are not network links

An IoT custom cable joins a board's pin to a smart thing. It is drawn on the workspace and listed with the cables, but it is stored with the board (`board.wires`), not in the lab's links: no frames cross it, nothing routes over it, and the board's program reads or switches the thing through it directly.

## Follow the Crafting Apps rules, except where the owner decided otherwise

NetLabCraft follows the storytold Crafting Apps rules (`craft-anything`'s `AGENTS.md`): never crash, safe Rust, other programs through one runner, one mutex type, no secrets, branches and pull requests, pitfalls and extending guides. One difference is deliberate: work is picked from `docs/gaps.md` with task ids (`M6.41`), not from roadmap items with claim lines, because the project has one maintainer. CI runs `cargo xtask ci` in GitHub Actions (`.github/workflows/ci.yml`) and locally.
