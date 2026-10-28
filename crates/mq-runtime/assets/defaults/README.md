# Default addon logos

The logos of the default addons (`../../src/defaults.json`), bundled into the core with
`include_bytes!` (`../../src/defaults.rs`, `LOGOS`) and served by the core's loopback server at
`/logo/<token>/<key>`. The first-run step therefore asks no logo host anything before the user
has chosen. Each file was fetched once by hand on 2026-10-19 (Streaming Catalogs' on 2026-10-22, stremio-addons.net's on 2026-10-28)
from the logo URL in the addon's own manifest. PNGs were resized so the longest side is 96 px (`sips -Z 96`). The one SVG was drawn to a
96 px PNG (`rsvg-convert -w 96 -a`), since the apps' image loaders read bitmaps only.

| File | Source (the manifest's `logo`) | Original | License note |
|---|---|---|---|
| `torrentio.png` | https://torrentio.strem.fun/images/logo_v1.png | 256 x 256 PNG | No license stated; Torrentio's mark, shown only to name its addon |
| `turkcealtyazi.png` | https://raw.githubusercontent.com/gorlev/turkcealtyaziorg-stremio-addon/master/images/logo.png | 144 x 144 PNG | From the addon's repository, MIT licensed (github.com/gorlev/turkcealtyaziorg-stremio-addon) |
| `subsense.png` | https://i.imgur.com/FaDbQAp.png | 1024 x 1024 PNG | No license stated; SubSense's mark, shown only to name its addon |
| `opensubtitles.png` | http://www.strem.io/images/addons/opensubtitles-logo.png | 602 x 151 PNG | No license stated; OpenSubtitles' mark, shown only to name its addon |
| `trakt.png` | https://2ecbbd610840-trakt.baby-beamup.club/public/logoPS.png?ver=0.2.7 | 1000 x 956 PNG | No license stated; the Trakt addon's mark, shown only to name it |
| `rottentomatoes.png` | https://upload.wikimedia.org/wikipedia/commons/4/45/Rotten_Tomatoes_alternative_logo.svg | 143.75 x 142.5 SVG, no scripts | Wikimedia Commons: public domain, marked "trademarked" (Rotten Tomatoes) |
| `torrentcatalogs.png` | https://i.ibb.co/w4BnkC9/GwxAcDV.png | 256 x 256 PNG | No license stated; the addon's mark, shown only to name it |
| `kitsu.png` | https://i.imgur.com/7N6XGoO.png | 280 x 280 PNG | No license stated; the Anime Kitsu addon's mark, shown only to name it |
| `streamingcatalogs.png` | https://play-lh.googleusercontent.com/TBRwjS_qfJCSj1m7zZB93FnpJM5fSpMA_wUlFDLxWAb45T9RmwBvQd5cWR5viJJOhkI | 512 x 512 PNG | No license stated; the addon uses Netflix's "N" as its logo, shown only to name the addon |
| `stremioaddons.png` | https://stremio-addons.net/img/web-app-manifest-192x192.png | 192 x 192 PNG | No license stated; the directory's mark, shown only to name its addon |

Cinemeta's manifest has no `logo`, and strem.io serves none at its addon image path, so it has
no file here; `DefaultAddonInfo.logo` is then empty and the apps show its initials.

A default that changes its logo keeps this copy until the file is replaced by hand.
