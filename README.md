# PacketCraft

An open-source network simulator in pure Rust: build labs with routers, switches, PCs and servers, configure them from a console with the commands network engineers already know, and watch every frame cross every cable.

PacketCraft follows the conventions of the storytold Crafting Apps. It is an independent, clean-room project, not affiliated with or derived from any network equipment vendor or its software.

## What works today

- **Devices:** routers, switches, a hub, PCs, laptops, servers and a printer (11 generic models).
- **Switching:** learning bridges with MAC aging, VLANs, access and 802.1Q trunk ports (native VLAN, allowed lists), per-VLAN spanning tree, EtherChannel (LACP, PAgP, static), VTP, port security, switch VLAN interfaces.
- **Routing:** connected, static and OSPF routes, longest-prefix match, proxy ARP, router-on-a-stick, ACLs, NAT and PAT, DHCP server and relay.
- **Services:** servers with HTTP (and a web browser on the PCs), DNS and DHCP; Telnet and SSH between devices.
- **ARP, IPv4, ICMP:** echo, TTL exceeded, unreachable; `ping` and `traceroute` with real timing in simulated time.
- **Console:** user / privileged / configuration / interface / subinterface / VLAN / line / DHCP / OSPF / ACL modes, passwords, abbreviations (`sh ip int br`), `?` help, Tab completion, history, `do`, `interface range`, `running-config` / `startup-config` / `write` / `reload`.
- **Host shell:** `ipconfig` (static or DHCP), `ping`, `tracert`, `nslookup`, `telnet`, `ssh`, `arp`.
- **Simulation mode:** an event list of every frame with drop reasons, a layer-by-layer decode and hex dump, a replay on the canvas, and **pcap export** for any packet analyser.
- **Labs:** saved as readable JSON (`.pcraft`), with undo/redo, notes, drawings and eleven sample labs.
- **Front ends:** desktop app, browser (WASM), `packetcraft-cli`, and an MCP server so AI agents can build and test labs.

See [`ROADMAP.md`](ROADMAP.md) and [`docs/target-app-parity.md`](docs/target-app-parity.md) for what is missing (IPv6, wireless, more server services…).

## Run it

```sh
cargo run --release -p packetcraft -- --sample vlans
cargo run --release -p packetcraft-cli -- exec --sample two-subnets PC1 'ping 10.0.2.10' 'tracert 10.0.2.10'
cd apps/packetcraft-web && trunk serve      # browser build
```

## Develop

```sh
cargo xtask ci        # fmt, clippy, tests, doctests, assets, layers, wasm, parity
```

Read [`AGENTS.md`](AGENTS.md) first: the clean-room and never-crash rules bind every contribution.

## Licence

MIT or Apache-2.0, at your option. See [`NOTICE`](NOTICE) and [`ATTRIBUTION.md`](ATTRIBUTION.md).
