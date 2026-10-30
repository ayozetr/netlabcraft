# netlabcraft-cli

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** trivial (shared format)

The headless command line: run engine commands, type at consoles, write captures and serve MCP, with no window.

```text
netlabcraft-cli run [--in LAB] [--sample ID] --cmd 'id={json}'... [--out LAB]
netlabcraft-cli exec [--in LAB] [--sample ID] DEVICE 'line'... [--out LAB]
netlabcraft-cli console [--in LAB] [--sample ID] DEVICE [--out LAB]      # lines from stdin
netlabcraft-cli pcap [--in LAB] [--sample ID] --ping FROM:ADDRESS... --out FILE.pcap
netlabcraft-cli mcp [--in LAB] [--sample ID]
netlabcraft-cli commands | samples | version
```

Examples:

```sh
netlabcraft-cli exec --sample two-subnets R1 enable 'show ip route'
netlabcraft-cli run --cmd 'device.add={"kind":"router"}' --cmd 'device.add={"kind":"pc"}' \
  --cmd 'link.add={"a":"PC1","b":"R1"}' --out lab.nlcraft
netlabcraft-cli pcap --sample vlans --ping PC1:192.168.20.10 --out vlans.pcap
```

Every command id and its parameters: [`command-reference.md`](command-reference.md).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
