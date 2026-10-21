# Reference study: Stremio (2026-10-21)

The owner asked to take Stremio's good parts too (see the "Owner direction, later on 2026-10-21" block of `design/DECISION.md`). This file records Stremio's web app as measured: web.stremio.com computed styles and screenshots (headless Google Chrome via Playwright 1.63.0, `--mute-audio`, media requests blocked, dark scheme, signed out, nothing installed), the stylesheet that build serves, and the open source web app (Stremio/stremio-web, `development` branch), plus the official Android and Android TV apps, inspected from their APKs without installing or running them (section "Stremio Android and Android TV"). It complements `design/reference/apple-tv.md`: Apple sets the look, Stremio contributes structure that Apple's app does not have (addons, streams, sources). We take the structure and the feel, never Stremio's name, logo, artwork, mascot or font. Screenshots are private reference in the session scratchpad (`stremio-ref/`), never committed or shipped.

## Summary

1. Stremio is a single dark stage (#0C0B11 fading to #1A173E at 41 degrees) with one 5 percent white fill (`--overlay-color`) for every surface, chip, card and hover, white text at 90 percent, and three loud accents: purple #7B5BF5 for selection and progress, green #22B365 for every primary action, yellow #F6C700 and red #E50914 for flags.
2. One family (Plus Jakarta Sans) at four weights; the root size steps with the window (14 px up to 1600 wide, 15 px to 2200, 16 px to 2800, 18 px above), and everything is in rem, so the whole layout scales as one; a user zoom (75 to 175 percent) sits on top.
3. The title page is its best idea: an info column (logo, runtime, year, IMDb rating with the IMDb mark, labeled chip groups for genres, cast and directors, summary, one action row) beside a full-height right panel that holds either the episode list or the stream list, and switches between them in place.
4. Streams are listed in one flat list in addon order with an "All" or per-addon filter on top; each row shows the source name in a fixed 7 rem column and the addon's own description verbatim, with a play circle on hover or focus and a progress line under the source used last.
5. Shape is uniform (one 0.75 rem radius for cards, posters, panels and menus; pills for every control), focus and hover are a 2 or 3 px white ring with no lift, and IMDb is the only rating shown (number plus IMDb mark, title pages only); Rotten Tomatoes appears nowhere.

## Sources and how to read the numbers

| Tag | Source |
|---|---|
| live | web.stremio.com, build `f0222603bb8119d8a63958ad8acfd025de973f2e` (shown in Settings as "App Version 6.0.7"), computed styles read with `getComputedStyle` in the live page, signed out, guest profile with its default addons (Cinemeta, YouTube, WatchHub, Public Domain Movies, OpenSubtitles v3, Local Files). Pixel values are at a 1920x1080 viewport (root 15 px) unless 1440 (root 14 px) or phone (390x844, root 14 px) is named |
| css | that build's `styles/main.css` (461 KB, saved in the scratchpad as `stremio-study/data/deployed.css`); its `:root` tokens match the source below exactly |
| src | github.com/Stremio/stremio-web, `development` at `9f991f0` (2026-10-21), files under `src/` named per row; colors package `@stremio/stremio-colors` 5.2.0 |
| shot | a file in `stremio-ref/` (listed at the end) |

Stremio sets every size in rem and changes the root size by window width (`src/App/styles.less`): 18 px at 2800 px and wider, 16 px from 2200 to 2800, 15 px from 1600 to 2200, 14 px at 1600 and below (phones included). Numbers below are given as rem with the px at 1920 in parentheses. The live build is a newer cut than `development` in a few places (for example the row title is a `.title` inside `.title-container`); where they differ, live wins and the row says so. Guests also see no like or love buttons.

Not reachable signed out, so taken from src only: the Library grid, the Calendar grid and list, Continue Watching cards, the like and love rating buttons, and the player menus beyond the control bar.

## Tokens

### Color (css `:root`, src `src/App/styles.less`)

| Role | Token | Value | Notes |
|---|---|---|---|
| Page | `--primary-background-color` | #0C0B11 | `html` background |
| Page gradient | `body` | `linear-gradient(41deg, #0C0B11 0%, #1A173E 100%)` | purple-indigo cast in the top right (live) |
| Panels, menus, modals | `--modal-background-color` | #0F0D20 | Discover preview, dropdowns, nav menu, dialogs, player menus |
| The one fill | `--overlay-color` | rgba(255,255,255,.05) | posters before load, chips, pills, cards, hover on rows; 1.1:1 against the page, so it reads as a tint, not a surface |
| Side panel over art | (src MetaDetails) | rgba(0,0,0,.4) + `backdrop-filter: blur(15px)` | stream and episode panel |
| Text | `--primary-foreground-color` | rgba(255,255,255,.9) | 15.8:1 on #0C0B11 |
| Text levels | element `opacity` | .6, .44, .4, .3 | measured contrast on #0C0B11: .6 is 6.1:1, .44 is 3.7:1, .4 is 3.3:1, .3 is 2.3:1 (the uppercase GENRES, CAST, SUMMARY labels fail 4.5:1) |
| Selection accent | `--primary-accent-color` | #7B5BF5 | selected nav tab, progress bars, seek bar, links, today marker, Discover filter count; 4.35:1 on the page |
| Action accent | `--secondary-accent-color` | #22B365 | Install, Add addon, play circles, Show on hover, toggles on, dropdown selected dot; white text on it is 2.5:1 |
| Watched flag | `--tertiary-accent-color` | #F6C700 | episode "Watched" chip |
| Active chip | `--quaternary-accent-color` | #1245A6 | the active Library sort chip (`Chips`) |
| Live | literal | #E50914 | Live badge and "at live" seek label |
| IMDb | `--color-imdb` | #F5C518 | the IMDb mark only |
| Outer glow | `--outer-glow` | `0 0 15px rgba(123,91,245,.37)` | every dialog, dropdown menu and tooltip |
| Modal scrim | `@color-background-dark5-40` | rgba(0,0,0,.4) (live) | |
| Warning banner | (live) | #CB8E00, radius 0.5 rem, shadow `0 3.75px 15px rgba(0,0,0,.48), 0 7.5px 45px rgba(0,0,0,.64)` | "Streaming server is not available." |
| Placeholder | `--color-placeholder-background` | hsla(0,0%,50%,.2) | skeletons |

### Type (src `src/App/styles.less`, live)

| Use | Size | Weight | Notes |
|---|---|---|---|
| Family | Plus Jakarta Sans (self-hosted TTF), then Arial, Helvetica, a TwemojiFlags font for flag emoji only | | `-webkit-font-smoothing: antialiased` |
| Body, inputs, stream description | 1 rem (15) | 400 to 500 | |
| Nav rail label, episode date, flags | 0.8 rem (12) | 500 (flags 800 uppercase) | |
| Uppercase group labels (GENRES, SUMMARY) | 0.95 rem (14.25) | 700 | opacity .3 |
| Stream source name, Settings menu, addon buttons | 1.1 rem (16.5) | 400, 500, 700 | |
| Title page meta row (runtime, year, rating) | 1.25 rem (18.75) | 600 | |
| Row titles, addon names, search hint title | 1.6 rem (24) | 500 (addon name 400) | |
| Settings section title | 1.8 rem (27), line height 3.4 rem | 400 | |
| Empty-state message | 2 rem (30) | 400 | |
| Summary | 1 rem, line height 2 em (30) | 400 | generous leading, no clamp on the title page |

### Shape, elevation, motion (src, live)

- Radius: one `--border-radius` 0.75 rem (11.25) for posters, cards, rows, panels, menus, dialogs, calendar cells; pills (2 to 4 rem radius) for every button, chip, dropdown and the search field; 0.5 rem for the IMDb link and calendar art; 0.3 rem for episode stills; 0.25 rem for badges.
- Elevation: no shadows on cards or posters. Overlays (dialogs, menus, tooltips) carry the purple outer glow; chips and buttons over art carry `blur(5px)`; side panels `blur(15px)`.
- Motion: 0.1 s ease-out for hover fills, opacity and the poster zoom; 0.15 s on player controls; drawers slide 0.3 s `cubic-bezier(0.32, 0, 0.67, 0)`; route content fades in over 350 ms with a 4 px rise (`animation-fade-in`), and reduced motion keeps the fade without the rise. On touch, pressed buttons scale to 0.97, nav tabs and action groups to 0.9, posters to 0.95.

## Surfaces

### Navigation (src `components/NavBar`, `components/MainNavBars`; live; shot `board-1920.png`, `nav-menu-1920.png`, `board-phone.png`)

- Top bar 5.5 rem (82.5) tall, transparent over the page: the logo (2.5 rem, at the left above the rail), the search field centered (30 rem x 3.25 rem, 450x49, pill, 5 percent fill, placeholder "Search or paste link" at 60 percent, submit icon at the right), then fullscreen and the avatar menu at the right (3.5 rem squares, radius 0.75 rem, 60 percent opacity, hover 5 percent fill and full opacity).
- Left rail 6 rem (90) wide, under the top bar: Board, Discover, Library, Calendar, Addons, Settings as 4.8 rem (72) squares with radius 0.75 rem and 1 rem gaps. Icons 2.2 rem (33) at 35 percent; labels 0.8 rem (12) weight 500 exist but sit at opacity 0 on desktop and only show at 60 percent on hover. The selected tab turns icon and label purple; there is no fill or marker.
- Phone (below 640 wide): the rail becomes a bottom bar 6 rem (84) tall with five tabs (Settings drops), labels visible at 60 percent, selected label purple at 100 percent; the top bar keeps logo, a narrower search field and the avatar.
- Title pages swap the logo for a back chevron and keep fullscreen and the avatar.
- Avatar menu: a 22 rem (330) panel on #0F0D20, radius 0.75 rem, no glow: the user block (4 rem avatar, email or "Anonymous user", "Log in / Sign up"), then 4 rem rows with a 2 rem icon at 30 percent: Settings, Addons, Play URL/Magnet link, Help & Feedback.

### Board (home) (src `routes/Board`, `components/MetaRow`; live; shot `board-1920.png`, `board-1440.png`)

- No hero. The page is a stack of catalog rows, one per installed catalog in addon order: "Popular - Movie", "Popular - Series", "Featured - Movie", and so on (Continue Watching first when signed in). Content padding 0 1 rem; rows margin 1 rem top, 2 rem bottom (a 410 px pitch for poster rows at 1920).
- Row header: the catalog title 1.6 rem (24) weight 500 at the left, a "See All" pill at the right (2.5 rem tall, 60 percent opacity, label 500, a chevron; hover 5 percent fill and full opacity) that opens the catalog in Discover.
- Items per row by window width (the row shows the first N, no horizontal scroll on desktop): 9 at 1900 to 2200 (1920 measured: 9), 8 at 1600 to 1900, 7 at 1300 to 1600 (1440 measured: 7), 6 at 1000 to 1300, 5 at 800 to 1000, 4 below; landscape rows show one fewer. Phone: rows scroll sideways with `scroll-snap-type: x mandatory`, posters 31 percent wide (three and a sliver visible), landscape 46 percent.
- A floating warning card sits at the bottom when the local streaming server is missing (yellow fill, "Install", "Later", "Don't show again" in 24 percent black pills; shot `board-with-warning-1920.png`).

### Poster card (src `components/MetaItem`; live; shot `board-hover-1920.png`)

| Part | Measured (1920) |
|---|---|
| Cell | 200 px wide, padding 1 rem (15) on all sides, so posters sit 30 px apart |
| Poster | 170x249, ratio `--poster-shape-ratio` 1.464 (not 1.5), radius 0.75 rem (11.25), 5 percent fill under the image, image `object-fit: cover` at 90 percent opacity |
| Other shapes | square (1:1) and landscape (0.5625, so 16:9) from the catalog's `posterShape` |
| Title | under the art in a 4 rem (60) bar, 1 rem weight 600, centered, two lines max |
| Hover, keyboard focus (`:focus-within`), selected | a ring `box-shadow: 0 0 0 0.2rem` white at 90 percent (3 px) on the poster, the image zooms to 1.05 inside the clipped poster (0.1 s ease-out); the card itself does not move; a "more" dots button appears at the bottom right |
| Watched | a 1.5 rem purple circle with a check at the top left |
| New episodes | three stacked white tags at the top right (40, 60, 100 percent) with the count in purple |
| Continue Watching (src) | a 0.45 rem white progress bar inset 1 rem from the poster's bottom on a 30 percent white track; a dismiss X (1.5 rem circle on 60 percent page color) at the top left on hover; a centered play circle (4 rem, white ring, turns green on hover) |
| Live | a red #E50914 "LIVE" badge, 1.5 rem tall, radius 0.25 rem, top right |

### Discover (src `routes/Discover`; live; shot `discover-1920.png`, `discover-menu-1920.png`)

- A toolbar of dropdown pills (padding 1.5 rem): Type (one entry per content type the installed catalogs offer: Movie, Series, Channel for the guest addons), Catalog (Popular, New, Featured, Public Domain Movies), Genre; each 15 rem (225) by 3 rem, radius 2.75 rem, 5 percent fill. A fourth or later filter folds into a filter button; on phone Type and Catalog stay and the rest fold into a "Filters" button with a purple count.
- A poster grid under it with 0.5 rem gaps: 10 columns at 2800 and wider, 9 to 2800, 8 to 2500, 7 to 2200 (1920 measured: 7 columns of 183), 6 to 1900, 5 to 1600, 4 to 1300, then 5, 4, 3 once the preview panel is gone (below 1000, 800, 640). Phone: 3 columns.
- A preview panel at the right, 29 rem (435) wide, full height under the top bar, #0F0D20, radius 0.75 rem on the left corners only, padding 2 rem 2.5 rem, the title's background blurred 10 px at 30 percent behind it. It is the compact form of the title page info: logo 6 rem tall centered, runtime and year spread to the edges, the description, the chip groups, then an action row pinned to the bottom (Trailer round, Library and Watched pair, a wide "Show" pill that turns green on hover).
- Selection model: one click selects a poster (white ring) and fills the preview; Show or a second click opens the title page. The first item is selected on load.
- Dropdown menu: #0F0D20, radius 0.75 rem, purple glow, options 3 rem (45) tall with 1 rem padding, the current one marked by a 0.5 rem green dot at the right.

### Title page (meta details) (src `routes/MetaDetails`, `components/MetaPreview`; live; shots `detail-movie-1920.png`, `detail-series-1920.png`, `detail-movie-actions-hover-1920.png`)

Layout: the title's background image fills the window behind everything at 30 percent opacity over #0F0D20 (`object-fit: cover`, anchored right); the top bar floats over it; under the bar, the info column at the left (padding 0 4 rem 2 rem, so 60 px from the left edge) and the 30 rem side panel at the right; the space between stays empty art.

Info column, top to bottom:

| Part | Measured (1920) |
|---|---|
| Logo | 9 rem (135) tall, up to 30 rem (450) wide, `object-fit: contain`, aligned left, 2 rem below it; if there is no logo, the name as text at 1.7 rem |
| Meta row | 1 rem below: runtime ("148 min"), release info ("2010", or a year range for series, 2008 to 2013 in the shot), then the IMDb link; labels 1.25 rem (18.75) weight 600, 3 rem (45) between items, no separators |
| IMDb link | the rating as text ("8.8", same 1.25 rem 600) followed by the IMDb mark (an icon 3 rem (45) square in #F5C518); radius 0.5 rem, a 2 px transparent border that turns white on focus; it opens the IMDb page through stremio.com's leave-site warning |
| Link groups | "GENRES", "CAST", "DIRECTORS" (and "WRITERS" in data but not shown): uppercase label 0.95 rem weight 700 at 30 percent, 0.75 rem above the chips; groups 1.5 rem apart |
| Chips | pills (radius 2 rem), padding 0.4 rem 1.25 rem (35 px tall), weight 500, 5 percent fill with `blur(5px)`, 0.75 rem gaps; hover fill 30 percent white; focus a 2 px white border; each chip is a link (genre to Discover, person to Search) |
| Summary | "SUMMARY" label (same style), then the text at 1 rem with 2 em leading (30 px), full width of the column, no clamp |
| Action row | pinned to the bottom of the column (padding-top 3.5 rem): a wide "Trailer" pill (4 rem (60) tall, padding 0 2 rem, icon 1.75 rem and label 500 at 90 percent), a paired pill holding Add to Library and Mark as watched (two 4 rem icon cells, icons 2 rem at 70 percent, full on hover), a round Share (4 rem); all 5 percent fill with `blur(5px)`, 1 rem apart |
| Action states | hover and focus: the fill drops to transparent and a 2 px white outline appears (live); a tooltip names icon-only actions (2.5 rem tall, #0F0D20, radius 0.75 rem, purple glow, 0.1 s fade) |
| Personal rating (src only) | a paired pill with thumbs up and heart (Like, Love) beside the actions, only for signed-in users |

The side panel (30 rem (450) wide, full height under the top bar, rgba(0,0,0,.4) with `blur(15px)`, radius 0.75 rem on the left corners) shows, for a movie, the stream list; for a series, the episode list; for a chosen episode, that episode's stream list with a back chevron to the episodes.

Phone: one scrolling column; the logo centers (2 rem margins), the meta row spreads to the edges, the action row moves under the summary, and the panel follows below with rounded top corners (shot `detail-movie-phone.png`, `detail-movie-streams-phone.png`).

### Stream list and its grouping by addon (src `routes/MetaDetails/StreamsList`; live; shots `detail-movie-stream-hover-1920.png`, `detail-pd-tt0063350-1920.png`, `detail-pd-tt0063350-addon-menu-1920.png`, `detail-episode-1920.png`)

- Grouping: streams arrive per addon; the list shows all of them in one flat list in the core's addon order, with no group headers. When more than one addon answered, a dropdown at the top of the panel (full panel width, 3 rem pill) filters the list: "All" plus each addon by its manifest name ("All", "WatchHub", "Public Domain Movies" measured). Choosing one scrolls the list to the top. With a single addon the dropdown is hidden.
- Header for an episode: a back chevron (0.5 rem radius, 60 percent) and "S1E1 Pilot" in one line before the list.
- Row: padding 0.5 rem 1 rem, gap 1 rem, radius 0.75 rem, 0.5 rem apart, 420x60 at 1920. Left column 7 rem (105) wide: the stream's own name, or the addon name when the stream has none, at 1.1 rem (16.5), wrapping as the addon wrote it ("Netflix", "Amazon Video", "1080p"); or an 8 x 5 rem thumbnail when the stream has one. Then the description, the addon's text kept verbatim with its line breaks (`white-space: pre`), for example "Subscription", "Buy", or a size line with an emoji. Then a play circle, 3 rem (45), green, hidden until hover or focus. The row's tooltip is the addon name.
- States: hover 5 percent fill and the play circle; keyboard focus a 2 px white outline plus the fill; the stream played last carries a 0.4 rem purple progress bar under its name column.
- Right click or long press opens a menu titled with the description: Play, Copy stream link, Copy magnet link, Copy download link.
- Loading: "N addons loading" and a 0.3 em purple bar at the bottom while any addon is still answering; two skeleton rows meanwhile.
- Empty: an illustration (10 rem), "No streams were found" at 1.4 rem, and for series an episode picker (season and episode inputs) to look up another episode.
- An "Install addons" pill (green, 4 rem tall, 1 rem weight 700 with a puzzle icon) ends the list for guests and new users and opens the addon catalog.

### Episodes panel (series) (src `routes/MetaDetails/VideosList`, `components/Video`; live; shots `detail-series-1920.png`, `detail-series-season-menu-1920.png`, `detail-series-episode-hover-1920.png`)

- Season bar: "Prev" and "Next" pills (6.5 x 3 rem, radius 3 rem, label 500, chevron 1.5 rem; Prev at 50 percent when there is no previous season) around a centered "Season 1" dropdown (seasons 1 to N, then "Special").
- A search field ("search videos", 3 rem pill, 5 percent fill) filters episodes by title.
- A "Receive notifications" toggle above the bar for series in the library (src).
- Episode row: padding 0.5 rem, radius 0.75 rem, 0.5 rem apart (420x94 at 1920): a 16:10 still 8 x 5 rem (120x75) at 90 percent opacity, blurred 0.5 rem when the "Blur unwatched episodes" setting is on; then the title "1. Pilot" (two lines max) and the air date "Jan 21, 2008" at 0.8 rem weight 500, 44 percent; flags at the right: "UPCOMING" green, "WATCHED" yellow, "LIVE" red, 1.6 rem tall chips with 0.8 rem weight 800 uppercase dark text; a purple 0.4 rem progress bar inside the still for a started episode.
- States: hover and focus 5 percent fill; the episode chosen from elsewhere is scrolled to and outlined in purple that fades out over 3 s. Right click: Watch, Mark as watched, Mark season as watched.
- Click an episode: the panel turns into that episode's stream list (above). A round play button shows on the row only when the episode already has a direct player link.

### Library (src `routes/Library`; live signed out; shot `library-1920.png`)

- Signed in: a type dropdown pill (2.75 rem) and sort chips beside it (2.75 rem pills, transparent at 60 percent, hover 5 percent fill, the active sort filled #1245A6 blue; default "Last watched"), then the poster grid: 10 columns at 2200 and wider, 9 to 2200, 8 to 1900, 7 to 1600, 6 to 1300, 5 to 1000, 4 to 800, 3 on phone; the poster card as above with progress and watched marks.
- Signed out: a centered message at 1.75 rem and 50 percent ("Library is only available for logged in users"), a fanned poster illustration, two benefit lines with line icons (1.1 rem, 500), and a "Log in" pill (4 rem tall, 5 rem side padding, 5 percent fill). Calendar uses the same placeholder (shot `calendar-1920.png`).

### Calendar (src `routes/Calendar`)

- A month selector centered at the top: "Prev" and "Next" (6 x 3 rem, radius 0.5 rem, labels at 50 percent) around the month at 1.5 rem weight 500 with the year above it at 1 rem, 50 percent.
- Main area: a 7-column month grid (week header 3 rem; cells 5 percent fill, 1 px gaps, only the four outer corners rounded) and, at the right, a 24 rem list of the month's days with episodes.
- Cell: the day number in a 2 rem circle; today in a 1.5 rem purple circle; that day's episodes as 2:3 posters in a horizontal strip (radius 0.375 rem); hover dims the poster to 50 percent and shows a green play circle; the selected day gets a 0.15 rem white border, hover a 5 percent border.
- List card per day: a 3.5 rem heading (today filled purple), then episode rows: 16:9 art 8 rem wide, radius 0.5 rem; "S1E3" at 0.8 rem weight 600 in purple with tabular figures; the show name 1 rem weight 600; the episode title 0.875 rem at 65 percent. Below 1300 wide in portrait the list hides; on phones the posters in a cell stop being links, so a tap selects the day.

### Addons (catalog, card, details, install) (src `routes/Addons`, `components/AddonDetailsModal`; live; shots `addons-1920.png`, `addons-remote-1920.png`, `addons-catalog-menu-1920.png`, `addon-details-modal-1920.png`, `addons-phone.png`)

- Toolbar (padding 1.5 rem): a source dropdown ("Installed", "Official Addons", "Community Addons"), a type dropdown ("All", "Movie", "Series", ...), a spacer, a green "Add addon" pill (3 rem, a plus icon, label 700) that asks for an addon URL, and a search field 18 rem (270) wide.
- Card: one full-width row per addon (1785x169 at 1920), padding 1.5 rem, radius 0.75 rem, 5 percent fill, a 0.15 rem transparent border that turns 5 percent white on hover; 1.5 rem between cards.
- Card content, left to right: the logo in an 8 rem (120) square (`contain`, a puzzle icon when the addon has none); then on one baseline the name at 1.6 rem (24) weight 400 and the version "v.3.0.14" at 60 percent; under it the types ("Movie & Series", capitalized) at 40 percent; then the description at full white. At the right a 17 rem (255) column: the main button (3.5 rem pill, 1.1 rem weight 700): "Install" green, or "Uninstall" as a 2 px white outline at 30 percent that goes to 100 percent on hover; a round green gear before Install when the addon needs configuring; and a "Share addon" text button with an icon under it.
- Details dialog (a click on the card, installs nothing): 40 rem (660 at 1920 including padding) wide, #0F0D20, radius 0.75 rem, the purple glow, the addon's background art at 10 percent across the whole dialog; header "Stremio Addon" (1.2 rem weight 500, 4.5 rem tall) with a close X; the logo (5 rem), name (1.6 rem) and version on one line; then the description, "URL:" with the manifest URL as selectable text, "Supported types: anime, movie & series", a watch-activity warning box (5 percent fill with an icon) when the addon receives what you watch, and an italic disclaimer ("Using third-party addons will always be subject to your responsibility ..."); buttons "Cancel" (transparent, 30 percent) and "Install" (green pill, 3.5 rem), splitting the width.
- Phone: the card wraps; logo and text on top, the main button full width under them, Share as an icon only.

### Search (src `routes/Search`; live; shots `search-1920.png`, `search-results-1920.png`, `search-focus-1920.png`)

- The field lives in the top bar on every page; it accepts a query or a pasted link (HTTP, magnet, IMDb or TVDB URL), and shows recent searches in a menu under it.
- Empty page: "You Can Search Anything..." at 1.6 rem and 40 percent, centered, over four hint tiles (4 rem line icons and 1.2 rem labels, all at 40 percent): "Movies, Series, YouTube & TV", "Actors, Directors & Writers", "HTTP & Magnet Links", "IMDB / TVDB links".
- Results: the Board row pattern, one row per searchable catalog ("Popular - Movie", "Popular - Series"), each with See All; no list view, no ratings on results.

### Settings (src `routes/Settings`; live; shots `settings-1920.png`, `settings-toggles-1920.png`, `settings-phone.png`)

- A left menu 18 rem (270) wide (padding 3 rem 1.5 rem): General, Interface, Player, Streaming, Shortcuts as 4 rem (60) pills, 1.1 rem weight 500 at 40 percent; the current one weight 600 at full white on the 5 percent fill. The page is one long scroll; the menu jumps to and tracks sections. "App Version" and "Build Version" sit at the bottom of the menu at 30 percent. The menu hides below 1000 wide.
- Sections: a 35 rem (525) column, padding 3 rem 0, a 5 percent hairline between sections, section title at 1.8 rem (27). General starts with the account block (a 5 rem avatar in a 2 px purple ring, "Anonymous user", "Log in / Sign up" link) and plain links (Contact support, Source code, Terms of Service, Privacy Policy) in purple.
- Option row: label at the left (line height 1.5 rem, two lines max), control at the right, 2 rem apart, 2 rem between rows. Controls: a dropdown pill (3 rem, 5 percent fill); a button pill (3.5 rem, 5 percent fill, hover outline: "Authenticate" for Trakt); a toggle 3.2 x 1.7 rem (48x25.5) with a white thumb, 5 percent track off, green on; the interface zoom as a tick slider (75, 100, 125, 150, 175 percent); color inputs for subtitle colors.
- Player settings group into categories (Subtitles, Audio, Controls, Auto Play, Advanced) with a 2 rem icon and a 1.1 rem heading (4 rem row).

### Player chrome (src `routes/Player`; live trailer page, paused, muted, media blocked; shots `player-trailer-1920.png`, `player-trailer-hover-1920.png`)

- Black stage. A top layer with the title and the back chevron over an 8 rem gradient `linear-gradient(to bottom, rgba(0,0,0,.35), transparent)`; a bottom layer over a 10 rem gradient the other way. Both hide while the video plays and the pointer rests, and stay while paused or while a menu is open.
- Control bar (padding 0 2 rem): a seek row 2.5 rem tall with elapsed and total time at each end (5.5 rem wide, 1.1 rem, tabular figures, 90 percent) and a 0.4 rem track; elapsed and thumb purple (thumb 1.2 rem, scales 1.2 on hover); a time tooltip over the track on hover. Under it, 4 rem (60) square buttons with radius 0.75 rem and 2.2 rem icons, hover 5 percent fill, disabled 40 percent: play or pause, next episode, volume with a 0.35 rem slider, then at the right (src order) streaming statistics, speed, cast, subtitles, audio tracks, episodes, video scale and a more menu; the trailer page showed statistics, cast and subtitles.
- Menus open in one panel at the bottom right (4 rem from the right, 7.5 rem from the bottom), #0F0D20 with `blur(15px)`, radius 0.75 rem. Subtitles is three columns (16, 16 and 17 rem): languages, that language's variants, and settings (delay, size, vertical offset); option rows 3.5 rem, current one 5 percent fill and a 0.5 rem green dot.
- A side drawer (up to 35 rem, #0F0D20, radius on the left corners) reuses the title page info and the episode list inside the player.
- Phone: the right-side buttons fold into one menu button.

### Player loading (src `routes/Player/Buffering/Buffering.tsx`, `Buffering.less`, `Player.js`, `useStatistics.ts`, `styles.less`; core `models/streaming_server.rs`; live; shots `player-loading-fast-0-1920.png`, `player-loading-recon-*.png`)

Sources: the component arrived in stremio-web commit `b74e6e9` (2026-05-18, "feat(player): add buffering indicator for torrents"); its progress formula was reworked in `ccdf2a4` (2026-08-20, "feat(player): improve stream statistics") and `8a051b1` (2026-10-02). The live bundle (`f0222603`, `scripts/main.js`) carries the same code: `clipPath:"inset(0 ".concat(100-E,"% 0 0)")` and the same formula, and the live stylesheet has `.buffering .logo { animation: 2s ease 0s infinite pulse; max-width: 15rem; max-height: 15rem; transition: clip-path 0.1s ease-in-out }` and `.logo.background { opacity: 0.2 !important }`.

When: the layer shows while `(video.state.buffering || !video.state.loaded) && !error`, so on the first load and again on any later stall. The `buffering` and `loaded` flags come from the player library (stremio-video); only the fill amount comes through the core.

| Part | Value |
|---|---|
| Stage | the player container is #000. Until the video reports loaded, a background layer shows the title's `background` image full screen, `object-fit: cover`, opacity 0.6, no blur and no gradient of its own (live: opacity 0.6, filter none, over rgb(0,0,0)). During a later stall there is no backdrop: the paused video frame stays under the logo |
| Logo | two copies of the title's `logo`, stacked and centered in a full-screen flex layer, each `max-width: 15rem; max-height: 15rem` with the aspect kept (225 px at a 1920 wide window, 210 px at 1440). A wide logo such as tt0063350's renders 225x87 at 1920, 236x92 at the pulse peak |
| Faint copy (behind) | opacity forced to 0.2 with `!important`, so the pulse cannot change it; it still runs the pulse's scale |
| Fill copy (front) | `clip-path: inset(0 {100 - progress}% 0 0)`: the full-strength logo is revealed from its left edge toward the right as progress rises; `transition: clip-path 0.1s ease-in-out` |
| Pulse (both copies) | `@keyframes pulse`: 0% opacity .4 and scale 1, 50% opacity 1 and scale 1.05, 100% back; 2 s, `ease` (the default, as no timing is written), infinite. There is no reduced-motion rule for it |
| No logo | the Image component's `fallbackSrc`: Stremio's own symbol (`/assets/images/stremio_symbol.png`) in the same two-copy treatment |
| Text | none in the layer. The nav bar (back chevron and the title) and the control bar (seek bar reading 00:00:00, buttons) stay on screen, because the overlay hides only while the video plays (`overlayHidden` needs `paused === false`). Peers, speed and completed percent live in the Statistics menu, never on the loading screen |
| Colors | none of its own: the logo's own colors at 20 percent behind and 40 to 100 percent in front, over the title backdrop at 60 percent on black |

What drives the fill (`useStatistics.ts`): it follows real torrent progress and does not loop. For a torrent stream (it has an `infoHash`), the web app dispatches `StreamingServer` `GetStatistics` at once and then every 5000 ms; the core fetches `{streaming server}/{infoHash}/{fileIdx}/stats.json` (live request: `http://127.0.0.1:11470/11ea0258.../0/stats.json`). The fill is

`progress = min(99, 20 × min(1, peers / 8) + 70 × min(1, downloaded / T) + 10 × min(1, downloadSpeed / 1 MiB/s))`, with `T = clamp(0.8 percent of the file length, 2 MiB, 8 MiB)`.

It is 0 until the first statistics arrive (only the faint copy shows), never reaches 100 while loading (cap 99), and the layer simply disappears when the video reports loaded. A stream without an `infoHash` (HTTP, YouTube) gets 100 at once, so the full logo pulses with no fill. Because statistics arrive every 5 s and the clip transition is 0.1 s, the fill moves in visible steps every 5 s while the 2 s pulse runs on. Worked example, the 1.51 GB public domain file: T is 8 MiB (0.8 percent would be 12.1 MB); with 4 peers, 4 MiB downloaded and 0.5 MiB/s the logo is 10 + 35 + 5 = 50 percent filled.

Live check (2026-10-21, signed out, muted, media blocked, tt0063350, the Public Domain Movies "1080p" torrent): the layer rendered for about 100 ms (frame at 96 ms: front copy `clip-path: inset(0px 100% 0px 0px)` and opacity 0.4, back copy 0.2, `pulse 2s ease`), then the player showed "Video is not supported", because no local streaming server runs and `stats.json` failed. The first screenshot (`player-loading-fast-0-1920.png`, also `ref/stremio-web/062`) already shows that error over the real stage (backdrop at 60 percent, title top left, control bar), so neither the logo nor its fill could be captured live. `player-loading-recon-p0`, `-p35`, `-p70`, `-p100` (scratchpad) are a reconstruction: a page with Stremio's exact CSS and markup and the same title's logo and background, with progress set by hand. They are labeled as such and are not live captures.

### Subtitles menu (src `routes/Player/SubtitlesMenu/SubtitlesMenu.js`, `styles.less`, `SubtitleVariant/`, `Stepper/`, `useSubtitles.ts`, `subtitleDelay.ts`, `common/languages.ts`, `common/CONSTANTS.js`; core `models/player.rs`, `types/player.rs`, `types/streams/streams_item.rs`, `types/profile/settings.rs`, core-web `model/serialize_player.rs`; stremio-video `withHTMLSubtitles.js`, `withStreamingServer/fetchVideoParams.js`; apk-tv `views/player/menus/SubtitlesMenuKt`, `LanguageMenuListKt`, `composables/MenuKt`; apk-phone `ui/screens/player/SubtitlesMenuKt`, `ui/composables/MenuKt`, `common/extensions/PlayerExtKt`, `common/translations/LanguageKt`)

The live bundle (`f0222603`) carries the same component: the origin order `["LOCAL","EMBEDDED","EXCLUSIVE"]`, the same language sort and the same markup. It adds one thing the `development` source lacks: for a signed-in Premium user with an addon subtitle selected, a purple "Live Sync" pill (`SUBTITLES_LIVE_SYNC_SHORT`, `--primary-accent-color` fill, radius 2 rem, padding 0.5 x 1 rem) takes the place of the Delay stepper. The menu only opens over a playing stream with tracks; here every stream fails without a local streaming server, so the shots are a reconstruction (below).

**Web layout.** One panel in the player's menu layer (bottom right, 4 rem from the right and 7.5 rem from the bottom; #0F0D20 with `blur(15px)`, radius 0.75 rem, shadow `0 1.35rem 2.7rem rgba(0,0,0,.4), 0 1.1rem 0.85rem rgba(0,0,0,.2)`; max height `100% - 13rem`, max width `100% - 4rem`). The panel is 25 rem tall and holds three columns side by side, with no second page:

| Column | Values |
|---|---|
| Languages, 16 rem | header "Subtitles Languages" (700, padding 1.5 x 2 rem); a scrolling list with 1 rem side padding; rows 3.5 rem tall, 1.5 rem side padding, 0.5 rem apart, radius 0.75 rem, label 1.1 rem in 90 percent white, one line with ellipsis; the first row is "OFF" |
| Variants, 16 rem | header "Subtitles Variants"; rows 4 rem tall, two lines: the label (1.1 rem, line height 1.5 rem) and the origin under it (0.9 rem, `hsla(0,0%,75%,.5)`), plus an "ASS" badge (0.65 rem, 1 px border, radius 0.25 rem) for styled subtitles; with Off chosen, the column reads "Subtitles are disabled" |
| Settings, 17 rem | header "Subtitles Settings"; three steppers, each a label at 60 percent over a 3.5 rem pill (5 percent fill) with round minus and plus buttons (3.5 rem, 5 percent fill) and the value in weight 500: Delay (0.1 s steps; holding repeats after 250 ms every 100 ms and speeds up from 3x to 6x over the hold), Size (75, 100, 125, 150, 175, 200, 250 percent), Vertical Position (0 to 100 percent, step 1). A stepper shows `--` and dims to 40 percent when it does not apply (delay only works for addon subtitles; size and position are off while ASS styling is active) |

- **States.** The selected language and the selected variant get the 5 percent fill and a 0.5 rem green dot (#22B365) at the right; hover gives the same fill. There is no separate mark for an automatically chosen variant: it is simply the selected one.
- **No search, no flags, no icons.** The language list is a plain list of names; nothing filters it.
- **Language names** come from Stremio's `langs` package (`Stremio/nodejs-langs`): the native name (`local`), for example "English", "Čeština", "Português (Brasil)", "Српски". Codes are normalized to ISO 639-2 first (`toCode`), so "cze" and "ces" are one language.
- **Language order:** sorted by that three-letter code (not by name), then three are pulled to the top in this order: "Local" (a subtitle file dropped onto the player), the preferred subtitles language from Settings, the interface language. For tt0063350's 69 OpenSubtitles results this gives English, Български, Čeština, Suomi, Français, Magyar, Latviešu, Português (Brasil), Polski, Português, Română, Slovenčina, Español, Српски, Svenska: Español lands after Slovenčina because "spa" sorts after "slk".
- **Variant order within a language:** origin first, "LOCAL", then "EMBEDDED" (tracks inside the file), then "EXCLUSIVE" (subtitles that ship with the stream itself, `stream.subtitles`), then every addon's results in addon order and in the order each addon returned them (core-web flattens `player.subtitles` per addon, origin = the addon's manifest name).
- **Variant label:** the track's `label` when it has one that is not a URL, else the language name again. OpenSubtitles v3 returns no `label` (it returns `subtitleFileName`, `movieReleaseName`, `releaseGroup`, `releaseFormat`, which the web ignores), so its variants all read "English / OpenSubtitles v3" and differ only in the hover title (the last part of the download URL, a number). A right click on an addon variant offers Download subtitle, Copy subtitle URL, Copy subtitle ID.
- **Choosing a language** selects its first variant in the order above at once (one click turns subtitles on); the variants column is for changing it.
- **Phone width:** the portrait rule only moves the panel (2.5 rem from the right, 11 rem from the bottom). At 390 px wide the panel is 334 px, so of the three columns (686 px together) only the languages and part of the variants show: the global `* { overflow: hidden }` clips the rest (seen in the reconstruction, `ref/stremio-web/069`).

**How subtitles are gathered (core and stremio-video).**
- Embedded tracks come from the file through the player (`origin: 'EMBEDDED'`); a file dropped on the player becomes `origin: 'LOCAL'` with language `local`; the stream's own `subtitles` are added on load as `origin: 'EXCLUSIVE'` (`withHTMLSubtitles.js`).
- Addon subtitles: the core asks every installed addon with a `subtitles` resource at once (`AggrRequest::AllOfResource`), adding three extra properties: `videoHash` (the stream's `behaviorHints.videoHash`, else the OpenSubtitles hash the streaming server computes at `/opensubHash?videoUrl=...`), `videoSize` and `filename` (`subtitles_update` in `models/player.rs`). The request waits until at least one of them is known. The matching itself happens inside the addon. OpenSubtitles v3 is hosted and closed source; its results carry a match field (`"m": "i"`, matched by IMDb id, in every row fetched). Adding a release `filename` alone did not change its order for Inception (90 results, tested), and a hash match could not be tested without a local file.

**How the default is picked (`useSubtitles.ts`).** Nothing is picked when the preferred subtitles language is unset (Settings: none). Otherwise the first match in this list wins:
1. the track saved for this exact stream (`streamState.subtitleTrack`, by id; carried to the next episode of the same binge group only when it is embedded, and then matched by id and language);
2. this session's chosen language (`SubtitlePreference`: enabled, source, language, kept across Player loads within the session), else the saved track's language;
3. the preferred subtitles language from Settings (`subtitles_language`, default "eng");
4. during a session where the user turned subtitles on, English as a last resort.

For each language, embedded tracks are tried before addon tracks, unless the saved or session source was an addon, in which case addon tracks come first; within a source the first track in list order wins. The core's settings also hold `secondary_subtitles_language` and `subtitles_auto_select` (default true), but the web app reads neither.

**Android TV (apk-tv).**
- Subtitles open a Compose side panel titled "Subtitles" (19 sp bold), at most 70 percent of the screen wide, with two lists side by side, languages and that language's tracks, over the Delay, Size and "Vertical Offset" rows (each opens its own picker).
- Rows are 42 dp tall with 10 dp corners. Focus gives the light #DCDCDC fill with dark text; the selected row (unfocused) is purple #7B5BF5 with a checkmark; other rows are white with no fill. Labels marquee when focused; a codec chip (`toDisplayCodec`, radius 6 dp, 10 percent fill) sits on each track row.
- Languages: distinct three-letter codes from the tracks, named in their own language (`Locale.getDisplayName` in that locale, first letter capitalized; "und" reads "Original"), sorted "Original" first, then the primary preferred language, then the secondary one, then alphabetically by name, with "Disabled" on top.
- A setting "Show only preferred languages for subtitles" (`subtitlesHideOtherLangs`) drops every language except the primary and the secondary.
- Auto-pick (`PlayerPage` effect): once the video is loaded and not buffering, if no track is chosen yet and "Automatic subtitles selection" is on, it picks by saved track id, then saved language, then "Default Subtitles Language", then "Secondary Subtitles Language" (`findTrack`, first track of the language in list order). Audio follows the same rule with its own two languages and falls back to the first track.
- No flags: the only flag drawables are media3's own `media3_icon_flag_*` icons, which the menu does not use.

**Android phone (apk-phone).**
- The same two lists side by side (languages and the chosen language's tracks, with a 1 dp divider) in a Material 3 bottom sheet titled "Subtitles" (18 sp). Below them on a portrait screen, or beside them in landscape, sit the Delay, Size and "Vertical Offset" steppers.
- Rows are 50 dp tall; the selected one is purple with a checkmark; a 6 dp codec chip shows on tracks.
- Languages are named the same way as on TV but appear in the order of the tracks, not sorted. Tapping a language picks its first track. A track's label is its name from the addon (`Subtitle.name`, else its id), else the language name.
- The same auto-pick as TV (saved id, saved language, primary, secondary), gated by "Automatic subtitles selection".
- No search and no flags.

Shots (reconstruction, not live): `ref/stremio-web/067` (1920), `068` (1440) and `069` (phone, 390). They rebuild the web menu with its exact markup and CSS values, the real OpenSubtitles v3 list for tt0063350 (69 results, 15 languages), Stremio's own language names and Plus Jakarta Sans from the APK; English is selected; the stepper glyphs are plain lines, not Stremio's icons. Generator: scratchpad `stremio-study/recon-subs.ts`.

## Ratings: IMDb and Rotten Tomatoes

- IMDb: shown only in the title page meta row and in Discover's preview panel, as the rating number (1.25 rem weight 600) followed by the IMDb mark in #F5C518, after runtime and year with 3 rem gaps; the pair is one focusable link to the title's IMDb page. The number comes from the metadata addon's `links` entry in the `imdb` category (its `name` is the rating, for example "8.8"); Cinemeta supplies it. No rating appears on posters, Board rows, search results, the calendar or the library, and there is no star glyph anywhere.
- Rotten Tomatoes: not shown anywhere. No string, icon or class for Rotten Tomatoes, Metacritic or any second rating exists in the source (`grep -ri "rotten\|tomato\|metacritic" src` returns nothing) or in the live stylesheet. The only other "rating" is the signed-in user's own Like or Love.
- The Android apps match: the TV preview and the phone title page show the IMDb number with the IMDb mark tinted #F5C518 (`imdbIconColor`, `Colors.imdb`), and neither APK has any Rotten Tomatoes or Metacritic string, icon or code.

## Stremio Android and Android TV

### Sources (apk)

| Tag | Source |
|---|---|
| apk-tv | "Stremio for Android TV" 1.10.4, `com.stremio.one`, `com.stremio.one-1.10.4-33145732-arm64-v8a.apk` from dl.strem.io as linked on stremio.com/downloads (95.7 MB, sha256 `caaf4fab...993c`); the manifest declares `android.software.leanback` and a `LEANBACK_LAUNCHER` banner |
| apk-phone | "Stremio for Android Mobile" 2.3.2, `com.stremio.one`, `com.stremio.one-2.3.2-7145730-arm64-v8a.apk` (94.3 MB, sha256 `b5ad3fef...c91d`) |
| how | downloaded into empty scratchpad folders and never installed or run. Resources decoded with jadx 1.5.5 (`jadx -s`: `res/values` colors, dimens, styles, fractions, strings, and the binary XML layouts and drawables); app code decompiled with jadx from the dex files that hold `com.stremio.*` (TV: `classes5`, `6`, `7`, `9`; phone: `classes5`, `6`, `8`), read for Compose values (`Dp`, `sp`, `Color(...)` constants, focus branches) |

Neither app is a web view or a JavaScript bundle. Both are native Kotlin on the same Rust core (`libstremio_core_kotlin.so`), with libvlc and libmpv for playback and a bundled Node streaming server (`libnode.so`, `assets/server.js`). The TV app uses Leanback rows and cards in XML layouts (data binding) for Home, Discover, Library, the title page and Addons, and Jetpack Compose for the side menu, Settings, profiles, sign-in and the player overlay. The phone app is all Compose. Both bundle Plus Jakarta Sans (regular, medium, semibold, bold, italic) and a `roboto_medium_numbers.ttf` for figures.

Units: dp and sp. An Android TV at 1080p reports density 2, so the TV canvas is 960x540 dp and every TV number below is half its 1080p pixel size.

### TV tokens (apk-tv `res/values`, `com.stremio.tv.Colors`, `Shapes`, `FontSizes`)

| Role | Value | Notes |
|---|---|---|
| Window | `primaryColor` #0C0C10 | neutral near-black, no gradient (the web's purple cast is gone) |
| Card | `secondaryColor` #18181C | poster, episode and stream card fill |
| Text | `surface_light5_90` (white 90 percent); secondary `surface_light5_50` (white 50 percent, 5.3:1 on the window) | theme `textColorPrimary` and `textColorSecondary` |
| XML button | idle `overlayColor` (white 4 percent) with 50 percent text; focused `tertiaryColor` #26262D with 90 percent text; radius 50 dp, 20 dp side padding, 10 dp icon gap | `button_background`, `button_text_color` |
| Compose focus | focused fill `secondaryBackground` #DCDCDC with label #0C0B11 (14.3:1); idle the same fill at 10 percent, or transparent; disabled alpha .5 | `composables/ButtonKt`, the side menu tabs and the player controls all use it |
| Card focus | stream cards add a 10 percent light grey overlay (`surface_light1_10`); addon cards turn 50 percent light grey (`surface_light1_50`) | `card_stream_foreground`, `card_stream_background` |
| Accents | purple #7B5BF5 (current tab, watched badge, playback progress); green #22B365 for Add addon and Install, #30D87D when focused and for switches on; yellow #F6C700; success #4CD866; error #DF2F4B | `defaultBrandColor`, `accent_button_color`, `settingsControlActivatedColor` |
| Radii | cards 10 dp (`cardViewCornerRadius`, Compose `Shapes.roundedCorner`); chips and badges 5 dp; dialogs 18 dp; buttons 50 dp | |
| Type | preview title 32 sp bold; row header 18 sp bold; Compose small 11, medium 13, large 19 sp; card titles 11 sp weight 600; stream text 12 sp (name bold); inputs 42 dp tall (36 small) | `TextAppearance.Title.Large`, `catalog_header_text_size`, `FontSizes` |
| Focus zoom | Leanback zoom factors: small 1.10, medium 1.14, large 1.18 (`lb_focus_zoom_factor_*`); rows use medium with no dimming and row shadows off | `FixedStartListRowPresenter(2, false)`, `setShadowEnabled(false)` |
| Focus sound | every focus change plays the system navigation sound (`AudioManager.playSoundEffect` in `focusSound`) | buttons, tabs, settings rows |

### TV surfaces (apk-tv layouts and code)

- **Side menu** (`views/menu/NavigationMenuKt`): collapsed it is an 85 dp strip of 25 dp icons with no fill; when focus enters it animates to 185 dp, fills with #0C0B11 (10 dp radius on the right corners), fades a 30 percent #0C0B11 scrim over the page, and shows labels (semibold, marquee-scrolled when long) beside the icons; padding 10 dp, icon-label gap 10 dp, logo 35 dp. Tabs: Home, Discover, Library, Addons, Settings (filled icon when current, outline otherwise) and the profile with "Switch Profile" at the bottom. The focused tab takes the light #DCDCDC fill with dark icon and label; the current tab is purple; the rest 90 percent white. Page content starts 92 dp from the left edge.
- **Home, Discover and Library share one layout** (`fragment_board`, `fragment_discover`, `fragment_library`): a preview area 330 dp tall (61 percent of the screen) on top and catalog rows below that start 100 dp up inside it. Discover and Library add a filter picker between the two.
- **Preview that follows focus** (`fragment_meta_preview`, `MetaPreviewFragment`): the focused title's background fills the right 70 percent (`centerCrop`) and fades into the window color to the left and to the bottom (`background_gradient`, two linear gradients with the center at 0.7); the text column is the left 50 percent with 30 dp padding: name 32 sp bold on one line; 15 dp below, runtime, year, the IMDb number with the IMDb mark and the genres at 11 sp, 20 dp apart; the description at 13 sp, four lines; the cast at 11 sp in 50 percent white, one line. Moving focus to any card swaps the preview (`onItemSelected`, `PreviewMetaItem`), and each row loads only when focus reaches it (`LoadRow`).
- **Rows and cards** (`CatalogRowsController`, `card_catalog_item`, `CustomHorizontalGridView`): the focused card scales to 1.14 and stays pinned 30 dp from the row start while the row slides under it (window alignment at the low edge, offset 30 dp); focus never climbs into the preview, since the page only offers the rows as focus targets (`FixedFocusLinearLayout` limits `addFocusables` to them). Row padding 30 dp; cards 18 dp apart (8 dp grid spacing plus a 10 dp card margin); header 18 sp bold, 8 dp above the cards. Card: radius 10 dp on #18181C; at least 125 dp on each side, so a 1.464 poster is about 125x183 dp, a landscape still 222x125 dp, a square 125 dp. The title sits under the card at 11 sp weight 600, one line, centered, 50 percent white that turns 90 percent with focus; a setting ("Show title under catalog items") hides it. Badges: "IN CINEMA" at the top right (24 dp tall, 10 sp bold uppercase on the window color, radius 5 dp), a watched check in an 18 dp purple pill at the top left, a new-episodes tag at the top right; progress is a 4 dp bar inset 5 dp at the bottom, 90 percent white on a 20 percent track. A long press opens a context menu.
- **Title page** (`fragment_meta_details`, `views/metadetails`): the background fills the screen under a window-color layer at 85 percent. At the top left (30 dp in, 30 dp down, a 700 dp column): the logo in a 333x80 dp box, the title, a line for the focused episode, the description at 50 percent. Then stacked rows, 15 dp below: season chips, addon chips, the episode row, the stream row. At the bottom: pill buttons Trailer, Library, Watched, Notifications, Like, Love (10 dp apart) and a status label at the right.
- **Focus selects, nothing opens**: moving focus onto a season chip selects that season and scrolls the episode row to it (`SelectSeason`); moving onto an addon chip shows only that addon's streams in the row below (`SelectAddon`, one addon at a time, no "All"); focusing an episode updates the episode line (`SelectVideo`); pressing it loads its streams into the stream row (`LoadStreams`). Chips: radius 5 dp, padding 10 x 5 dp, bold; the selected one has a 4 percent white fill and 90 percent text, the others 50 percent; they zoom 1.14 and keep focus centered.
- **Episode row**: all seasons in one continuous row; a 2 dp, 125 dp tall hairline (white 10 percent) with 15 dp margins marks each season boundary (`video_season_separator_item`). Episode cards are 16:9 at 125 dp tall, radius 10 dp, with the episode number tag (and a check once watched) at the top right, an hourglass at 45 percent for unreleased episodes, and the 4 dp progress bar; zoom 1.14, focus centered.
- **Stream row** (`StreamsController`, `card_stream_item*`): one horizontal row of cards, zoom 1.10, focus centered. A "Theme" setting picks the card: Default, a 360x125 dp card with a 70 dp name column (12 sp bold), the description in a 220 dp column up to eight lines (12 sp) and a 48 dp play circle that turns green #30D87D on focus; Expanded, small 230x63 dp cards with the name and one line of description at 50 percent, plus one wide 848x80 dp card under the row with the focused stream's full description (five lines); Marquee, 360x80 dp cards whose four description lines scroll sideways. A "Filter streams" setting keeps per-device presets with include and exclude tags.
- **Addons** (`fragment_addons`, `card_addon_item`, `fragment_addon_details`): an "Add addon" green pill and a filter picker over a one-column grid (zoom 1.10); cards 760 dp wide, padding 20 dp, an 80 dp logo in a 10 percent grey box, the name bold with the version at 12 sp and 50 percent and an "Installed" tag, then the description at 12 sp; the focused card turns 50 percent light grey. The details page lists logo, name, version, description, URL lines, an italic disclaimer and Configure, Upgrade, Install, Uninstall buttons.
- **Player** (`views/player`, Compose): 27 dp control icons with 5 dp padding that take the light focus fill; progress in purple; an optional clock at the top in 80 percent white ("Show the time on the player"); controls hide after 3.5 s (`playbackControlsAutoHideTickleTimeout` 3500); a next-episode popup with its own duration setting; menus for audio, subtitles, speed, video, chapters and statistics.
- **Profiles**: a profile picker with avatar circles and a PIN pad dialog (`views/userprofiles`), reachable from the side menu.

### Phone app (apk-phone, `com.stremio.android.ui`)

- Colors (`theme/Colors`): page #0C0B11; surfaces a 12 percent lavender white (#1FE8DEF8) and #1D1B20; text #F9F9FB, secondary #CAC4D0 (11.5:1), tertiary #D9D6FE, quaternary #9C8CF9; inactive tab icons #8F8B93 (5.9:1); purple #7B5BF5, blue #1245A6 (selected chip), green #22B365, yellow #F6C700, error #CF6678, IMDb #F5C518. These are Material 3 dark roles, not the web's opacity steps.
- Type and sizes (`theme/FontSizes`, `ThemeKt`, `Sizes`): 10, 12, 14, 16, 18 sp; app bar 60 dp; a collapsing banner bar from 60 to 250 dp; portrait bottom bar 70 dp with a 50 dp fade above it; landscape rail 80 dp with the logo (30 dp) and Search added as the first tab; buttons large 65 dp (32 dp icon, 16 sp bold), medium 60 dp (24 dp, 14 sp bold), small 50 dp (20 dp, 12 sp bold).
- Bottom bar (`bars/NavigationBarKt`): Home, Discover, Library, Addons, Settings; 25 dp icons, filled and purple when current, outlined #8F8B93 otherwise; labels 12 sp semibold 6 dp under the icon; 20 dp between tabs.
- Posters (`MetaPreviewItemKt`, `CatalogKt`): 125 dp wide (square 125, landscape 222), 15 dp apart; radius 12 dp on the 12 percent surface; a 3 dp white border fades in on keyboard or D-pad focus.
- Streams (`screens/metadetails/StreamListKt`): the addon filter is a horizontal chip row instead of a dropdown (pills, 20 x 10 dp padding; selected filled #1245A6 with bold white, others transparent with medium 50 percent white) whose ends fade out over 10 percent of the width (`chipsGradientMask`); rows are 12 dp radius cards on the 12 percent surface with a 25 dp round stream icon, the name on one line (line breaks turned into spaces) in semibold and the description in #CAC4D0; a long press offers "Download this video".
- Episodes: season chips over the list, and "Mark rest as watched" in the episode menu.
- "Organize content": a drag list to reorder and hide the home catalogs (`ContentSettingsMenuKt`, reorderable list); a Downloads screen.

## What to borrow for Marquee

Each item names what to take, and how it lands in the v3 language (white focus with scale and lift, glass panels, 10 percent hairlines, the white primary pill, Apple's blue only on sheet primaries).

1. **Title page with a stream panel beside the info.** The info column at the left and a full-height panel at the right that holds the episode list or the stream list and switches in place (episodes, then a chosen episode's streams, with a back chevron and "S1E1 Pilot"), so choosing a source never leaves the title. Why: Apple has no sources at all, and this is the one layout that makes Stremio's core job one screen deep. In v3 the panel is the strong glass with a hairline edge, not 40 percent black.
2. **Streams in addon order with an "All / per addon" filter.** One flat list, a filter pill at the top only when two or more addons answered, choosing one scrolls to the top; the source name in a fixed 7 rem column and the addon's description verbatim beside it. Why: it keeps the addon's own words (the core's captions shown as is, per the DECISION rule) while staying scannable. In v3: the filter is a pill menu like our seasons picker; the play affordance is the white focus and lift, not a green circle.
3. **Last-used source marked with progress.** The stream played last carries a thin progress line under its name, so resume picks the same source. Why: cheap and directly useful. In v3 the line is white, as Apple's progress fill.
4. **The info column order.** Logo (fallback: the name in display type), then one meta row (runtime, year or year range, ratings), then labeled groups, then the summary, then one action row at the bottom. Why: it reads top down with no table and fits the TV and web title pages. In v3 the meta row carries IMDb and Rotten Tomatoes side by side where Stremio has IMDb alone, and each rating stays a link to its source as Stremio's IMDb link does.
5. **Genres, cast and directors as linked chips.** Each genre opens Discover filtered by it, each person opens Search for them. Why: navigation Apple puts in a separate Cast and Crew shelf comes for free. In v3: glass pills with the hairline, label weight 500, group label in primary text (not 30 percent).
6. **Paired library and watched control.** Add to Library and Mark as watched share one pill with two icon cells and a tooltip each. Why: two related toggles cost one slot in the action row. In v3 it sits beside the white primary pill as a glass capsule with visible labels on TV.
7. **Episode panel tools.** Prev and Next season pills around a season menu, an episode search field, and a dated row ("1. Pilot", "Jan 21, 2008") with Watched and Upcoming flags and a progress line in the still; arriving from elsewhere scrolls to the episode. Why: long shows need search and stepping, which Apple's episode shelf lacks. In v3 the flags are monochrome chips (white text on glass), not yellow or green.
8. **Discover's select-then-open preview on wide screens.** A filter row of pills (type, catalog, genre), a grid, and a persistent 29 rem preview panel that one click fills and a second click (or Show) opens. Why: on desktop and TV it lets you browse without page hops. In v3 the panel is glass, the selected card gets the white focus treatment, and the panel hides below about 1000 px as Stremio's does.
9. **Addon catalog row and details sheet.** A source switch (Installed, Official, Community), a type filter, search, and full-width rows: logo square, name and version on one baseline, types at a lower level, description, and the action column (Install, or Uninstall as a quiet outline, a gear when configuration is needed, Share). The details sheet lists name, version, description, the manifest URL as selectable text, supported types, a watch-activity warning when it applies, and a disclaimer. Why: this is the trust surface for third-party addons and Apple has nothing to copy. In v3: Install is the white pill on the row; in the sheet, the primary follows Apple's sheet rule (key blue), Cancel is plain text.
10. **Search that also takes links.** One field for titles, people and pasted HTTP, magnet, IMDb or TVDB links, with an empty state that says so in four plain hints. Why: it removes a separate "open link" flow. In v3 the hints are text in tertiary white, no icon grid needed.
11. **Settings as one scrolling page with a section menu.** A left menu (General, Interface, Player, Streaming, Shortcuts) that jumps to and tracks sections of one long page, label left and control right, version info at the bottom of the menu, player options grouped into Subtitles, Audio, Controls, Auto Play, Advanced. Why: it matches macOS System Settings closely and keeps every option findable. In v3 the rows sit in grouped glass lists with hairlines, Apple style, and switches use Apple's green per the DECISION block.
12. **Three-column subtitle menu.** Languages, then that language's variants, then delay, size and offset, in one player panel. Why: picking among many addon subtitles for one language is a Stremio-specific need Apple's menu does not solve. In v3 it is a glass panel with white selection, no green dot.

From the Android TV and phone apps, where they beat the web:

13. **TV home preview that follows focus.** The top 61 percent of the screen previews whichever card has focus (art in the right 70 percent fading into the page, name, one meta line with ratings, four lines of description, cast), and the rows sit under it, overlapping it by 100 dp. Why: it is exactly the Top Shelf idea of the v3 pick, driven by focus instead of a carousel, and it gives every poster row a hero for free. In v3 the meta line carries IMDb and Rotten Tomatoes side by side and the fade uses Apple's eased scrims.
14. **Focus that stays put.** The focused card stays pinned at the row start (30 dp in) while the row slides under it, and focus never leaves the rows for the preview above. Why: the eye never hunts for focus, and the preview reads as a display, not a control. In v3 the focused card takes the white focus with lift and shadow; Stremio's 1.14 (Leanback medium) is a measured reference point where Apple publishes no scale number.
15. **TV title page where focus selects.** One screen of stacked rows (season chips, addon chips, episodes, streams): resting on a season jumps the episode row, resting on an addon shows its streams, pressing an episode fills the stream row, all without a dropdown or a new page; the episodes of every season form one row with a thin hairline at each season boundary. Why: on a remote this is far quicker than the web's panel with menus, and it keeps the addon grouping visible as a row of names. In v3 the chips are glass pills with white selection and the hairline is the 10 percent one.
16. **Compact stream cards with a detail strip on TV.** Small cards (name and one line) in a row plus one wide strip under them that shows the full description of the focused stream (Stremio's "Expanded" theme). Why: addon descriptions are long (release names, sizes, codecs), and the strip shows all of it for one stream while the row stays scannable. In v3 this is the one TV layout, not a theme setting.
17. **Side menu that opens on focus, and a light focus fill.** A collapsed 85 dp icon strip that widens to 185 dp with labels and dims the page 30 percent when focus enters; focused buttons, tabs and player controls turn a light fill with dark content (14.3:1). Why: it is the tvOS sidebar behaviour and the same white-focus rule as v3. In v3 the strip is glass, the fill is white, and labels are always present on TV once it opens.
18. **Phone: addon chips with faded ends, and organizing home.** On the phone the stream filter is a horizontal chip row whose ends fade out, and home catalogs can be reordered and hidden in a drag list. Why: chips are one tap where a dropdown is two, and catalog order is the user's main control over a home made of addon rows. In v3 the selected chip is white with dark text, not blue.

From the player:

19. **Loading screen: the title's logo filling with real progress.** While a stream loads, the title's logo sits large in the center over its backdrop at 60 percent: a faint copy at 20 percent, and a full copy revealed from the left by `clip-path: inset(0 {100 - progress}% 0 0)` as real progress comes in, both breathing between 40 and 100 percent over 2 s. Why: the owner likes it, it tells the viewer something is happening with the title they chose instead of a bare spinner, and the artwork stays the only color, as v3 wants. In v3: feed it from our engine's own progress (peers, bytes toward the start buffer, speed), ease the clip to each new value over the update interval instead of 5 s steps, keep the core's status line ("Starting 1080p, 606 MB from Torrentio") as a small caption under the logo in tertiary white (Stremio shows none), fall back to the title in display type when there is no logo, and under reduced motion drop the pulse and keep the fill.

From the subtitles menu:

20. **Languages first, with search.** The first level lists only languages, each named in its own language (Stremio's `langs` endonyms on the web, `Locale.getDisplayName` on Android), with Off on top and the user's languages pulled to the top: Android TV's order is the one to copy (Original, the preferred language, the secondary one, then the rest alphabetically by name), not the web's sort by ISO code. Choosing a language turns subtitles on at once with its best variant. Why: a title often has 15 or more languages and over 60 files, and a flat list of files is what the owner dislikes in ours. Stremio has no search; we add a search field at the top of the language list that matches the native name, the English name and the code, which is what makes 40-language lists usable on a phone. In v3 the rows are a grouped glass list with a white checkmark, and each language may show its variant count as a quiet caption.
21. **A variants page per language.** The second level shows the chosen language's files: origin first (from the file, from the stream, then each addon in addon order), each row with a real label and its source under it. On a phone it is a pushed page with a back button, because Stremio's three side-by-side columns get clipped at 390 px; on TV and desktop it can sit beside the languages, as on Android TV. Why: switching to another English file is a deliberate second step, never in the way of the first. Stremio labels every OpenSubtitles row with the language name alone, which hides the only useful difference; we show the release name the addon already returns (`movieReleaseName` or `subtitleFileName`), plus the format badge (ASS) and "Embedded" where they apply.
22. **Auto-pick the variant that matches the video.** Stremio picks, in order: the track saved for this stream, the language chosen earlier in the session, the preferred language, then (on Android) the secondary language; within a language, tracks inside the file come before addon files, and the hash and release matching is left to the addon through `videoHash`, `videoSize` and `filename`. We keep that order and do the matching in our own ranking: a hash match first, then the closest release name to the stream's file name (group, source, resolution), then addon order. The picked variant carries a quiet "Matches this video" caption, since Stremio gives the user no hint why a file was chosen. In v3 the caption is tertiary white, never a color.

## What not to borrow

| Stremio does | Why not for v3 |
|---|---|
| Three accents at once: purple #7B5BF5 for selection and progress, green #22B365 for primary actions and play, yellow and red flags, a blue active chip | v3 is monochrome with one key color used where Apple uses it; DECISION rules out mixing colors and rules out red; white text on the green is 2.5:1 |
| A purple-indigo page gradient (#0C0B11 to #1A173E) and a purple glow around every dialog, menu and tooltip | Apple's stage is neutral black; colored glows read as decoration and clash with artwork |
| Hover and focus as a 2 to 3 px outline or ring with no movement, and buttons that lose their fill on hover | v3 focus is white with scale, lift and shadow (tvOS); an emptied button reads as disabled |
| A left icon rail whose labels stay invisible until hover, icons at 35 percent | Apple uses a labeled top navigation on the web and a labeled tab bar on phones; hidden labels fail on TV and for new users |
| Uppercase group labels and secondary text at 30 to 44 percent white | measured 2.3:1 to 3.7:1, under the 4.5:1 text floor in DECISION |
| The whole title page washed out: art at 30 percent opacity behind all content | Apple shows the art at full strength in a hero and protects text with eased gradients; dimmed art loses the "artwork is the only color" effect |
| Posters dimmed to 90 percent and titles centered under them | Apple shows art at full brightness and left-aligns lockup text |
| A floating yellow warning banner across the bottom of Home | a full-width colored fill breaks the monochrome; v3 states sit in neutral glass |
| No home hero, Home as equal catalog rows only | v3 keeps Apple's hero and shelves; Stremio's rows are a fallback for catalogs without art |
| Plus Jakarta Sans, Stremio's icon set, logo, mascot avatar and illustrations | brand rule: we take the feel, never their assets |
| Like and Love personal ratings | not part of the v3 ask; ratings in v3 are IMDb and Rotten Tomatoes side by side |
| Addon text shown with its emoji and flags (it ships a flag emoji font for that) | conflicts with the no-emoji rule; Marquee needs its own decision on addon-provided emoji (strip, or show as is under the core-caption rule), recorded in DECISION rather than copied |
| Android TV: a green play circle on the focused stream (#30D87D, white on it 1.9:1), green Install buttons and a purple current tab | the same color mixing as the web; v3 focus is white |
| Android TV: focus as a flat Leanback zoom with shadows turned off and no lift | v3 focus adds lift, shadow and the specular highlight on top of the scale |
| Android TV: a system sound on every focus move | the platform owns navigation sounds; a custom one adds noise (and the owner's walks stay silent) |
| Android TV: three stream card layouts behind a "Theme" setting | pick the one good TV layout (item 16) instead of a setting, per the minimal rule |
| Android phone: Material 3 lavender-tinted surfaces and text (#1FE8DEF8, #CAC4D0, #D9D6FE) and a blue selected chip | v3 surfaces are neutral glass and white text at Apple's opacity levels |
| Player loading: fill updates every 5 s with a 0.1 s transition, a scale pulse with no reduced-motion rule, and Stremio's own symbol as the no-logo fallback | jumps read as stalls; v3 honours reduced motion; the brand rule keeps their symbol out (our fallback is the title in display type) |
| Subtitles: every OpenSubtitles variant labeled with the language name alone, the real difference only in a hover title | a list of identical rows; show the release name the addon returns |
| Subtitles: languages sorted by ISO code, so Español follows Slovenčina | sort by the name the user reads |
| Subtitles: three fixed columns in one 25 rem panel, clipped on a phone, and green dots or purple rows for the selection | pages on the phone, a white checkmark for the selection |

## Screenshot files (private, `stremio-ref/` in the session scratchpad)

A full capture set of every measured web page (desktop 1440 at 2x and phone 390 at 3x, each page stepped top to bottom, 60 files with an `index.md`) is in `ref/stremio-web/` at the repo root, which is gitignored. The scratchpad files below are the measuring shots.

Full path: `<scratch>`. Suffix `-1920`, `-1440` or `-phone` names the viewport.

| File | Shows |
|---|---|
| `board-1920.png`, `board-1440.png`, `board-phone.png` | Home rows, top bar, rail or bottom bar |
| `board-with-warning-1920.png`, `board-with-warning-1440.png` | the streaming server warning card |
| `board-hover-1920.png`, `board-hover-1440.png` | poster hover ring and zoom |
| `nav-tab-hover-1920.png`, `nav-tab-hover-1440.png` | rail tab hover with its label |
| `nav-menu-1920.png` | avatar menu |
| `discover-1920.png`, `discover-1440.png`, `discover-phone.png`, `discover-menu-1920.png` | grid, preview panel, catalog dropdown |
| `detail-movie-1920.png`, `detail-movie-1440.png`, `detail-movie-phone.png`, `detail-movie-streams-phone.png` | movie title page and its stream list |
| `detail-movie-stream-hover-1920.png`, `detail-movie-actions-hover-1920.png` | stream row hover, action hover with tooltip |
| `detail-pd-tt0063350-1920.png`, `detail-pd-tt0063350-addon-menu-1920.png` | two addons answering: the "All" filter and its menu |
| `detail-pd-tt0017136-1920.png`, `detail-pd-tt0032138-1920.png` | one-addon list; the empty state |
| `detail-series-1920.png`, `detail-series-phone.png`, `detail-series-episodes-phone.png` | series title page and the episode panel |
| `detail-series-season-menu-1920.png`, `detail-series-episode-hover-1920.png` | season menu, episode hover |
| `detail-episode-1920.png` | one episode's streams with the back chevron |
| `library-1920.png`, `calendar-1920.png` | signed-out placeholders |
| `addons-1920.png`, `addons-community-1920.png`, `addons-phone.png` | installed addons (all types; movie type) |
| `addons-catalog-menu-1920.png`, `addons-remote-1920.png`, `addon-details-modal-1920.png` | source menu, community catalog, details dialog |
| `search-1920.png`, `search-focus-1920.png`, `search-results-1920.png` | empty search, focused field, results for "dune" |
| `settings-1920.png`, `settings-toggles-1920.png`, `settings-phone.png` | settings menu, sections, toggles |
| `player-trailer-1920.png`, `player-trailer-hover-1920.png` | player chrome over a paused, muted trailer |

The measuring scripts and raw JSON are in the scratchpad at `stremio-study/` (`pass1.ts` to `pass4.ts`, `phone.ts`, `data/*.json`).
