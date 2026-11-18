#!/usr/bin/env bash
# Renders the README screenshots (docs/images) into the directory given as $1, with the recipes
# recorded here. Run by the `readme` job of .github/workflows/snapshots.yml; the PNGs are looked at,
# optimised and committed by hand, never by CI.
set -uo pipefail
out=${1:?output directory}
mkdir -p "$out"
snap=target/release/examples/snapshot
fail=0
run() { "$@" || { echo "FAILED: $*" >&2; fail=1; }; }
cli() { printf 'cli.exec={"device":"%s","line":"%s"}' "$1" "$2"; }

run "$snap" --sample multilayer "$out/hero-multilayer.png"

run "$snap" --sample ospf --console R1 --tab cli \
  --cmd "$(cli R1 enable)" --cmd "$(cli R1 'show ip ospf neighbor')" --cmd "$(cli R1 'show ip route ospf')" \
  "$out/router-cli.png"

run "$snap" --sample two-subnets --simulation \
  --cmd 'net.ping={"from":"PC1","to":"10.0.2.10","count":1}' \
  "$out/simulation-mode.png"

run "$snap" --sample ipv6 --console PC1 --desktop ip "$out/pc-ip-configuration.png"

run "$snap" --sample serial-wan "$out/serial-wan.png"

run "$snap" --sample activity-first-router --instructions "$out/activity.png"

# A home router with a PC, two Wi-Fi laptops and the provider's router.
run "$snap" --size 1600x820 --console Home --config-page home \
  --cmd 'device.add={"model":"Home-Router","name":"Home","x":520,"y":200}' \
  --cmd 'device.add={"kind":"pc","name":"PC1","x":520,"y":400}' \
  --cmd 'device.add={"model":"Laptop-WiFi","name":"Laptop1","x":720,"y":380}' \
  --cmd 'device.add={"model":"Laptop-WiFi","name":"Laptop2","x":320,"y":380}' \
  --cmd 'device.add={"model":"Router-3","name":"ISP","x":720,"y":60}' \
  --cmd 'link.add={"a":"Home","a_port":"Ethernet1","b":"PC1"}' \
  --cmd 'link.add={"a":"Home","a_port":"Internet0","b":"ISP"}' \
  --cmd 'device.wireless={"device":"Laptop1","ssid":"HomeNetwork"}' \
  --cmd 'device.wireless={"device":"Laptop2","ssid":"HomeNetwork"}' \
  "$out/home-router.png"
exit $fail
