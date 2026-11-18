# Lab 76 — Backbone cabling between two closets

**Course:** Network cabling and infrastructure · **Time:** 40 minutes

## Objectives

- Put a patch panel in each of two wiring closets and a drop from a device to each panel.
- Join the two panels with a backbone cable (`backbone.add`) and see that the two devices, one
  in each closet, become one working link.
- Read the backbone in `backbone.list`, in `closet.list` (a cable "to" another place) and in
  the Cable Properties window.
- Stand a wall mount on the wall of a room, plug a PC into it and carry it to a panel in a
  closet with a second backbone.
- Take a backbone away and watch the link go with it, while the drops stay.

## Topology

```text
 Main Wiring Closet                       Annex (another closet of the building)
 PC1 ── drop ── PP1 port 1 ═══ backbone ═══ PP2 port 1 ── drop ── PC2

 Main Building, a room's wall
 PC3 ── drop ── WM1 port 1 ═══ backbone ═══ PP1 port 3 ── drop ── PC4 (Main Wiring Closet)
```

A **backbone** is a cable between two ports that stand in two different places: closet to closet
in a building, or building to building. A patch cord, by contrast, joins two ports of one closet.
A path made of a drop, a backbone and a drop cables the two devices as an ordinary automatic
cable, and the cable is checked for reach like any other: copper carries 100 m, and the run
between two closets of one building counts as 30 m.

```lab
place.add {"kind": "closet", "name": "Annex", "parent": 2}
device.add {"model": "PC", "name": "PC1"}
device.add {"model": "PC", "name": "PC2"}
device.add {"model": "PC", "name": "PC3"}
device.add {"model": "PC", "name": "PC4"}
device.place {"device": "PC2", "place": 4, "x": 80, "y": 80}
device.place {"device": "PC3", "place": 2, "x": 40, "y": 40}
passive.add {"kind": "copper-patch-panel", "name": "PP1", "place": 3}
passive.add {"kind": "copper-patch-panel", "name": "PP2", "place": 4}
patch.wire {"a": {"device": "PC1", "port": "FastEthernet0"}, "b": {"panel": "PP1", "port": 1}}
patch.wire {"a": {"device": "PC2", "port": "FastEthernet0"}, "b": {"panel": "PP2", "port": 1}}
```

```console PC1
ipconfig 10.76.0.1 255.255.255.0
```

```console PC2
ipconfig 10.76.0.2 255.255.255.0
```

## Part 1 — The drops alone carry nothing

Each PC has a drop to the panel in its own closet, but the panels are not joined, so there is
no path between the PCs:

```verify
ping PC1 10.76.0.2 => failure
run backbone.list {}
  reject: Annex
```

## Part 2 — Join the closets

A patch cord cannot leave its closet; the command says to use a backbone instead:

```verify
run patch.wire {"a": {"panel": "PP1", "port": 1}, "b": {"panel": "PP2", "port": 1}}
  expect: backbone
```

`backbone.add` takes a port of each panel. Both ends have a drop, so the path is complete and
the two PCs are cabled:

```lab
backbone.add {"a": {"panel": "PP1", "port": 1}, "b": {"panel": "PP2", "port": 1}}
```

```verify
ping PC1 10.76.0.2 => success
run backbone.list {}
  expect: Main Wiring Closet
  expect: Annex
  expect: PC1
run closet.list {"place": 3}
  expect: backbones
  expect: Annex
run closet.list {"place": 4}
  expect: Main Wiring Closet
```

In the wiring closet view each backbone leaves its port as a short cable labelled `to Annex`
(`to Main Wiring Closet` in the other closet). Click the cable between the PCs in the
logical view and Cable Properties names the backbone it runs through.

A port takes one patch cord or backbone, so a second one on port 1 is refused (and a copper
panel is never joined to a fiber panel):

```verify
run backbone.add {"a": {"panel": "PP1", "port": 1}, "b": {"panel": "PP2", "port": 2}}
  expect: already
```

## Part 3 — A wall mount in a room

A wall mount stands on the wall of a room: in a wiring closet or a container, and in a
building too. PC3 stands in the building, and its drop goes to the wall mount; a backbone then
carries the wall mount's port to port 3 of the panel in the main closet, where PC4 has a drop:

```lab
passive.add {"kind": "copper-wall-mount", "name": "WM1", "place": 2}
patch.wire {"a": {"device": "PC3", "port": "FastEthernet0"}, "b": {"panel": "WM1", "port": 1}}
patch.wire {"a": {"device": "PC4", "port": "FastEthernet0"}, "b": {"panel": "PP1", "port": 3}}
backbone.add {"a": {"panel": "WM1", "port": 1}, "b": {"panel": "PP1", "port": 3}}
```

```console PC3
ipconfig 10.76.1.3 255.255.255.0
```

```console PC4
ipconfig 10.76.1.4 255.255.255.0
```

```verify
ping PC3 10.76.1.4 => success
run closet.list {"place": 2}
  expect: WM1
  expect: Main Wiring Closet
```

## Part 4 — Take a backbone away

Removing the backbone unplugs the cable its path made. The drops and the panels stay, so
joining the ports again brings the link back:

```lab
backbone.remove {"a": {"panel": "PP1", "port": 1}}
```

```verify
ping PC1 10.76.0.2 => failure
run closet.list {"place": 3}
  expect: PP1
```

```lab
backbone.add {"a": {"panel": "PP1", "port": 1}, "b": {"panel": "PP2", "port": 1}}
```

```verify
ping PC1 10.76.0.2 => success
```
