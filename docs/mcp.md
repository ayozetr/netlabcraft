# MCP server

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (sessions for Simulation mode, automation and powered devices)

`netlabcraft-cli mcp [--in LAB] [--sample ID]` speaks the Model Context Protocol (JSON-RPC 2.0, one message per line) on stdin/stdout.

| Tool | Arguments | Does |
|---|---|---|
| `netlabcraft_run` | `command`, `params` | Runs any engine command ([`command-reference.md`](command-reference.md)). |
| `netlabcraft_cli` | `device`, `lines` | Types lines at a device console; returns each output and the final prompt. |
| `netlabcraft_lab` | — | Devices, ports, addresses, link state and cables. |
| `netlabcraft_commands` | — | The command catalog. |

A typical agent session: `file.new` (or `device.add` + `link.add`), configure with `netlabcraft_cli`, verify with `net.ping` / `net.traceroute`, inspect `sim.events`, then `file.save`.

Other sessions, all through `netlabcraft_run`:

- **Simulation mode:** `pdu.complex` (or `pdu.simple`) adds a PDU to the scenario in use and
  sends it; `pdu.list`, `pdu.fire`, `pdu.fire_all` and `pdu.delete` manage the list;
  `scenario.new`, `scenario.select`, `scenario.list` and `scenario.delete` keep several lists;
  `sim.advance` sends periodic PDUs as simulated time passes; `sim.event` returns one event's
  OSI view (`osi.in`, `osi.out`), what the receiver decided (`explain`) and the events it caused.
- **Automation:** `pc.http_request` sends a RESTCONF (or any HTTP) request from a host and
  returns the status, headers, body, parsed `json` and the console lines a router applied;
  `pc.http_saved` keeps requests in the lab; `netconf.session` runs NETCONF operations over
  port 830; `controller.devices` and `controller.push` manage several routers from one host.
  The managed routers' passwords are stored in clear in the lab file.
- **Powered devices and clouds:** `device.power.source` says where a phone's or a lightweight
  AP's power comes from, `device.adapter` plugs or unplugs its adapter, `phone.configure` sets
  a phone to SIP, and `cloud.map` pairs a cloud's ports.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Sessions for PDUs, scenarios and the OSI view, RESTCONF, NETCONF and the controller, powered devices and clouds. |
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
