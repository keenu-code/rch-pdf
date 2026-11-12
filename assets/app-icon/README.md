# RCH PDF app icon

<img src="pdfcraft-small.svg" alt="RCH PDF app icon: RPDF over an orange bar on cream" width="128">

**Mark:** the letters RPDF (RCH PDF) in Poppins SemiBold over a short orange bar, with "RCH PDF" spaced out
beneath in Poppins Medium, after the RCH Lawyers logo (Poppins with a solid orange bar).

**Palette:**

| Colour | Hex | Used for |
|---|---|---|
| Cream | `#FCFAF4` | the tile |
| Ink | `#231F20` | the lettering |
| RCH orange | `#FF6C2F` | the bar |
| Edge | `#E2DCD0` | a hairline round the tile, so it reads on white |

**Tile:** `viewBox="0 0 512 512"`, a rounded square with `rx=112`. Windows and Linux icons use the tile as it
is; macOS icons put it on Apple's grid (an 824 px body centred on a transparent 1024 px canvas).

**Provenance:** drawn by Ryan Blinco (Blinco Technologies) for this build. The lettering was set in Poppins
(Indian Type Foundry, SIL Open Font License 1.1, google/fonts `ofl/poppins`) and converted to outlines, so no
font file is used or shipped. Licence: [LICENSE.txt](LICENSE.txt) (`MIT OR Apache-2.0`, like the repo).

## Files

| File | What it is |
|---|---|
| `pdfcraft.svg` | the master vector (traced at 2048 px); every PNG, `.ico` and `.icns` is rendered from it |
| `pdfcraft-small.svg` | a lighter vector (traced at 1024 px) for places where size matters, such as this README |
| `pdfcraft-1024.png` | 1024 px on Apple's grid; also the runtime Dock icon on macOS |
| `pdfcraft.icns` | macOS icon (16–1024 px) |
| `pdfcraft.ico` | Windows icon (16–256 px), embedded in `pdfcraft.exe` by `apps/pdfcraft/build.rs` |
| `pdfcraft-document.svg` | Windows PDF file icon master: a Lucide document outline, red badge and outlined PDF lettering; no font dependency |
| `pdfcraft-document.ico` | Windows PDF file icon (16–256 px), embedded as resource 2 in `pdfcraft.exe` |
| `hicolor/<n>x<n>/apps/ai.storyteller.pdfcraft.png` | Linux hicolor theme, 16–512 px; the 256 px one is the runtime icon on Windows and Linux |
| `hicolor/scalable/apps/ai.storyteller.pdfcraft.svg` | Linux scalable icon (copy of the master) |

Where it shows: `apps/pdfcraft/src/main.rs` sets the window icon (Dock, taskbar, Alt-Tab, launcher) and the
Wayland app id `ai.storyteller.pdfcraft`; `packaging/linux/ai.storyteller.pdfcraft.desktop` names the
hicolor icon.

Windows PDF files use resource 2 (`"pdfcraft.exe",-2`) when PdfCraft is chosen as the default
reader; the app keeps resource 1. The document outline is adapted from Lucide `file-text`
1.49.0 (ISC); its badge and outlined lettering are original (`MIT OR Apache-2.0`), covered by
[LICENSE-document.txt](LICENSE-document.txt). Both icons are embedded, requiring no runtime
sidecar. This provides a file type icon; first-page thumbnails still need a thumbnail provider.

## Regenerate

```sh
packaging/icons.sh        # needs resvg and python3; iconutil (macOS) for the .icns
cargo xtask assets        # then update the sha256 values in ATTRIBUTION.toml and run with --write
```
