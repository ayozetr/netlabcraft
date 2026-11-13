# NetLabCraft app icon

A laptop and a desktop PC cabled to a switch, with a server below it: ink line work on paper
devices over a full-bleed teal tile with rounded corners. Supplied by the project owner for
NetLabCraft. It is not modelled on any vendor's icons or device pictures.

The supplied image was cleaned before it went in: renamed to `netlabcraft.png`, the black
background outside the rounded tile made transparent, and every metadata chunk removed.

## Palette

| Colour | Hex (approx.) | Used for |
|---|---|---|
| Ink | `#0b0b0c` | Line work and contour |
| Paper | `#efebe1` | The devices |
| NetLabCraft teal | `#4f95ad` | The tile |

## Geometry

A 1254 × 1254 px master; the tile fills it, with rounded corners and transparency outside them.
Windows and Linux use the full tile. The macOS icon (`netlabcraft-macos-512.png`) sits on
Apple's grid: an 824/1024 body with a transparent margin.

## Files

| File | Use |
|---|---|
| `netlabcraft.png` | Canonical artwork (master) |
| `netlabcraft-1024.png` | 1024 px render of the full tile |
| `netlabcraft-macos-512.png` | Runtime Dock icon on macOS |
| `netlabcraft.ico` | Windows icon, 16–256 px |
| `hicolor/<size>/apps/netlabcraft.png` | Linux icon theme, 16–512 px; the 256 px one is also the runtime window icon on Windows and Linux, and the web favicon |
| `LICENSE.txt` | MIT OR Apache-2.0 |

The icon name is `netlabcraft`, as used by `packaging/linux/io.github.ayozetr.netlabcraft.desktop` and the
`.nlcraft` file type. A `.icns` for a macOS bundle comes with the macOS packaging (it needs
`iconutil`).

## Regenerate

Replace `netlabcraft.png`, then run `packaging/icons.sh` (ImageMagick 7). Every output is
written without metadata.
