# Decisions

## Clean room, no reverse engineering

NetLabCraft is built from public standards and black-box observation only. The reference app's licence forbids reverse engineering, and the storytold Craft apps forbid reading vendor binaries; both rule out decompiling it or its file format.

## No `.pkt` import for now

Reading the reference app's encrypted, compressed lab format would mean either reverse engineering it or using third-party work derived from that. Out of scope until a lawyer-reviewed plan exists; labs use the open `.nlcraft` format.

## Console vocabulary, our own words

Command names and configuration syntax are kept (they are what students learn and what configs are made of); prompts follow the usual `name>` / `name#` / `name(config)#` convention. All output text, table layouts and help strings are original.

## The FreeBSD package is unsigned and untested on hardware

The release ships a FreeBSD `.pkg` and `.tar.gz` built by the `freebsd` CI job inside a FreeBSD 14 VM (no FreeBSD runners exist). The VM has no GPU, so CI proves that it builds, packages, installs with `pkg add` and that the CLI runs, not that the window opens. The package is not signed (no package repository, no signing key), and the maintainer has not yet run it on real FreeBSD hardware. The README says so; it counts as supported once someone has run it.

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

## The smart things share one room with simple physics

The IoT things share one room per lab (`Topology::environment`), with its temperature, humidity, soil moisture, smoke, CO and light. While `enabled`, the devices that are on change it and the sensors read it, so a rule such as "thermostat below 19: heater on" closes a loop in simulated time. It is off by default: older labs and samples set each sensor's reading by hand and must keep behaving that way, and a teacher can set it up as an exercise. When it is on, setting a sensor's reading sets the room's quantity instead.

The model is meant to be explainable and exact, not a thermal simulation. Every quantity is an integer number of thousandths and a step is one second of simulated time, so results are identical on every platform and nothing drifts; leaks toward the outside move at least a thousandth a step, so a room arrives at the outside value exactly. A heater adds 1.2 degrees a minute only below 40 degrees, an air conditioner removes 1.5 only above 10, an open window or garage makes the leak six times faster and lets 3/4 of the daylight in, a sprinkler wets the air and the soil, a fan clears smoke and CO. Light follows at once (no inertia). A long stretch is stepped at most a day at a time.

Honest limits: one room per lab (the physical view's buildings and closets are not rooms), no heat capacity, no day and night cycle or weather, no humidity-temperature coupling, no light sensor model, a door lock does nothing, and the values reached while running are not saved with the lab (a loaded lab starts from its starting values).

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

## RSA keys, X.509 certificates and a small CA are real; TLS and SSH use them when a router has them

`wire/src/rsa.rs` generates RSA keys (512 to 2048 bits, e = 65537; primes by a bounded search with trial division and six Miller-Rabin rounds over our own big integers) from a seeded generator and signs with PKCS#1 v1.5 over SHA-256 or SHA-1 (RFC 8017); `wire/src/x509.rs` builds and reads X.509 v3 certificates and PKCS #10 requests over a strict DER codec and verifies chains (RFC 5280: signatures, validity, basicConstraints and path length, trust anchors). Keys are deterministic: a router's seed is its identity, the key label, the size and how many keys it made before, so a lab replays the same bytes; that is right for teaching and wrong for protecting anything, and the arithmetic is not constant-time. A 2048-bit key is generated in about 0.1 s here, so every size is allowed; sizes below 512 bits and above 2048 are refused rather than faked (the console used to accept 360 to 4096 and only record the number). The CA (`crypto pki server`) signs automatically and over HTTP on the simulated network; there is no manual approval, revocation, certificate pasting or SCEP. Keys and certificates live in the lab file next to the device and not in `show running-config`.

RESTCONF's HTTPS server speaks `TLS_DHE_RSA_WITH_AES_128_CBC_SHA` when the router has `ip http secure-trustpoint` naming a trustpoint with a certificate and its key: the Certificate message carries the chain and the ServerKeyExchange is signed with the key (RFC 5246 §7.4.2-7.4.3). A client always checks that signature against the certificate it was sent; with trust anchors (`trust` on `pc.http_request`) it also checks the chain and refuses an untrusted, expired or not-yet-valid one with the matching alert. Without a trustpoint the server keeps the anonymous suite (`TLS_DH_anon_WITH_AES_128_CBC_SHA`) so existing labs run as written, a server with a certificate still serves a client that offers only the anonymous suite, and a client that demands verification refuses an anonymous server. Not done: revocation, host-name matching (names are for people here), client certificates, session resumption, an RSA key exchange. NETCONF's SSH server signs the exchange hash with the router's `NAME.DOMAIN` RSA key (`ssh-rsa` blob, `rsa-sha2-256` signature, RFC 4253 §8 and RFC 8332) and the client verifies it against the blob it was sent; there is no known-hosts list to compare with, and a router without the key keeps the old stand-in, an HMAC where the signature goes. Both stand-ins stay documented in `wire/src/tls.rs` and `wire/src/ssh.rs`.

## PEAP's key exchange uses a toy group

The PEAP tunnel of WPA2-Enterprise has TLS's shape (records, a handshake, a PRF, AES-CBC with HMAC-SHA1) and its secrecy is real against someone watching the frames, but its Diffie-Hellman runs modulo the Mersenne prime 2^61 − 1, far too small to resist a determined attacker, and the server names itself instead of showing a certificate (TLS under RESTCONF and SSH under NETCONF now can, see above; PEAP was not changed). It keeps the exchange quick and readable for teaching; it is not meant to protect anything.

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

## The cellular network is a small model of our own, not 3GPP

Smartphones and tablets have a cellular radio (a `Cellular0` port next to `Wireless0`), a **Cell Tower** (a hub-kind device with six sectors, `Cellular0-5`, and a backhaul port `Ethernet0`) carries their frames, and a **Central Office Server** (a router-kind device) admits them and gives each an address. Nothing of 3GPP signalling is modelled: no SIM, IMSI, authentication, bearers, handover or radio resource control, and the names are ours.

- **Coverage** is worked out from the lab, as Wi-Fi association is. A device attaches to the nearest tower in range that has a free sector (a tower has six; devices are placed in id order). Without a physical layout the range is 400 workspace units on the logical map; with one, the tower and the device must be in the same city and within 600 m on its map (a device in a building stands where the building's icon is). Signal is 1 to 4 bars by distance.
- **The attach is an exchange of real frames** of ours, `netlabcraft_wire::cell` (ethertype 0x88B5, the IEEE 802 local experimental one): the phone sends an *attach request* with its APN out of its cellular port, the tower bridges it to its backhaul, the central office answers *attach accept* (address, prefix, gateway) or *attach reject* (code and text), and the answer comes back the same way. They show in the event list (protocol `CELL`) and in captures. What the exchange says is decided from the lab (`Topology::cell_refresh`), and the phone's data frames do not wait for the answer: the address is written on `Cellular0` as soon as the lab admits the device, so a ping straight after a change does not race the exchange. This is the same trade as Wi-Fi association.
- **The server admits** a device when it has an address pool, one of its interfaces is in that network (that interface is the devices' gateway), the data plan is on, the APN matches (case-insensitive; empty on the server admits any) and the pool has an address left (the k-th host address in id order, skipping the gateway). Refusals say why (`show cellular`, `cell.status`, the phone's Settings).
- **Data path.** The server is an ordinary router: the phone ARPs for the gateway through the tower, and the server forwards between its cellular interface and the lab. The lab needs a route back to the pool (the sample's web server uses the server as its gateway). A tower never carries one device's frames to another's sector: sector traffic goes to the backhaul only, backhaul traffic to the sector it is addressed to (every sector for a broadcast); devices on one tower reach each other through the server, as through any router (the server answers ARP for the addresses it admitted, so a device that finds a neighbour in its own network still sends the packet to the server). No NAT is built in (a carrier-grade NAT could be configured with the router's own NAT).
- **Wi-Fi and cellular on one device.** A host uses its first interface that is up, and `Wireless0` comes before `Cellular0`, so a phone associated with an access point uses Wi-Fi for everything and falls back to cellular when Wi-Fi is gone, as phones do. A default route per interface is kept (the cellular one is the server's address, on the port); DNS from the server is not (browse by address or set DNS by hand on the Wi-Fi side).
- **The backhaul** is found through cables, hubs and switches from the tower's port to a central office interface; **VLANs on those switches are not looked at**. Tower and server are placed with `device.add`, joined with any copper cable; `cell.settings` (phone), `cell.core` (server, as console lines) and `cell.status` read and set the rest. The server also takes `cellular pool|apn|data-plan` and `show cellular` at its console, and phones and tablets `cellular [on|off|apn NAME]` in their Command Prompt.
- **The links are not saved.** `Topology::cell_links` is recomputed after every change (and on load) and the device's address is rewritten with it, so a saved lab holds only the settings (`cell`). Lab file version 3 (older files still open; their phones get the `Cellular0` port).

## The end-device firewall: first match wins, no match is dropped, answers pass

A PC, laptop or server has a Firewall and an IPv6 Firewall app (`Device::host_fw`, two lists, each with a switch). Off, everything is let in. On, the first rule that matches decides, and a packet no rule matches is dropped, as access lists do, so a firewall switched on with no rules blocks what starts a conversation. Only inbound traffic is judged, and only what starts something: ICMP echo requests, the first segment of a TCP connection (SYN without ACK) and UDP datagrams (by destination port). Echo replies, the rest of a TCP connection, ICMP errors, neighbour discovery and the answers to DHCP and DNS requests always pass, so a device with its firewall on can still use the network without the simulator tracking connections (that is the firewall appliance's job). A dropped packet gets no answer, as with a real stealth rule. A rule has no direction because the list is inbound only; the apps show "Inbound". Lab file version 5.

The Text Editor keeps its files in the host's existing `files` list (what `dir` and `ftp` use), bounded to 32 files of 16 KiB, so a file written in the editor can be sent over FTP.

## The Traffic Generator is a PDU burst that runs to its end; PPPoE is one step over the cable

The Traffic Generator sends its packets through `Sim::send_pdu`, the machinery of the Complex PDU, so a burst takes the same path and meets the same firewalls and access lists as any PDU, and shows in the event list. It runs inside its command and returns the counts: at most 100 packets, at most 5 simulated seconds apart, so it cannot stall the simulator, and the same lab gives the same result. The last burst of each device is kept in the session, not in the lab file. Because a closed UDP or TCP port is not an answer, a burst to a port nobody serves is counted as lost.

The PPPoE Dialer needed a server, and the simulator had none. A Server device gets a PPPoE service (`Services::pppoe`: on or off, PAP or CHAP, an address pool, accounts), set in its Services tab or with `server.services`. A dialer connects with `pppoe.connect`: the first port of the computer must be cabled straight to such a server (that is the discovery), the server checks the account (PAP compares the password; CHAP sends a challenge and compares MD5 answers as RFC 1994 describes, and the challenge comes from the two names and the simulated time, so a run is repeatable), and on success the computer gets the first free address of the pool that lies on the server port's subnet, that subnet's mask, and the server's address as default gateway. The session is kept on the computer (`Device::pppoe`) and shown by `ipconfig`; typing an address, `ipconfig /release` or `/renew` ends it. We did not model PADI, PADO, PADR, PADS or PPP frames: the exchange is one step in the engine, and the data then crosses the same Ethernet link as any traffic. What that leaves out is gaps.md item 8. Lab file version 6.

## `show interfaces` prints counts the simulator kept, and a load averaged since they began

Every frame that crosses a cable is counted at both ends (`Sim::l2_count`, bytes and unicast, multicast and broadcast frames in and out); `show interfaces` and `show interfaces counters` read the same counters, and `clear counters` zeroes them and remembers when. The reliability is 255/255 because nothing in the simulator corrupts a frame. The load is the average bit rate since the counters began (or since the lab started) over the port's bandwidth, 255 times that, between 1 and 255; the reference app averages over five minutes, and this needs no sampling clock and is the same on every run. Error, drop and keepalive lines are left out: nothing here produces input errors or runs keepalives, and a zero printed for them would be made up. The bandwidth and delay are the ones EIGRP uses (`bandwidth`, `delay`, else by port type), so the two views agree.

## NAT translations age out when the next packet is translated, by kind

A dynamic translation lives until it has been idle for its timeout: 86,400 s for an address alone (and TCP flows), 300 s for UDP flows and 60 s for ICMP flows, the usual defaults, changed with `ip nat translation timeout`, `udp-timeout`, `icmp-timeout` and `tcp-timeout`. The simulator has no timer per translation: an expired one is dropped from the table when the next packet is translated (either direction) and is left out of `show ip nat translations`, `show ip nat statistics` and the ICMP-error lookup from then on, so what a person sees never depends on whether a packet came by. Static entries never expire. TCP's `finrst-timeout` and `syn-timeout` are not there: no translation follows its connection's state.

## `show ip route summary` prints a memory estimate that is a count times a fixed size

The simulator keeps no routing-table memory pool, so the line at the end of `show ip route summary` is the number of routes times 160 bytes, and says it is a simulated estimate and not a measurement. It is the same on every run and moves with the table, which is what a lab asks of it (compare before and after a summary or a filter); it is not comparable with a real router's figure.

## The OSPF database is derived, so its sequence numbers stay fixed and it shows no ages

`show ip ospf database` is computed from the converged state (`ospf_lsdb::database`): there is no flooding of single LSAs, so nothing is ever originated again or aged, and a sequence number or an age printed for an LSA would be made up. They stay at the first sequence number and are not shown. What the database can say truthfully it does: which router originates which LSA (`self-originate`, `adv-router`) and the type 4 LSAs an area border router would originate for an AS boundary router, with the metric from its own interface or route costs. Real ages and sequence numbers need an LSA history (a record per LSA of when its content last changed), which the derivation does not keep.

## BGP sessions are worked out from the lab, carried by real messages in TCP segments, and take three seconds to come up

`router bgp` follows the other routing protocols here: what the routers would agree on is computed from the lab (`netlabcraft_sim::bgp`), not run as one state machine per router. A session exists when a TCP connection could be opened, which means the neighbour's address resolves in the routing table **without BGP's own routes** (connected, static, OSPF, EIGRP and RIP), the other router has a `neighbor` statement for the address the connection comes from (`update-source` decides it), the AS numbers agree on both sides, and an external peer is directly connected unless `ebgp-multihop` allows the distance. Paths then flow in rounds until nothing changes: best path by weight, local preference, locally originated, shortest AS path, origin, MED (only between paths from the same neighbouring AS), external over internal, lowest router id, lowest neighbour address. External neighbours get the AS prepended and the sender as the next hop, internal ones get the path as it is (the next hop too, unless `next-hop-self`), a path learned from an internal neighbour is never passed to another (no route reflectors), and a path whose AS path holds the receiver's AS is dropped. A path is usable only while its next hop resolves. BGP's routes enter the routing table as `B` at distance 20 (external) or 200 (internal), one best path per prefix.

The simulator's TCP carries short request and answer exchanges, not long-lived connections with state, so the sessions do not use it. Each session instead puts **real BGP messages** (RFC 4271: OPEN with the four-octet AS capability, KEEPALIVE, UPDATE, NOTIFICATION) on the wire inside real TCP segments to port 179, addressed and routed as the segments of any other host (TTL 255 for internal sessions, the multihop value for external ones). The three-way handshake and acknowledgement of each segment are **not** simulated; sequence numbers count the bytes each side has sent. A router that has a BGP process takes such a segment without answering it with a reset, counts it by kind (`show ip bgp neighbors`) and, with `debug ip bgp`, logs it. The messages are on the wire at the times a real exchange would send them: OPEN a second after the session becomes possible, KEEPALIVE a second later, the UPDATEs when it is Established, a KEEPALIVE every 60 s, an UPDATE with what changed afterwards, and a NOTIFICATION from `clear ip bgp`.

A session spends 3 simulated seconds going from Connect to OpenSent (after 1 s) and OpenConfirm (after 2 s) to Established; its routes exist only from then, so a lab has to let time pass after configuring it (`sim.advance`). This is a fixed delay, not a model of the TCP and OPEN exchange. A session ends **at once** when the path to its neighbour goes (a cable, an interface shut, a route lost): there is no hold timer to wait out, so a failed link takes the routes with it as `fast external fallover` would, and the same holds for internal sessions. Session counters count the messages that were put on the wire and the ones that arrived. The window pitfall of control traffic applies to keepalives (see [pitfalls.md](pitfalls.md)).

Not done, and listed in [gaps.md](gaps.md): route maps and prefix lists (they do not exist in the codebase yet), communities, route reflectors and confederations, aggregation, `maximum-paths`, the IGP metric to the next hop in the best-path order, `neighbor … password` (the simulator has no TCP MD5), custom timers, address families and IPv6.

## Prefix lists and route maps are plain configuration the simulator reads where policy applies

`ip prefix-list` and `route-map` are stored on the router (`netlabcraft_model::policy`) and answered by one function, `policy::apply(device, prefix_list, route_map, subject)`: `None` when the route is dropped, else the attributes to change. The simulator calls it where a route crosses a boundary: when a BGP router imports from and exports to a neighbour, when it originates a `redistribute`d route, and when OSPF takes static routes for redistribution. Nothing is cached: the results are recomputed from the lab whenever the routing fingerprint changes (it hashes the policy), so a policy edit applies at once and `clear ip bgp … soft` has nothing to reset (the command is accepted and says so, because courses type it).

The rules follow the usual ones. A prefix list's lines are kept in sequence order (numbered in steps of five when no number is typed), the first matching line decides, and a prefix no line matches is denied. A line without `ge` or `le` matches that prefix only; `le` alone covers lengths from the prefix's own up to `le`, and `ge` alone from `ge` to 32. A route map's clauses are tried in sequence order and the first whose `match` lines hold decides: a permit clause applies its `set` lines, a deny clause drops the route, and a route no clause matches is dropped. Several names on one `match ip address` line are alternatives; a clause with no `match` line matches everything. A standard access list named in a `match` is asked about the route's network address, an extended one about the address and the mask (as a source and a destination).

Two choices are ours. A prefix list or route map that a neighbour or a redistribution names but that is not defined **lets everything through** (the names are typed in either order, and the running configuration lists neighbours before the maps, so the replay at a reload must not drop routes); `show ip bgp neighbors` says "not defined" so the misspelt name is easy to find. A prefix list or ACL named in a `match` that does not exist **matches nothing**. In a BGP neighbour's route map, `set weight` and `set local-preference` act on the path the router imports (and local preference on what it sends an internal neighbour); toward an external neighbour they have nothing to act on, and are ignored. `set as-path prepend` on an outbound map is applied before the router adds its own AS, so `set as-path prepend 65001 65001` on AS 65001's outbound map gives three copies. OSPF's `redistribute static … route-map` only filters: redistributed routes keep metric 20, so a `set metric` there has no effect.

What is not modelled is in [gaps.md](gaps.md): `match` on AS paths, communities, interfaces, tags and next hops, `set community` and `set tag`, and `distribute-list`.

## The VPN Client talks to a remote-access router in a protocol of our own, over a real ESP tunnel

A PC's VPN Client needs a concentrator, so a router can now be set up as a remote-access (Easy VPN style) server with the console commands a course uses: `ip local pool`, `crypto isakmp client configuration group NAME` (`key`, `pool`), `crypto dynamic-map NAME SEQ` (`set transform-set`, `reverse-route`), `crypto map NAME client authentication list`, `isakmp authorization list` and `client configuration address`, `crypto map NAME SEQ ipsec-isakmp dynamic DYNAMIC`, `aaa authentication login LIST local` and `aaa authorization network LIST local`, the map applied to the interface the clients reach, and `username` lines for the accounts. The configuration lives in `Crypto::ra` and three fields of `CryptoMapEntry`; dynamic maps are crypto map entries marked as templates, so `set transform-set` and the sub-mode are the site-to-site ones.

**What is real:** the tunnel. After Connect, every packet a PC sends from its VPN address (all that goes beyond its own subnets, except to the concentrator) is sealed as ESP (RFC 4303) with the algorithms of the dynamic map's first transform set (AES, DES, 3DES or NULL with HMAC, the same code the site-to-site IPsec uses), sent to the router, opened, checked to come from the address the router gave, and routed as if it had arrived on that interface; packets for the client's address are sealed back. The request carries the group name and signs itself with the group key (HMAC-SHA-256), so a wrong group or key is refused; the password travels hidden under a stream derived from the key and a nonce and is checked against the router's `username` list, but only when the crypto map has a client authentication list (without one the router asks for no user, as a real one skips Xauth); the answer is signed with the group key, so a client rejects an answer from a router that does not know it; the keys of both directions come from the group key and the two nonces.

**What differs from the real protocol, honestly:** the exchange is not IKE. The real one runs ISAKMP phase 1 (aggressive mode with the group name as identity, then Xauth for the user name and mode-config to hand out the address) and quick mode on UDP 500; ours is one request and one answer on UDP port 10000 (the port Cisco's IPsec over UDP uses), in a format of our own, so `crypto isakmp policy` is not consulted, no `show crypto isakmp sa` entry has cookies, and a capture shows our messages, not ISAKMP. The nonces and SPIs come from a hash of the inputs, so a run is repeatable. There is no split tunnel (the client's group has no access list), no DNS or domain pushed, no rekey or lifetime, no dead-peer detection, and the session is not saved with the lab. The router does not apply NAT or outbound access lists to the sealed packets, and `reverse-route` is accepted but injects no route: a router behind the concentrator needs a static route to the pool. What is left is gaps.md item 11. Lab file version 7.
