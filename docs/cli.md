# netlabcraft-cli

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (examples for PDUs, the OSI view, RESTCONF, NETCONF, the controller and power)

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

Simulation mode, automation and powered devices from the command line:

```sh
# a complex PDU (here a ping with TTL 1), then the OSI view of the first event it caused
netlabcraft-cli run --sample two-subnets \
  --cmd 'pdu.complex={"from":"PC1","to":"R1","app":"ping","ttl":1}' --cmd 'sim.event={"id":1}'
# RESTCONF and NETCONF from a PC to a router (the automation sample's login)
netlabcraft-cli run --sample automation --cmd 'pc.http_request={"pc":"PC1","url":"https://192.168.1.1/restconf/data/ietf-system:system","user":"admin","password":"NetLab2026"}'
netlabcraft-cli run --sample automation --cmd 'netconf.session={"pc":"PC1","target":"192.168.1.1","user":"admin","password":"NetLab2026","rpcs":["<get-config><source><running/></source></get-config>"]}'
# a PC as a controller: add a router and read it
netlabcraft-cli run --sample automation --cmd 'controller.devices={"host":"PC1","add":{"address":"192.168.1.1","user":"admin","password":"NetLab2026"},"refresh":true}'
# where a phone's power comes from
netlabcraft-cli run --sample sip-phones --cmd 'device.power.source={"device":"Phone1"}'
```

The commands added with these features: `pdu.simple`, `pdu.complex`, `pdu.fire`, `pdu.fire_all`,
`pdu.delete`, `pdu.list`, `scenario.list`, `scenario.new`, `scenario.delete`, `scenario.select`,
`sim.event`; `pc.http_request`, `pc.http_saved`, `netconf.session`, `controller.devices`,
`controller.push`; `device.adapter`, `device.power.source`, `phone.configure`, `cloud.map`.

Every command id and its parameters: [`command-reference.md`](command-reference.md).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Examples and the list of new commands: PDUs and scenarios, `sim.event`, RESTCONF, NETCONF, the controller, power and clouds. |
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
