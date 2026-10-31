# MCP server

> **Last reviewed:** 2026-10-31 · **Last updated:** 2026-10-31 · **Change:** minor (the lab book over MCP; sessions for Simulation-mode options, automation depth, boards, power supplies and instructor tools)

`netlabcraft-cli mcp [--in LAB] [--sample ID]` speaks the Model Context Protocol (JSON-RPC 2.0, one message per line) on stdin/stdout.

| Tool | Arguments | Does |
|---|---|---|
| `netlabcraft_run` | `command`, `params` | Runs any engine command ([`command-reference.md`](command-reference.md)). |
| `netlabcraft_cli` | `device`, `lines` | Types lines at a device console; returns each output and the final prompt. |
| `netlabcraft_lab` | — | Devices, ports, addresses, link state and cables. |
| `netlabcraft_commands` | — | The command catalog. |

A typical agent session: `file.new` (or `device.add` + `link.add`), configure with `netlabcraft_cli`, verify with `net.ping` / `net.traceroute`, inspect `sim.events`, then `file.save`.

The lab book ([labs/](labs/README.md)) is the worked example: `apps/netlabcraft-cli/tests/mcp_lab_book.rs`
starts `netlabcraft-cli mcp` and drives eight of its labs over JSON-RPC, each `lab` block as
`netlabcraft_run` calls and each `console` block as `netlabcraft_cli` calls.

Other sessions, all through `netlabcraft_run`:

- **Simulation mode:** `pdu.complex` (or `pdu.simple`) adds a PDU to the scenario in use and
  sends it; `pdu.list`, `pdu.fire`, `pdu.fire_all` and `pdu.delete` manage the list;
  `scenario.new`, `scenario.select`, `scenario.list` and `scenario.delete` keep several lists;
  `sim.advance` sends periodic PDUs as simulated time passes (and those with an absolute `at`);
  `sim.event` returns one event's OSI view (`osi.in`, `osi.out`, `osi.origin`), what the receiver
  decided (`explain`), what the sender decided (`origin`) and the events it caused;
  `sim.constant_delay` makes every hop take the same time and `sim.capture_filter` keeps only
  chosen protocols in the event list and the capture.
- **Automation:** `pc.http_request` sends a RESTCONF (or any HTTP) request from a host and
  returns the status, headers, body, parsed `json` and the console lines a router applied;
  `pc.http_saved` keeps requests in the lab; `netconf.session` runs NETCONF operations over
  port 830 over SSH, with `keep_open`, `session` and `close` to hold a session (and its locks)
  across calls; `controller.discover` finds routers over CDP and LLDP from a seed,
  `controller.devices` keeps the inventory and template variables, and `controller.push` fills
  templates per router. `pc.http_request` answers with `tls` when `https://` was used, and takes
  XML bodies and IPv6 URLs. The managed routers' passwords are stored in clear in the lab file.
- **Powered devices and clouds:** `device.power.source` says where a phone's or a lightweight
  AP's power comes from, `device.adapter` plugs or unplugs its adapter, `phone.configure` sets
  a phone to SIP and its address (DHCP or static), `device.psu` fits and takes out power
  supplies, and `cloud.map` pairs a cloud's ports.
- **Boards:** `board.program` sets and runs a BoardScript program, `board.status` returns its
  state, variables, pins and output, and `board.wire` (or `link.add` with
  `"cable": "iot-custom"`) joins a pin to a smart thing.
- **Instructor tools:** `activity.save_answer`, `activity.answer`, `activity.load_answer` and
  `activity.clear_answer` keep the answer network; `activity.tree`, `activity.weight` and
  `activity.tree_checks` build checks from it; `activity.save_initial`, `activity.load_initial`
  and `activity.clear_initial` set where the student starts; `activity.translate` adds
  instructions in a language; `activity.connectivity` runs the ping checks;
  `activity.export_results` and `activity.verify_results` write and check signed results;
  `activity.edit_check` changes a check's points or hint; `activity.locks` limits what a student
  may change while the activity is locked.
- **Services:** `server.syslog` returns the messages a server's syslog service received.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-31 | minor | The lab book over MCP; Simulation-mode options, kept NETCONF sessions, discovery and templates, boards, power supplies, phone addresses, `server.syslog` and the instructor commands. |
| 2026-10-30 | minor | Sessions for PDUs, scenarios and the OSI view, RESTCONF, NETCONF and the controller, powered devices and clouds. |
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
