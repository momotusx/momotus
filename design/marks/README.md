# Service marks

The marks the title page's ratings row shows beside each number (plans/title-page-ratings-genres-similar.md), in their owners' colours, as the owner asked on 2026-10-24 ("like on the covers"). Each platform keeps them in one asset place, so a swap is one file per mark.

| File | Mark | Source (read 2026-10-24) |
|---|---|---|
| `imdb.svg` | IMDb's yellow badge | the logo IMDb's own site draws (`#home_img` on imdb.com), 64x32 |
| `tmdb.svg` | TMDB "Primary short (blue)" square | themoviedb.org/about/logos-attribution, `blue_square_1` |
| `trakt.svg` | Trakt "Logo Mark" | app.trakt.tv/branding, the purple `--purple-600` (oklch(49% .217 289), #6438D0 in sRGB) with its white mark |

Where they live: web `apps/web/src/components/marks.ts`, Android `design/Marks.kt` (vectors from these paths), Apple `Marks.xcassets`. Apple's asset catalog renderer reads presentation attributes only, so its copies have TMDB's class fill written inline and IMDb's 100% rect in user units: the same artwork.

Rules kept: no recolouring, no alteration, clear space around each mark (Trakt's branding page); TMDB's credit line and logo also sit in Settings > About.
