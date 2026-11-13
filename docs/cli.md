# netlabcraft-cli

> **Last reviewed:** 2026-11-13 · **Last updated:** 2026-11-13 · **Change:** minor (results replay: the JSON export and verify example (M9.6)) · earlier: minor (examples for Simulation-mode options, controller discovery, kept NETCONF sessions, boards, power supplies and the instructor commands; the lab book from the command line)

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

Simulation-mode options, discovery, boards, power supplies and instructor tools:

```sh
# every hop 1 ms, and only ICMP kept in the event list and the capture
netlabcraft-cli run --sample two-subnets --cmd 'sim.constant_delay={"on":true}' \
  --cmd 'sim.capture_filter={"on":true,"protocols":["ICMP"]}' --cmd 'net.ping={"from":"PC1","to":"10.0.2.10"}'
# a controller finds the routers behind a seed over CDP and LLDP and manages those that log in
netlabcraft-cli run --sample automation --cmd 'controller.discover={"host":"PC1","seed":"192.168.1.1","user":"admin","password":"NetLab2026"}'
# a NETCONF session kept open (its number comes back as "session"), then closed
netlabcraft-cli run --sample automation \
  --cmd 'netconf.session={"pc":"PC1","target":"192.168.1.1","user":"admin","password":"NetLab2026","keep_open":true,"rpcs":["<lock><target><candidate/></target></lock>"]}' \
  --cmd 'netconf.session={"pc":"PC1","session":1,"close":true}'
# a board's program, pins and output after ten seconds of simulated time
netlabcraft-cli run --sample board --cmd 'sim.advance={"ms":10000}' --cmd 'board.status={"device":"Board1"}'
# a router's power-supply bays; fitting a second supply
netlabcraft-cli run --sample two-subnets --cmd 'device.psu={"device":"R1","bay":2,"fitted":true}'
# an activity's connectivity tests and a signed results file
netlabcraft-cli run --sample activity-first-router --cmd 'activity.connectivity={}' --cmd 'activity.export_results={"format":"csv"}'
# a JSON results file embeds the student's network; verifying it replays the checks (`replayed`, `replay_matches`)
netlabcraft-cli run --sample activity-first-router --cmd 'activity.export_results={"format":"json","path":"results.json"}' --cmd 'activity.verify_results={"path":"results.json"}'
```

The lab book ([labs/](labs/README.md)) is written so its `lab` blocks paste into `run --cmd`
and its `console` blocks into `exec` or `console`.

The commands added with these features: `pdu.simple`, `pdu.complex`, `pdu.fire`, `pdu.fire_all`,
`pdu.delete`, `pdu.list`, `scenario.list`, `scenario.new`, `scenario.delete`, `scenario.select`,
`sim.event`; `pc.http_request`, `pc.http_saved`, `netconf.session`, `controller.devices`,
`controller.push`; `device.adapter`, `device.power.source`, `phone.configure`, `cloud.map`. Since then:
`sim.constant_delay`, `sim.capture_filter` (and `sim.status` reports both), `pdu.complex` with
`at` (an absolute start time), `sim.event`'s `origin`; `controller.discover`, `controller.devices`
with `set_vars`, `controller.push` with template variables, `netconf.session` with `keep_open`,
`session` and `close`, `pc.http_request` with XML bodies and IPv6 URLs; `board.program`,
`board.status`, `board.wire` (and `link.add` with `"cable": "iot-custom"`); `device.psu`;
`phone.configure` with `address`, `mask` and `gateway`; `server.syslog`; and the instructor
commands `activity.save_answer`, `activity.answer`, `activity.load_answer`,
`activity.clear_answer`, `activity.tree`, `activity.weight`, `activity.tree_checks`,
`activity.save_initial`, `activity.load_initial`, `activity.clear_initial`,
`activity.translate`, `activity.connectivity`, `activity.export_results`,
`activity.verify_results`, `activity.edit_check` and `activity.locks`.

Every command id and its parameters: [`command-reference.md`](command-reference.md).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-13 | minor | Example of a replayed results file (M9.6). |
| 2026-10-31 | minor | Examples for Constant Delay and the capture filter, controller discovery, a kept NETCONF session, a board, power supplies and the instructor commands; the commands added since listed; the lab book from the command line. |
| 2026-10-30 | minor | Examples and the list of new commands: PDUs and scenarios, `sim.event`, RESTCONF, NETCONF, the controller, power and clouds. |
| 2026-10-30 | trivial | Shared format. |
| 2026-10-29 | major | Created. |
