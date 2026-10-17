# Design decision

Status: v2, the owner's pick on 2026-10-17: "go like flystream.net; 1 (Ember) is good". The study of the reference is `design/reference/flystream.md`; the six directions shown are `design/exploration/v2/index.html` (Ember is direction 1). v1 (warm black, Fraunces, amber) is retired; its history is in git.

## The language

Minimal, dark, glass. One cool near-black page, translucent surfaces with a cool hairline edge, white for the one primary action, a single red-orange accent used sparingly, heavy tight sans titles, pills everywhere.

| Piece | Rule |
|---|---|
| Page | `bg` #06090E, a cool near-black. Dark only: there is no light theme; `color.light` mirrors `color.dark` so the platform generators keep working while the apps drop their light code |
| Surfaces | Glass: `glass` rgba(13,18,25,.58) or `glassStrong` rgba(9,13,19,.84), backdrop blur 18 px with saturate 1.08, a 1 px `edge` rgba(215,232,255,.13), a top highlight `inset 0 1px 0 rgba(255,255,255,.055)`, shadow `0 18px 48px rgba(0,0,0,.24)`. Solid fallbacks where blur is unavailable: `surface` #0C1118, `raised` #141B24 |
| Text | White by level: `text` #FFFFFF, `muted` #DCDDDD, `dim` #AAABAD. Never grey on grey |
| Primary action | One per screen (Play, Resume, Play again, Install): a white pill (`primary` #FFFFFF, `onPrimary` #0A0A0C), weight 650. Secondary actions are round or pill glass buttons with a `control` #626A73 edge (3.63:1 on `bg`) |
| Accent | `accent` #E5372A (4.66:1 on `bg`, 4.43:1 on `surface`, 4.05:1 on `raised`): the focus ring, the active tab marker, eyebrow labels, a focused or hovered title, Skip and progress. Never a large fill. Accent text sits on `bg` only; over artwork an eyebrow is a chip filled with `bg` (as flystream's spotlight chip), since even a 90% scrim over light art leaves accent at 3.7:1 |
| Ratings | `star` #F5C518, only beside a number |
| Type | Inter (OFL) everywhere. Display weight 800 at tracking -0.035em; headings 800 at -0.025em; labels 650 at -0.01em; body 400. TV and phone sizes stay as in `type` |
| Radii | Posters `m` 12, cards and rows `card` 14, panels `l` 18, the home hero card `hero` 22, pills 999 |
| Motion | 180 ms focus, 260 ms panels, `cubic-bezier(0.22, 1, 0.36, 1)`; content fades up on load; reduced motion honoured |

## Patterns

- **Navigation.** TV and desktop: a floating glass pill centered at the top (wordmark, Home, Movies, Series, Calendar, Library, a divider, Search, Settings); the active link sits in a lighter inner pill. Phone: a floating glass bar at the bottom with icons and labels; the active one in a lighter rounded square.
- **Home hero.** An inset rounded card (`hero` radius, 1 px `edge`) holding the backdrop, the title (logo when it reads, else the title in display type), a meta line (star rating, year, runtime, genres), three lines of synopsis, the white Play pill and round glass secondaries with labels, and dash indicators for the carousel.
- **Section headers.** A 3 px bar, the title (heading weight), a hairline running to the edge, an "All" pill at the end.
- **Cards.** Posters 2:3 with a glass rating chip at the top right; the title (700) and year (`dim`) under the art, never over it. Continue Watching: 16:9 stills with a thin accent progress line and the core's caption as is. Focus (TV and keyboard) and hover lift the card, add a 1 px white edge, and turn the title `accent`.
- **Title page.** A full-bleed backdrop fading to `bg` from the left and the bottom; the logo; genres; the white primary pill and round glass secondaries (Trailer, Add to Library, Versions) with labels; a meta row; three lines of synopsis with More; a glass key-value table at the right (Status, First aired, Country, Seasons, Episodes, or Released and Runtime with "Ends at").
- **Episodes.** A toolbar of pills (seasons, search, sort); rows as glass cards with a hairline edge: a 16:9 still with the number at the bottom left and a "Watched" chip, the title (700), "56 min · Feb 18, 2022", the synopsis.
- **Player.** Full-width scrims top and bottom; title and "S1:E3 · Name" at the top left; the scrubber; round transport buttons; pill buttons Audio, Subtitles, Versions, Speed; Skip as a white pill with an accent edge, bottom right.
- **Search.** A centered command palette on `glassStrong` over the blurred page: scope pill, a pill input, result rows with a small poster, title and "Movie · 2021 · ★ 8.0 · genres".
- **Browse pages.** An accent eyebrow over a big display title, a muted subtitle, filter pills, then text tabs with an accent underline.

## Rules that stay (TV and accessibility)

- One token source: `design/tokens.json` (v2), generated to CSS variables (web), Compose and SwiftUI.
- TV canvas 1920x1080 points; body 29 pt, floor 23 pt; safe area 60 pt top and bottom, 80 pt sides.
- Focus: a 4 px `accent` ring with a dark halo plus the card lift, never color alone; desktop and phone show it for keyboard focus only.
- The core's captions, labels and states are shown as is.
- Lucide icons with visible labels on TV; no emoji anywhere.
- Contrast floors: text 4.5:1, control edges 3:1 (measured above with the WCAG formula).
