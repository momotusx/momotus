# Service marks

The marks the title page's ratings row shows beside each number (plans/title-page-ratings-genres-similar.md), in their owners' colours, as the owner asked on 2026-10-24 ("like on the covers"); and the two sign-in providers' marks on their buttons (design/DECISION.md: each provider draws its own button). Each platform keeps them in one asset place, so a swap is one file per mark.

| File | Mark | Source (read 2026-10-24) |
|---|---|---|
| `imdb.svg` | IMDb's yellow badge | the logo IMDb's own site draws (`#home_img` on imdb.com), 64x32 |
| `tmdb.svg` | TMDB "Primary short (blue)" square | themoviedb.org/about/logos-attribution, `blue_square_1` |
| `trakt.svg` | Trakt "Logo Mark" | app.trakt.tv/branding, the purple `--purple-600` (oklch(49% .217 289), #6438D0 in sRGB) with its white mark |
| `google.svg` | Google's standard four-colour "G" | the G the Google Identity Services button draws (accounts.google.com/gsi), as developers.google.com/identity/branding-guidelines requires it unchanged; read 2026-11-04 |
| `facebook.svg` | Facebook's "f" in its circle, the Login button's #1877F2 | the Facebook Login button's mark (developers.facebook.com/docs/facebook-login, Login button); the same artwork as design/exploration/v3/src/kit.ts |

Where they live: web `apps/web/src/components/marks.ts`, Android `design/Marks.kt` (vectors from these paths), Apple `Marks.xcassets` (the sign-in marks on web and Apple only, where the desktop sign-in draws them). Apple's asset catalog renderer reads presentation attributes only, so its copies have TMDB's class fill written inline and IMDb's 100% rect in user units: the same artwork.

Sign-in buttons (desktop, plans/owner-picks.md 2026-11-04): Google's dark theme, fill #131314, a 1 px #8E918F stroke inside, #E3E3E3 title, the G on a white circle; Facebook's #1877F2 fill with the white mark and title.

Rules kept: no recolouring, no alteration, clear space around each mark (Trakt's branding page); TMDB's credit line and logo also sit in Settings > About.
