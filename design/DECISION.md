# Design decision

## The v3 pick (2026-10-21)

The owner's words, translated: "I told you Apple TV: the colors and everything should be the same." So v3 is the direction closest to the Apple TV app, which both design critics named in every round: V2 Top Shelf (`design/exploration/v3/index.html`, five review rounds, 10 of 10 on TV and phone, `plans/reviews/design-v3-*`). Its colors follow Apple TV's own, as measured in `design/reference/apple-tv.md`: a black or near-black stage where the artwork is the only color, white text at Apple's opacity levels, 10 percent hairlines, a white focus (scale, lift, shadow, specular, never a colored ring), the white primary pill, and Apple's system blue only where Apple uses a key color (sheet primaries, sign-in, links). This replaces the blue-green accent switch of the exploration and the v2 accent #E5372A. If the owner wants another direction or a different key color, this block is where it changes.

Details decided with it, by the same rule (match Apple TV, except where the owner ruled a color out): switches use Apple's own green on iOS and Android (`#30D158` dark) as the iOS Settings switch does; destructive actions (Delete account, Remove) stay without red, as the owner ruled red out, and use the key blue in alerts with the destructive word itself; the TV form has no key color at all, as tvOS; provider sign-in buttons keep their provider's own colors (Facebook blue, Google's mark), since each provider draws its own button. The built exploration is `design/exploration/v3/v2.html`; the draft language and tokens are `design/exploration/v3/drafts/`.

## Owner direction, later on 2026-10-21

The owner's words, translated: study every page of Apple's design in detail, on the phone and on the web, and make our phone, web and desktop designs exactly that way; IMDb and Rotten Tomatoes ratings sit side by side, nicely, by default, always; and take from Stremio's design too, which is not bad overall.

What this sets: phone layouts follow Apple's iPhone TV app (`design/reference/apple-tv.md`, iPhone sections); web and desktop layouts follow tv.apple.com as measured there (its navigation, hover and keyboard states, shelves and pages), not the TV canvas scaled; the TV form follows tvOS and is checked on Android TV; Stremio's good parts are studied in `design/reference/stremio.md` and borrowed where they fit this language; every title shows its IMDb and Rotten Tomatoes ratings side by side wherever a rating appears (hero, title page, cards where ratings show), as Apple's TV app shows Rotten Tomatoes, by default and without any setting.

## Owner direction for v3 (2026-10-21, before the pick)

The owner's words, translated: every page, Settings included, must look ultra sleek, Apple level, Apple style. Study Apple's own design in detail (the Apple TV app on tvOS and iPhone, tv.apple.com) and keep its appeal. Color: monochrome, with one accent between blue and green; no red and no orange. Do not mix colors. Run the design through review rounds until it is perfect.

What this sets for every v3 direction:
- Monochrome: black, greys and white carry the whole interface; the one accent is a blue-green (teal, mint or cyan family) used sparingly (focus, progress, the active marker, a selected state), never a large fill.
- No red or orange anywhere in the chrome. Delete and other destructive actions use the system's destructive style only where the platform requires one; the v2 accent #E5372A and the orange markers in the v3 exploration go.
- Apple's real layouts, type scale, materials and focus effects are the reference; the brand rules stay (own wordmark and icons, no Apple logos or artwork).

The pick between V1 to V5 is still the owner's; this block records the constraints all five must meet.

## v2 (current until the v3 pick)

Status: v2, the owner's pick on 2026-10-17: "go like flystream.net; 1 (Ember) is good". The study of the reference is `design/reference/flystream.md`; the six directions shown are `design/exploration/v2/index.html` (Ember is direction 1). v1 (warm black, Fraunces, amber) is retired; its history is in git.

## The language

Minimal, dark, glass. One cool near-black page, translucent surfaces with a cool hairline edge, white for the one primary action, a single red-orange accent used sparingly, heavy tight sans titles, pills everywhere.

| Piece | Rule |
|---|---|
| Page | `bg` #06090E, a cool near-black. Dark only: there is no light theme; `color.light` mirrors `color.dark` so the platform generators keep working while the apps drop their light code |
| Surfaces | Glass: `glass` rgba(13,18,25,.58) or `glassStrong` rgba(9,13,19,.84), backdrop blur 18 px with saturate 1.08, a 1 px `edge` rgba(215,232,255,.13), a top highlight `inset 0 1px 0 rgba(255,255,255,.055)`, shadow `0 18px 48px rgba(0,0,0,.24)`. Solid fallbacks where blur is unavailable: `surface` #0C1118, `raised` #141B24 |
| Text | White by level: `text` #FFFFFF, `muted` #DCDDDD, `dim` #AAABAD. Never grey on grey |
| Primary action | One per screen (Play, Resume, Play again, Install): a white pill (`primary` #FFFFFF, `onPrimary` #0A0A0C), weight 650. Secondary actions are round or pill glass buttons with a `control` #626A73 edge (3.63:1 on `bg`) |
| Accent | `accent` #E5372A (4.66:1 on `bg`, 4.43:1 on `surface`, 4.05:1 on `raised`): the focus ring, the active tab marker, eyebrow labels, a focused or hovered title, and progress. Never a large fill. Accent text sits on `bg` only; over artwork an eyebrow is a chip filled with `bg` (as flystream's spotlight chip), since even a 90% scrim over light art leaves accent at 3.7:1 |
| Ratings | `star` #F5C518, only beside a number |
| Type | Inter (OFL) everywhere. Display weight 800 at tracking -0.035em; headings 800 at -0.025em; labels 650 at -0.01em; body 400. TV and phone sizes stay as in `type` |
| Radii | Posters `m` 12, cards and rows `card` 14, panels `l` 18, the home hero card `hero` 22, pills 999 |
| Motion | 180 ms focus, 260 ms panels, `cubic-bezier(0.22, 1, 0.36, 1)`; content fades up on load; reduced motion honoured |

## Patterns

- **Navigation.** TV and desktop: a floating glass pill centered at the top (wordmark, Home, Movies, Series, Calendar, Library, a divider, Search, Settings); the active link sits in a lighter inner pill. Phone: a floating glass bar at the bottom with icons and labels; the active one in a lighter rounded square.
- **Home hero.** An inset rounded card (`hero` radius, 1 px `edge`) holding the backdrop, the title (logo when it reads, else the title in display type), a meta line (star rating, year, runtime, genres), three lines of synopsis, the white Play pill and round glass secondaries with labels, and dash indicators for the carousel.
- **Section headers.** A short white bar (`text`), the title (heading weight), a hairline running to the edge, an "All" pill at the end.
- **Cards.** Posters 2:3 with a glass rating chip at the top right; the title (700) and year (`dim`) under the art, never over it. Continue Watching: 16:9 stills with a thin accent progress line and the core's caption as is. Focus (TV and keyboard) and hover lift the card, add a 1 px white edge, and turn the title `accent`.
- **Title page.** A full-bleed backdrop fading to `bg` from the left and the bottom; the logo; genres; the white primary pill and round glass secondaries (Trailer, Add to Library, Versions) with labels; a meta row; three lines of synopsis with More; a glass key-value table at the right (Status, First aired, Country, Seasons, Episodes, or Released and Runtime with "Ends at").
- **Episodes.** A toolbar of pills (seasons, search, sort); rows as glass cards with a hairline edge: a 16:9 still with the number at the bottom left and a "Watched" chip, the title (700), "56 min · Feb 18, 2022", the synopsis.
- **Player.** Full-width scrims top and bottom; title and "S1:E3 · Name" at the top left; the scrubber; round transport buttons; pill buttons Audio, Subtitles, Versions, Speed; Skip as a white pill, bottom right, with the focus ring only while it has focus.
- **Search.** A centered command palette on `glassStrong` over the blurred page: scope pill, a pill input, result rows with a small poster, title and "Movie · 2021 · ★ 8.0 · genres".
- **Browse pages.** An accent eyebrow over a big display title, a muted subtitle, filter pills, then text tabs with an accent underline.

## Rules that stay (TV and accessibility)

- One token source: `design/tokens.json` (v2), generated to CSS variables (web), Compose and SwiftUI.
- TV canvas 1920x1080 points; body 29 pt, floor 23 pt; safe area 60 pt top and bottom, 80 pt sides.
- Focus: a 4 px `accent` ring with a dark halo plus the card lift, never color alone; desktop and phone show it for keyboard focus only.
- The core's captions, labels and states are shown as is.
- Lucide icons with visible labels on TV; no emoji anywhere.
- Contrast floors: text 4.5:1, control edges 3:1 (measured above with the WCAG formula).
