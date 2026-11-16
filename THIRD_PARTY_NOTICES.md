# Third-Party Notices

Device HUB bundles or derives from the following third-party works.

## Vendored code

| Package | Copyright | License | Notes |
| --- | --- | --- | --- |
| `vendor/gpui-pre`, `vendor/gpui-pre-windows` | Zed Industries | Apache-2.0 (`LICENSE-APACHE` inside each dir) | GPUI fork with a GPU backdrop-filter patch; see `vendor/README.md` |

## Bundled fonts (`crates/device-hub/fonts/`)

All embedded into `device-hub.exe` at build time.

| Font | Copyright | License |
| --- | --- | --- |
| Geist | Vercel | SIL Open Font License 1.1 |
| IBM Plex Sans JP | IBM | SIL Open Font License 1.1 |
| Zen Maru Gothic | Yoshimichi Ohira | SIL Open Font License 1.1 |

OFL 1.1 permits bundling with an application provided the fonts are not sold
by themselves and this notice is preserved. The full license text:
<https://openfontlicense.org>

## Icons

`crates/device-hub/assets/icon.{ico,png}` and the in-app icon set are part of
Device HUB itself (Lucide-derived SVG paths for UI icons, ISC-licensed
<https://lucide.dev>).

## Analysis material

`analysis/` contains network captures and downloaded JavaScript from the
vendor web configurators (mouse.fit / hub.xsyd.top). Those files are NOT
Device HUB code: they are kept only for protocol-interoperability analysis,
are the property of their respective vendors, and are not covered by this
repository's license grant.
