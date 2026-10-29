# packetcraft-cli

```text
packetcraft-cli run [--in LAB] [--sample ID] --cmd 'id={json}'... [--out LAB]
packetcraft-cli exec [--in LAB] [--sample ID] DEVICE 'line'... [--out LAB]
packetcraft-cli console [--in LAB] [--sample ID] DEVICE [--out LAB]      # lines from stdin
packetcraft-cli pcap [--in LAB] [--sample ID] --ping FROM:ADDRESS... --out FILE.pcap
packetcraft-cli mcp [--in LAB] [--sample ID]
packetcraft-cli commands | samples | version
```

Examples:

```sh
packetcraft-cli exec --sample two-subnets R1 enable 'show ip route'
packetcraft-cli run --cmd 'device.add={"kind":"router"}' --cmd 'device.add={"kind":"pc"}' \
  --cmd 'link.add={"a":"PC1","b":"R1"}' --out lab.pcraft
packetcraft-cli pcap --sample vlans --ping PC1:192.168.20.10 --out vlans.pcap
```

Every command id and its parameters: [`command-reference.md`](command-reference.md).
