# Reference study: flystream.net (2026-10-17)

The owner named flystream.net as the overall design language for Marquee: minimal, dark first, modern, 2026. This file records what that language is, measured from the live site (computed styles, its stylesheet, screenshots of Home, Movie, Series, Browse, Calendar, Settings, Search, and the phone layout). Screenshots and the downloaded stylesheet stay local and untracked (`.playwright-mcp/flystream/`); nothing of theirs ships. We take the language, not their assets, fonts, names or content.

## Tokens, measured

| Token | Value | Notes |
|---|---|---|
| Background | `#06090e` (body `rgb(5,8,13)`) | Near black with a cool blue tint, not neutral, not warm |
| Raised surface | `rgba(20,27,36,.72)`, alt `rgba(12,17,24,.72)` | Always translucent over the page |
| Glass surface | `rgba(13,18,25,.58)`, strong `rgba(9,13,19,.78)` | With `backdrop-filter: blur(12 to 22px) saturate(1.08)` |
| Glass edge | `rgba(215,232,255,.13)`, strong `rgba(225,239,255,.24)` | 1 px, cool white; plus `inset 0 1px rgba(255,255,255,.055)` as a top highlight |
| Glass shadow | `0 18px 48px rgba(0,0,0,.24)` | Soft, large |
| Lines | `rgba(255,255,255,.08)`, strong `.16` | Hairlines only |
| Text | `#ffffff`; muted `rgba(255,255,255,.86)`; dim `.66` | White hierarchy by opacity, no grey hex |
| Accent | `#e5372a` (red orange) | Used sparingly: active tab underline, eyebrow labels, hover title, a few badges |
| Rating | gold `#f5c518` star | IMDb gold, only beside numbers |
| Control pill | `linear-gradient(145deg, rgba(255,255,255,.12), rgba(255,255,255,.065))`, edge `rgba(255,255,255,.24)`, hover edge `.44` | Secondary circular icon buttons and chips |
| Primary button | white fill `#fff`, text `#0a0a0c`, 50 px tall, pill, padding 0 30 px, 14 px weight 650, `letter-spacing -0.01em`, shadow `0 3px 16px rgba(0,0,0,.28)` | One per screen: Play |
| Radius | pill 999 (179 uses); 12 / 10 / 8 / 14 / 16 / 18 | Posters 10 to 12; episode rows 14; panels and the hero card 16 to 18 |
| Font | "Netflix Sans" (theirs, proprietary), falling back to self-hosted Inter 400 to 800 | We use Inter (OFL) |
| Type | section titles 27 to 28 px weight 800, tracking -0.025em; hero title 64 px 800; page titles about 64 px 800 with a white to grey vertical gradient; body 13 to 16 px; labels 11 to 12.5 px weight 600 to 700 | Heavy weights, tight tracking, small UI text |
| Motion | 150 to 220 ms; easing `cubic-bezier(.22,1,.36,1)` | Snappy ease-out; content fades up on load |

## Patterns

- **Floating glass nav**: a pill centered at the top (logo, five text links, a divider, profile, search, settings), active link in a lighter inner pill. On phones it becomes a floating glass bottom bar of icons, the active icon in a lighter rounded square.
- **Home hero as an inset card**: the backdrop sits in a rounded rectangle 22 px from the edges with a 1 px edge, not full bleed. Title (or logo) bottom left, a meta line (star rating, year, genre), three lines of synopsis, then the white Play pill and round glass icon buttons. Carousel progress as short dashes bottom right with a pause button.
- **Section headers**: a 3 px vertical bar, the title, a hairline running to the right edge, and an "All" pill at the end.
- **Poster cards**: 2:3, radius about 10, a dark glass rating chip top right ("★ 8.0"), title (weight 700) and year (dim) under the poster, no overlay text on art. Hover lifts the card a little, adds a 1 px white edge and turns the title to the accent. Rows page with a round glass arrow on the edge.
- **Top 10 row**: big outlined two-digit numerals over the bottom left of the poster.
- **Spotlight banner**: a wide rounded card with the backdrop, an accent eyebrow chip ("SERIES SPOTLIGHT"), title, meta, synopsis and a white button; the poster floats at the right.
- **Title page**: full-bleed backdrop fading to the page color from the left and the bottom; the title logo large; genres; Play pill plus a row of round glass icons (party, add, watched, download, episodes); a meta row (year, runtime, a boxed rating like "R" or "4K", star rating); a three-line synopsis with "Read more"; a glass key-value table at the right (Status, Language, Released, Runtime with "Ends 7:34 PM").
- **Episodes**: a toolbar of pills (season dropdown, search episodes, search quotes, sort), then rows as rounded cards with a hairline edge: a 16:9 still with the episode number bottom left and a "4K" badge top right, title weight 700, "56 min · October 15, 2026", one-line synopsis.
- **Browse**: an accent eyebrow ("BROWSE") over a big gradient title, a muted subtitle, filter pills with carets, then text tabs with an accent underline.
- **Search**: a centered command-palette panel over a heavily blurred page; a scope pill; result rows with a small poster, title, "Movie · 2021 · ★ 7.8 · genres" and a chevron.
- **People**: cast as glass cards in a three-column grid, round photo, name, character.
- **Settings**: a left list of sections in a card, content panels with titled blocks and outlined buttons. The plainest part of the site.

## What Marquee takes, and what it keeps

Takes: the cool near-black page, translucent glass surfaces with a cool hairline edge and top highlight, the white primary pill with round glass secondaries, heavy tight sans titles, opacity-based text hierarchy, pill everything, the inset home hero card, section headers with a hairline, rating chips, the episode card row, the palette-style search, the floating glass nav, the 150 to 220 ms ease-out.

Keeps from our own system (non-negotiable for TV and accessibility): the focus ring with its halo for remotes (4 px plus halo, never color alone), TV type sizes and safe areas, one primary action per screen, the core's captions and states shown as is, Lucide icons with labels on TV, `prefers-reduced-motion`, contrast floors (body text 4.5:1, controls 3:1).

Leaves out: their logo, mascot and seasonal decorations, "Netflix Sans", provider rows with third-party brand names, any wording of theirs.

## Open choices for the owner (variants in `design/exploration/v2/`)

Accent hue (their red orange, our amber, white only, or another), display face (Inter heavy versus another open grotesk), home hero (inset card versus full bleed), and the phone navigation (floating bottom bar versus tab bar).
