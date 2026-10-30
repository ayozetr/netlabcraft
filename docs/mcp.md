# MCP server

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** trivial (shared format)

`netlabcraft-cli mcp [--in LAB] [--sample ID]` speaks the Model Context Protocol (JSON-RPC 2.0, one message per line) on stdin/stdout.

| Tool | Arguments | Does |
|---|---|---|
| `netlabcraft_run` | `command`, `params` | Runs any engine command ([`command-reference.md`](command-reference.md)). |
| `netlabcraft_cli` | `device`, `lines` | Types lines at a device console; returns each output and the final prompt. |
| `netlabcraft_lab` | — | Devices, ports, addresses, link state and cables. |
| `netlabcraft_commands` | — | The command catalog. |

A typical agent session: `file.new` (or `device.add` + `link.add`), configure with `netlabcraft_cli`, verify with `net.ping` / `net.traceroute`, inspect `sim.events`, then `file.save`.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
