---
name: regen-icons
description: Regenerate the kotonoha app icon and menu-bar tray glyph from design/*.svg. Use when the logo or tray icon needs to be changed, re-rendered, or when icons look wrong (white square in menu bar, missing gradients, opaque corners).
---

# Regenerate app icon & tray glyph

Two assets, two different pipelines. Do not swap them — each works around a different tooling trap.

## Tooling traps (why this is not just `magick logo.svg icon.png`)

- **ImageMagick's SVG renderer silently drops gradients and clipPaths** → renders black shapes. Never rasterize `design/logo.svg` with `magick` directly.
- **qlmanage (and IM) put a white background behind SVG renders** → corners become opaque. Always re-apply the squircle alpha mask after rendering.
- **macOS template (tray) images use only the alpha channel.** A PNG without alpha = solid white square in the menu bar. Render the tray glyph with ImageMagick *native drawing* on `xc:none`, never from SVG.

## App icon (design/logo.svg → app-icon.png → icon set)

```sh
cd <repo root>
qlmanage -t -s 1024 -o /tmp design/logo.svg
magick -size 1024x1024 xc:none -fill white -draw "roundrectangle 0,0 1023,1023 230,230" PNG32:/tmp/squircle_mask.png
magick /tmp/logo.svg.png -background none -gravity center -extent 1024x1024 \
  /tmp/squircle_mask.png -alpha off -compose CopyOpacity -composite PNG32:app-icon.png
npm run tauri icon app-icon.png   # regenerates src-tauri/icons/ — leaves tray.png alone
```

Verify: `magick app-icon.png -format "%[pixel:p{2,2}]" info:` must print `srgba(...,0)` (transparent corner).

When writing/editing logo.svg: use `gradientUnits="userSpaceOnUse"` and a plain `<rect rx="230">` for the squircle (the mask handles corners). Design language: dark squircle `#1b1b21→#0a0a0d`, mint gradient `#6ef2da→#2dd4bf`, subtle mint radial glow top-left.

## Tray glyph (design/tray.svg is the *reference drawing only*)

Render with native IM drawing at 4x then downscale; cut the midrib out of the alpha with DstOut:

```sh
magick -size 176x176 xc:none -fill black -draw "path 'M 135.0 21.3 Q 164.0 115.8 42.8 146.0 Q 13.8 51.5 135.0 21.3 Z'" PNG32:/tmp/tray_leaf.png
magick -size 176x176 xc:none -stroke white -strokewidth 13 -fill none -draw "path 'M 125.7 31.2 Q 79.3 77.6 49.1 135.6'" PNG32:/tmp/tray_rib.png
magick /tmp/tray_leaf.png /tmp/tray_rib.png -compose DstOut -composite -resize 44x44 PNG32:src-tauri/icons/tray.png
```

(The 176-canvas paths are design/tray.svg coordinates × 4. If the SVG changes, rescale its path coordinates accordingly.)

Verify: `magick src-tauri/icons/tray.png -format "%[pixel:p{0,0}] %A" info:` → corner `srgba(0,0,0,0)`, alpha `Blend`. Size guidance: the glyph should fill most of the 44px canvas (~3px margins) or it looks undersized next to other menu-bar items.

## After regenerating

Rebuild the app (`npm run tauri build -- --bundles app`, with `APPLE_SIGNING_IDENTITY` exported if available). If the Dock shows a stale icon, `killall Dock` refreshes the cache.
