# MCP server

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** trivial (shared format)

`packetcraft-cli mcp [--in LAB] [--sample ID]` speaks the Model Context Protocol (JSON-RPC 2.0, one message per line) on stdin/stdout.

| Tool | Arguments | Does |
|---|---|---|
| `packetcraft_run` | `command`, `params` | Runs any engine command ([`command-reference.md`](command-reference.md)). |
| `packetcraft_cli` | `device`, `lines` | Types lines at a device console; returns each output and the final prompt. |
| `packetcraft_lab` | — | Devices, ports, addresses, link state and cables. |
| `packetcraft_commands` | — | The command catalog. |

A typical agent session: `file.new` (or `device.add` + `link.add`), configure with `packetcraft_cli`, verify with `net.ping` / `net.traceroute`, inspect `sim.events`, then `file.save`.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
