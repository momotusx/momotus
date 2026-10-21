# Reference study: Apple TV (2026-10-21)

The owner asked for every v3 page, Settings included, to read as Apple level (see the top block of `design/DECISION.md`). This file records Apple's TV design as measured: tv.apple.com computed styles and screenshots (headless Google Chrome 155, dark color scheme, 1440x900 and 1920x1080 viewports at 2x), Apple's Human Interface Guidelines for tvOS and iOS, and Apple's developer documentation. We take the feel, never Apple's logos, artwork, fonts or branding. Screenshots are private reference in the session scratchpad (`apple-ref/`), never committed or shipped.

## Summary

1. Apple's TV design is a black or near-black stage (#000 hero and header and #1F1F1F page on the web; a #000 page with #1C1C1E groups on iPhone) where the artwork is the only color; chrome is white text at fixed opacities (92, 70, 64, 40 percent) and hairlines at 10 percent white.
2. One type family (SF Pro) carries everything; hierarchy comes from size and weight, never color: shelf titles 17 pt bold on the web, tvOS Title 1 to Caption 2 from 76 down to 23 pt, body 29 pt on TV and 17 pt on iPhone.
3. Content sits on a strict grid: tvOS 80 pt side and 60 pt top and bottom safe area, 40 pt gutters, fixed column widths (8 posters per row on the web, a HIG grid table that runs from 2 to 9 columns on tvOS), and partially visible next items to signal scrolling.
4. Every card is the same shape language: 14 px corners on the web, a 0.5 px inner white keyline at 25 percent, eased legibility gradients under any text on art, and full-bleed 16:9 heroes with layered scrims.
5. On TV, focus is the interface: the focused item scales up, rises with a shadow, sways with parallax and catches a specular highlight; color alone never marks focus, and the one key color (Apple's blue) appears only on sheet primaries, sign-in and links; the hero primary pill is white.

## Sources and how to read the numbers

| Tag | Source |
|---|---|
| HIG layout | https://developer.apple.com/design/human-interface-guidelines/layout |
| HIG type | https://developer.apple.com/design/human-interface-guidelines/typography |
| HIG color | https://developer.apple.com/design/human-interface-guidelines/color |
| HIG materials | https://developer.apple.com/design/human-interface-guidelines/materials |
| HIG focus | https://developer.apple.com/design/human-interface-guidelines/focus-and-selection |
| HIG images | https://developer.apple.com/design/human-interface-guidelines/images (parallax, layered images) |
| HIG tab bars | https://developer.apple.com/design/human-interface-guidelines/tab-bars |
| HIG lists | https://developer.apple.com/design/human-interface-guidelines/lists-and-tables |
| HIG lockups | https://developer.apple.com/design/human-interface-guidelines/lockups |
| HIG video | https://developer.apple.com/design/human-interface-guidelines/playing-video |
| HIG accounts | https://developer.apple.com/design/human-interface-guidelines/managing-accounts |
| UIKit focus | https://developer.apple.com/documentation/uikit/about-focus-interactions-for-apple-tv |
| web | tv.apple.com computed style, read with `getComputedStyle` in the live page (storefront TR in English, signed out; `/us` redirects by location). Pixel values are CSS px at a 1440 px wide viewport unless a width is named |
| web vars | tv.apple.com resolved CSS custom properties on `:root` in dark mode (581 variables) |
| web r1 | an earlier pass on 2026-10-19: headless Chromium (Playwright 1.63.0), dark scheme, storefront "Türkiye (English)", signed out, 1440x900 and 1920x1080 at DPR 1; computed styles of the live DOM and the downloaded stylesheet; focus by keyboard modality plus `focus()`, so `:focus-visible` matched |
| shot | a file in `apple-ref/` (listed at the end) |
| iPhone | the owner's iPhone 13 Pro (iOS 27.0.1), XCUITest screenshots and accessibility frames, 2026-10-21; method in "iPhone TV app and iOS Settings" below |

HIG numbers are points (tvOS canvas 1920x1080 pt). Web numbers are CSS px; the web app is the desktop form of the same design, so its ratios (radius to card size, text opacity levels, gradient stops) carry over, its absolute sizes do not.

## Color

### Web dark tokens (web vars)

| Role | Token | Value |
|---|---|---|
| Page | `--pageBG` | #1F1F1F |
| Hero, header, product header | (computed) | #000000 |
| Lower page band (About, Information) | `.section.second` | #2B2B2B (`--opaqueShelfBG`) |
| Footer | (computed) | #323232 |
| Card fill (search result, About card) | `--cardBackgroundColor` | #FFFFFF1A (10 percent white) |
| Shelf fill | `--shelfBG` | #FFFFFF0D (5 percent white) |
| Primary text | `--systemPrimary` | #FFFFFFEB (92 percent white) |
| Secondary text | `--systemSecondary` | #FFFFFFA3 (64 percent) |
| Caption and synopsis text | (computed) | rgba(255,255,255,.70) |
| Tertiary text | `--systemTertiary` | #FFFFFF66 (40 percent) |
| Quaternary fill | `--systemQuaternary` | #FFFFFF1A (10 percent) |
| Quinary fill | `--systemQuinary` | #FFFFFF0D (5 percent) |
| Divider | `--labelDivider` | #FFFFFF1A; keyline `--keyline-border-style: .5px solid #ffffff1a` |
| Vibrant divider | `--vibrantDivider` | #EBEBF530 |
| Glass | `--glassMaterialBackground` | #26262899 (dark grey at 60 percent, used with blur) |
| Glass fallback | `--fallbackMaterialBG` | #1F1F1FF7 |
| Header over scrolled content | `--navigationScrollBackgroundColor` | #00000066 |
| Modal sheet | `--appModalBackgroundColor` | #2D2D2D; sign-in sheet computed #1D1D1F |
| Modal close button | `--modalCloseButtonBGColor` | #333336 (rollover #37373A, pressed #2F2F32) |
| Modal scrim | `--modalScrimColor`; `dialog::backdrop` computed (web r1) | #0000008C, rgba(0,0,0,.55); the `dialog` itself #1F1F1F, radius 10, no shadow |
| Award and "New" capsule | `div.badging-capsule` computed (web r1) | fill rgba(0,0,0,.8) inside a `linear-gradient(rgba(255,255,255,.5), rgba(255,255,255,.18))` rim, glow `0 0 12px rgba(255,255,255,.16)`, radius 8.5 for the rim and 7 for the fill; label 12/12, weight 600, primary text |
| Sidebar active item | `--sidebarActiveItemBackgroundColor` | #EBEBF51A |
| Segmented control | `--segmentedControlBG` / selected | #7676803D / #636366 |
| Progress fill | `--progressBarFillColor` | #FFFFFF (white, not the key color) |
| Key color | `--keyColorBG`, `--systemBlue` | #0A84FF (Sign In, links, MORE, the season chevron) |
| Web teal, mint, cyan (dark) | `--systemTeal`, `--systemMint`, `--systemCyan` | #6AC4DC, #63E6E2, #5AC8F5 |
| Greys (dark) | `--systemGray` to `--systemGray6` | #98989D, #636366, #48484A, #3A3A3C, #2C2C2E, #1C1C1E |
| Artwork placeholder | inline per image | the artwork's own dominant color (for example rgb(44,28,18)) until the image loads |

### HIG system colors, dark (HIG color, swatch values)

| Color | Default dark | Increased contrast dark |
|---|---|---|
| Mint | #00DAC3 (0,218,195) | #54DFCB |
| Teal | #00D2E0 (0,210,224) | #3BDDEC |
| Cyan | #3CD3FE (60,211,254) | #6DD9FF |
| Blue | #0091FF (0,145,255) | #5CB8FF |
| Green | #30D158 | #4AD968 |
| systemGray to systemGray6 | #8E8E93, #636366, #48484A, #3A3A3C, #2C2C2E, #1C1C1E | #AEAEB2, #7C7C80, #545456, #444446, #363638, #242426 |

HIG rules that bind us: tvOS "Avoid using only color to indicate focus. Subtle scaling and responsive animation are the primary ways to denote interactivity" (HIG color, tvOS). Liquid Glass "has no inherent color"; apply color to the background of a prominent control, not to its symbol or text, and "refrain from adding color to the background of multiple controls" (HIG color). In monochrome apps a brand accent color works as the app accent (HIG color).

## Typography

### tvOS text styles (HIG type)

| Style | Weight | Size / leading (pt) | Emphasized | Tracking (pt, HIG tvOS table) |
|---|---|---|---|---|
| Title 1 | Medium | 76 / 96 | Bold | +0.07 |
| Title 2 | Medium | 57 / 66 | Bold | about +0.29 (56: +0.30, 58: +0.28) |
| Title 3 | Medium | 48 / 56 | Bold | +0.35 |
| Headline | Medium | 38 / 46 | Bold | +0.37 |
| Subtitle 1 | Regular | 38 / 46 | Medium | +0.37 |
| Callout | Medium | 31 / 38 | Bold | +0.39 |
| Body | Medium | 29 / 36 | Bold | +0.40 |
| Caption 1 | Medium | 25 / 32 | Bold | +0.15 |
| Caption 2 | Medium | 23 / 30 | Bold | -0.10 |

Default size 29 pt, minimum 23 pt on tvOS; 17 and 11 pt on iOS (HIG type). Avoid Ultralight, Thin and Light (HIG type). SF Pro is the tvOS system font, New York is allowed (HIG type). Note the tvOS default weight is Medium, not Regular: TV text is one step heavier than phone text at the same role.

### iOS text styles, Large (default) (HIG type)

| Style | Weight | Size / leading (pt) | Emphasized | Tracking (pt) |
|---|---|---|---|---|
| Large Title | Regular | 34 / 41 | Bold | +0.40 |
| Title 1 | Regular | 28 / 34 | Bold | +0.38 |
| Title 2 | Regular | 22 / 28 | Bold | -0.26 |
| Title 3 | Regular | 20 / 25 | Semibold | -0.45 |
| Headline | Semibold | 17 / 22 | Semibold | -0.43 |
| Body | Regular | 17 / 22 | Semibold | -0.43 |
| Callout | Regular | 16 / 21 | Semibold | -0.31 |
| Subhead | Regular | 15 / 20 | Semibold | -0.23 |
| Footnote | Regular | 13 / 18 | Semibold | -0.08 |
| Caption 1 | Regular | 12 / 16 | Semibold | 0 |
| Caption 2 | Regular | 11 / 13 | Semibold | +0.06 |

### Web type in use (web)

Font stack `-apple-system, BlinkMacSystemFont, SF Pro, Helvetica Neue, Helvetica, Arial` (web vars `--font-family`); letter spacing is `normal` everywhere (SF's own optical tracking), except body copy at -0.006 px.

| Role | Size / weight / line height | Color |
|---|---|---|
| Top navigation tab | 16 / 600 / 20.7 | active 92 percent, others 64 percent white |
| Hero (carousel) title | 26 / 700 / 32 | 92 percent |
| Product page title (hidden behind the title logo) | 52 / 700 / 64 | 92 percent |
| Shelf title | 17 / 700 / 22, with a 16 px chevron 4 px after it | 92 percent |
| Shelf subtitle | 15 / 400 / 18 | 92 percent |
| Metadata line on a hero ("TV Show · Thriller · Drama") | 15 / 600 / 20 | 92 percent |
| Synopsis on a hero | 15 / 400 / 20.8, max width 410 px, 2 lines, inline MORE | 70 percent |
| Secondary meta ("2025 · 2h 35m") | 13 / 600 / 16 | 64 percent; format badges at opacity .8 |
| Lockup caption (genre under or on a poster) | 13 / 400 / 16 | 70 percent |
| Episode tag ("EPISODE 1") | 10 / 500 / 13, uppercase | 70 percent |
| Episode title | 15 / 550 / 18 | 100 percent |
| Episode synopsis | 13 / 500 / 16, 2 lines | 70 percent |
| Duration ("54 min") | 12 / 600 / 15 | 70 percent |
| Cast name / role | 12 / 400 / 15 | 92 / 64 percent |
| Info table term / value | 12 / 400 / 16 | 92 / 64 percent |
| Top 10 ordinal | 40 / 700 / 49 | 100 percent |
| Explainability line ("#1 Movie on Apple TV", "Another Season Is Coming"; web r1, `span.explainability-line`) | 13 / 600 / 16, between the synopsis and the buttons: synopsis, 9 px, this line, 16 px, the 40 px button row | 92 percent |
| MORE | 11 / 600 / 14 | on a hero: a pill, padding 0 6, radius 8, white on 64 percent white; in body text (About): key color text |
| Footer links | 11 / 400 | 92 percent; copyright 64 percent |

The web token scale (web vars, weight size/line height): `--large-title` 400 26/1.23, `--title-1` 400 22/1.18, `--title-2` 400 17/1.29, `--title-3` 400 15/1.33, `--body` 400 13/1.23, `--callout` 400 12/1.25, `--caption` 400 12/1.33, `--footnote` 400 10/1.3, `--header` 400 34/1.18, `--headline` 600 48/1.08, `--eyebrow` 600 24/1.17, `--label` 600 21/1.19. Each comes in regular, medium (500), emphasized and semibold (600), bold (700) and heavy (800), with short and tall line heights.

## Layout and grid

### tvOS (HIG layout)

- Safe area: content inset 60 pt from top and bottom, 80 pt from the sides.
- Grid, unfocused item width with 40 pt horizontal spacing and at least 100 pt vertical spacing: 2 columns 860, 3: 560, 4: 410, 5: 320, 6: 260, 7: 217, 8: 184, 9: 160 pt.
- Titled rows need extra space: from the bottom of the previous unfocused row to the center of the title, and from the title to the top of the row's items.
- Partially hidden offscreen items are kept the same width on both sides of the screen.
- Leave room for the focused item's larger size so it never overlaps neighbours or text.

### Web (web)

| Piece | 1440 viewport | 1920 viewport |
|---|---|---|
| Side margin | 40 px | 160 px (content capped at 1600 px) |
| Header | 52 px, black, sticky, padding 0 40 | same |
| Hero and product header | 1440x810 (16:9, full bleed) | full bleed |
| Poster row | 8 per row, `calc(12.5% - 17.5px)`, gap 20 px: posters 153x228 (about 2:3) | 183x273 |
| Episode and trailer row | 5 per row, `calc(20% - 16px)`: 256x197 episode cards, 256x167 trailer cards | scales the same way |
| Cast row | 10 per row, `calc(10% - 18px)`: 118 px circles | |
| Search results | 3 per row: 440x100 cards, gap 20 | |
| How to Watch | 4 per row, 355x120 cards, radius 12, 5 percent white fill | |
| Shelf rhythm | title 22 tall, 13 px to the items (14 px under a subtitle line), 44 px from the bottom of one row to the next title | |
| Scroll inset | each row list has padding 15 px and margin -15 px, so cards can grow on hover without clipping | |

## Surfaces

### TV home (Watch Now)

- web: a full-bleed 16:9 carousel at the top (1440x810), its title (26/700) and two lines (15/400 at 70 percent) at the bottom left, a white primary pill, a note under the button (13 at 64 percent), and the page dots centered in a small glass platter: 172x32, radius 20, fill rgba(0,0,0,.3), `backdrop-filter: saturate(1.8) blur(10px)`, 20 px dot targets. Shot: `web-home.png`, `web-home-hdr.png`.
- web: rows follow directly under the hero: "Must-See Hits", "Top 10 TV Shows" (posters with a 40/700 ordinal and a genre caption over the art), "Top 10 Movies", a full-width Spotlight banner (1360x305, radius 20), more poster rows. No row has a background; the page color carries them.
- HIG tab bars (tvOS): the tab bar scrolls offscreen with the content on single-view tabs such as Watch Now, Movies, TV Shows; Menu always returns focus to the tab bar.

### Title page (show and movie)

- web: product header full bleed 16:9 (1440x810) over black. Layers from the bottom: the backdrop (or a muted background video with a 28 px mute control at the top right), then three scrims on the artwork container: `linear-gradient(80deg, rgba(0,0,0,.14), transparent 41%)`, `linear-gradient(0deg, rgba(0,0,0,.45) 0%, .36 5%, .27 9%, .18 16%, .09 22%, .02 29%, 0 36%)`, and a radial vignette `radial-gradient(100vmax at 66.7% 0%, 0 0%, .04 19%, .15 36%, .30 51%, ...)`; finally the page fades into #1F1F1F with `linear-gradient(to top, #1F1F1F, transparent)`. Shots: `web-show.png`, `web-movie.png`.
- web, left column from the top at x 40: an award or schedule capsule (12/600 text, 20 px tall, gradient hairline `linear-gradient(rgba(255,255,255,.5), rgba(255,255,255,.18))` around a black 80 percent fill; see "Award and New capsule" under Color), the title as a logo image (F1: 141x81 px; never set text when a logo exists), the metadata line with a 23 px provider coin, two lines of synopsis with MORE, the secondary meta line with format badges, then the 40 px button row, which ends 75 px above the bottom of the hero.
- web, right column: "Starring" and "Director" lines right aligned at the bottom (13/400, label 64 percent, names 92 percent).
- web, below: Season picker and episodes, Trailers, Related (posters), Cast and Crew (circles), How to Watch, then a lighter band (#2B2B2B, 12 px top padding) holding About (a 440x155 card, 10 percent white, radius 12, padding 14 16 16), and three columns Information, Languages, Accessibility (headings 17/700, terms 12/400). Shots: `web-show-s1.png` to `web-show-s3.png`, `web-movie-s1.png`.

### Buttons (web)

| Button | Measured |
|---|---|
| Primary (Sign Up to Watch, Play) | 40 px tall pill (radius 40), padding 0 30, white at 92 percent on `::before`, label 15/600 black at 88 percent, `transition: scale .1s ease-in` (press shrinks it) |
| Secondary round (Add to Up Next) | 40x40 circle, fill rgba(40,40,40,.8) with `mix-blend-mode: multiply` and `backdrop-filter: blur(60px) saturate(2.4)`, 12 px white glyph |
| More (...) on cards | 27 to 28 px circle, glass platter rgba(40,40,40,.5) with `saturate(1.8) blur(60px)` |
| Sign In (header) | 83x28, radius 6, #0A84FF, 13/600 white |
| Sheet primary (Continue) | 320x44, radius 8, #0071E3, 17/400 white |

### Episodes

- web: a season picker as a title: "Season 1" 17/700 with a 9x5 chevron in the key color, 14 px above the cards. Shot: `web-show-s1.png`.
- web: episode cards 256x197, radius 14, the still fills the whole card and the text sits on it, bottom aligned, 12 px from the left: tag, title, two lines of synopsis, duration, a More button at the bottom right. Under the text a legibility gradient over the bottom 141 px: `linear-gradient(transparent 0, rgba(0,0,0,.13) 16px, .21 27px, .30 40px, .40)`. Shot: `web-show-episode-hover.png`.
- Every artwork carries a keyline: `::after` with four 0.5 px inset white box shadows at opacity .25 and radius 13 inside the 14 px card (web). On search posters the same idea is a 1 px border at 16 percent white.

### Player chrome

iPhone: not captured. The full capture on 2026-10-21 tried to play a free trailer on the iPhone with the media volume at zero, and stopped before playback because silence could not be guaranteed:
- The phone's media volume read 0.4. That is not one of the 16 button steps, so the exact value could not be restored with the volume buttons.
- A volume-down press from the Home Screen did not move the media volume; it went to the ringer, and was undone with one press up.
- Setting the volume through an `MPVolumeView` slider in the test runner did not change the reading, even with the runner in the foreground.

web (measured 2026-10-21, `ref/apple-tv-web/player/`, values from `desktop-dump.json` and `phone-dump.json`). Setup: a free trailer (Slow Horses, "Destabilize: Season 5", 2 min) played in headless Chrome with `--mute-audio` and every media element muted, at 1440x900 and 390x844. Chrome's Widevine CDM has to load (Playwright's default `--disable-component-update` blocks it): without it the player stays on its loading spinner, even though the trailer playlist loads.

What the player shows:
- **Opening:** a full-viewport black `dialog` that slides up (`animation` 0.4 s ease; the host sets `--animation-duration: 500ms`, `--animation-easing: cubic-bezier(0.11, 0.37, 0.36, 1)` and `--animation-delay: 420ms`) with a small centered spinner (`desktop-01-loading.png`).
- **Trailers playlist overlay:** trailers first play inside it. A Close X and "Trailers" (17/700) sit at the top left, the age rating glyph at the top left of the video, a Next Page arrow fills the right edge (30 px wide), "SLOW HORSES" (13/600 at 64 percent) and the trailer title (13/400 at 92 percent) sit at the bottom left, and the pagination dots (20 px targets) at the bottom center. One tap ("Show media controls") reveals the transport (`desktop-02`, `phone-02`).

Transport layout:
- The video letterboxes in black, under a full-height gradient `linear-gradient(transparent, rgba(0,0,0,.6) 44.64%, rgba(0,0,0,.7) 75%, #000)`.
- The scrim is a grid with 20 px top margin and side margins of 40 px, 30 px under 1680, 25 px under 1000 and 20 px under 740 (measured: 30 at 1440, 20 at 390); content is capped at 1680 px.
- **Header row (26 px):** Close X (18x21) at the left. At the right the volume group is 136 px wide: a 100x5 slider, then a 26 px speaker glyph.
- **Body, centered:**

  | Control | Desktop | Phone |
  |---|---|---|
  | Skip back 10 s | 40 px button, 32 px glyph | 32 px button |
  | Play / pause | 66 px button, 50 px glyph | 56 px button |
  | Skip forward 10 s | 40 px button, 32 px glyph | 32 px button |
  | Gap between them | 36 px | 38 px |

  Glyphs are white at 90 percent, with no plates or circles behind them.
- **Footer:**
  - Height: 121 px on desktop, 109 px on phone.
  - Title, left: 26/700 white on desktop, 17/700 on phone, truncated with an ellipsis.
  - Control group, right: 24 px targets, 14 px gap (10 px on small screens), white at 90 percent, in this order:
    1. Subtitles (20 px glyph in a speech bubble).
    2. Audio (a native `select` under a waveform glyph).
    3. Share.
    4. Full screen (24x22).
  - There is a jump-to-live control, marked unsupported here. There is no AirPlay and no Picture in Picture button in Chrome.
- **Scrubber:** the full content width (1380 px at 1440).
  - Track: 5 px tall; the played part #FFFFFF, the rest about #5A5B5C (white at about 35 percent over black, sampled).
  - Thumb: a white 11 px circle.
  - Times sit under the track, elapsed at the left and remaining as "-1:54" at the right: 11/500, tabular numbers, white at 55 percent.
  - A hover preview (230 px wide, radius 12, time 11/600 at 85 percent) exists but showed no thumbnail for this trailer (`desktop-04`).
  - A "LIVE" capsule (#FF5066, radius 18, 10/600) exists for live streams.
- **Under the scrubber:** a "Trailers" tab button (81x32, radius 6, 15/600 white, `backdrop-filter: blur(30px)`) with a 1 px white outline. It reopens the playlist (`desktop-03`, `phone-03`).
- **Subtitles menu:** a dark rounded popover above the button. It has a grey "Subtitles" header, then On, Off (with a check when selected), and a "Language" row with the current language (English) in small grey and a chevron. It is about 183 px wide on desktop and about 267 px on phone (by eye from `desktop-05`, `phone-05`).
- **Paused:** the center glyph turns into Play and the controls stay up (`desktop-06`, `desktop-07`). While playing they fade:
  - footer: opacity over 0.4 s `cubic-bezier(0.4, 0, 0.6, 1)` after 0.05 s, plus `translateY(20px)` over 0.5 s;
  - header: `translateY(-20px)`;
  - body and controls fade over 0.35 to 0.65 s.

What Apple publishes (HIG video): on tvOS the system player shows transport controls (subtitles, audio language, actions such as add to library) and, below them, content tabs (Info, Episodes, Chapters); keep custom actions to a step or two; wait at least 0.5 s before pausing for an interactive overlay; keep logos and noninteractive overlays small and translucent; scrubbing thumbnails are 160 px wide. The web keeps a white progress fill (`--progressBarFillColor` #FFF) and a dark player panel (`--playerBackground` #2D2D2DE0, `--playbackOverlayBackgroundColor` #333333B3) (web vars).

### Search

- web: results page under the same header. "Top Results" as 440x100 cards (10 percent white, radius 14, padding 14 16 14 14) with a 48x72 thumbnail at radius 8, title 15/400 and meta 13/400, then a "TV Shows" poster row. The search field in the header: 172x28, radius 4, 1 px border at 92 percent white, placeholder 15/400 at 64 percent. Shot: `web-search.png`.
- HIG search fields (tvOS): a search screen is a keyboard screen with results beneath it; offer popular, context and recent suggestions because typing with a remote is slow.

### Settings and lists

- HIG lists (tvOS): a focused row highlights and slightly grows, its corners can round; do not mask images near it yourself. The tab bar stays pinned at the top for split views such as Settings and the Library (HIG tab bars).
- HIG materials (tvOS): regular material to separate sections such as a sidebar or a grouped table; thick for a dark element over a regular background; thin and ultra thin for light schemes.
- web: the lower band pattern (a 5 to 10 percent lighter band holding grouped cards at radius 12) is the web's grouped list. Values above.
- iPhone: iOS Settings uses inset grouped cards (358 wide, radius about 26 pt, #1C1C1E on black), 51 pt rows, separators inset to the label, 35 pt between groups; measured values under "iOS Settings" below.

### Account and sign-in

- web sign-in sheet (view only, nothing typed): an 800x560 modal centered over the dimmed page, #1D1D1F, radius 10, shadow `0 10px 10px rgba(0,0,0,.2)`; a 36 px round close button (#333336) at the top left, 24 px in; app icon; heading 32/600/36 #F5F5F7; message 21/400/29 #86868B; one 480x56 field (radius 12, fill rgba(255,255,255,.04), 1 px #6E6E73 border, 2 px #0071E3 when focused, the label floats from 17 to 12 px); a privacy note; one 320x44 Continue button. Shot: `web-signin.png`.
- web sign-up offer: the same sheet shape with two side-by-side plan cards and full-width blue pills (seen on the first load of the home page, closed with its X).
- HIG accounts (tvOS): ask for the minimum, prefer signing in on another device, do not ask a shared account to pick a profile every time.

### Profiles

- HIG accounts and video: tvOS 16 and later share one sign-in across users and switch profiles automatically with the current viewer; if a playback request names no profile, ask once before playing.
- HIG lockups: people are shown as monograms, a circular picture with the name under it, initials when there is no image (the web cast row matches: 118 px circles, name and role under).
- iPhone: the TV app shows the viewer as a 40 pt initials avatar at the top right of every top level tab; it opens the Apple Account sheet (measured under "Account sheet" below). There is no profile picker on the phone.

### Tab bar and top navigation

- HIG tab bars (tvOS): 68 pt tall, top edge 46 pt from the top of the screen, neither can change; translucent with only the selected tab opaque; when focus enters it the selected tab gets a drop shadow; tabs select after a short delay on focus (UIKit focus).
- HIG tab bars (iOS, iPadOS): tab bars can minimize on scroll and fold an accessory (like a mini player) inline; prefer filled symbols; on iPad the tab bar sits at the top and can convert to a sidebar.
- web: a 52 px black header: product glyph and "Open TV" at the left (13/400 at 64 percent), centered text tabs (16/600, 6 px radius, padding 0 6), search field and Sign In at the right. Shot: `web-home.png`.

### Focus effect (tvOS)

- HIG focus: tvOS mostly shows focus with the parallax effect, not a ring; a focused item is elevated to the foreground, sways gently with the touch surface, and its surface shines with a moving highlight; after a period of inactivity out-of-focus content dims and the focused element expands (HIG images).
- HIG focus: five states, all visually distinct: unfocused, focused, highlighted (pressed: for example a brief color inversion), selected, unavailable. Supply art for the larger focused size.
- HIG images: layered images with 2 to 5 layers make the parallax; the background layer is opaque; keep text in the front layer; keep it subtle ("designed to be almost unnoticeable").
- HIG lockups: posters hide title and subtitle until focused; caption buttons tilt in the direction of the swipe (up and down in a column, left and right in a row, both in a grid).
- HIG materials and UIKit (`adjustsImageWhenAncestorFocused`): focused image views and buttons take on Liquid Glass on supported Apple TV models.
- Apple publishes no number for the focus scale, shadow or timing; measuring them needs the tvOS simulator (busy with another agent's walk today) or an Apple TV. Open item.
- web keyboard focus (web r1): every focusable element (posters, cards, people, buttons, links, the search field, Sign In) takes `box-shadow: 0 0 0 4px rgba(10,132,255,.6)` with `outline: none` and `outline-offset: 1px`, following the element's radius; the page dots instead take a 10 px outline in the same blue on the dot; focus also turns on the poster's hover scrim (`--lockupHoverBGColor` #3333334D, rgba(51,51,51,.3)) and its "..." button (`:focus-within`). The ring shows for keyboard focus only (`:focus-visible`).
- web hover: lockups do not scale on hover in the web app; the only press feedback measured is `scale` over 0.1 s ease-in on buttons and the global `opacity .1s ease-in` transition (web vars `--global-transition`).

### Motion (observable)

| What | Value | Source |
|---|---|---|
| Global transition | opacity 0.1 s ease-in | web vars |
| Button press | scale, 0.1 s ease-in | web |
| Artwork fade-in | opacity 0.1 s ease-in on the image | web |
| Interactive overlay over video | at least 0.5 s delay | HIG video |
| tvOS focus timing | not published | |

## iPhone TV app and iOS Settings

Walked on 2026-10-21 on the owner's iPhone 13 Pro (iphone-a, iOS 27.0.1, dark mode, 390x844 pt at 3x) with a browse-only XCUITest runner: screenshots from `XCUIScreen.main.screenshot()` and the app's accessibility tree (`debugDescription`) at every stop. Taps were limited to tabs, the account button and its Close, the search field, result rows, Back and Settings rows; nothing was played, bought, signed in or out, or changed. The runner was uninstalled afterwards.

How to read these numbers: frames (x, y, width, height) are points from the accessibility tree. Font sizes are estimated from a single-line label's box height divided by 1.19 (SF Pro's line height), calibrated on the large title (34 pt, box 40.7 pt) and checked against cap height (0.705 em): the large title's cap is 24.0 pt (34 pt), a shelf title's cap 13.3 pt (19 pt). Weights are by eye. Colors are sampled pixels; radii are fitted to the edge of the corner curve (Apple uses continuous corners, so a circle fit is within about 1 pt). Sources: `ios-*.png` screenshots and `trees/ios-*.tree.txt` in `apple-ref/`.

### Frame and navigation (TV app)

- Page #000000 everywhere (sampled). Content margin 20 pt left and right for shelves, cards and hero text; navigation chrome sits at 16 pt.
- Top level tabs (Home, Search, Library): a 54 pt navigation bar at y 47 holding a large title at x 16, 34 pt bold white (box 40.7), and the account button at the top right: a 40x40 avatar circle with initials at x 332 (shot `ios-tv-home-top.png`).
- Tab bar: a floating Liquid Glass capsule 348x62 at x 21, y 761, so it ends 21 pt above the bottom of the screen and 21 pt from each side. Five tabs of 83x54: Home, Apple TV, Store, Library, Search; icon about 28 pt (SF Symbols `house.fill`, `bag.fill`, `rectangle.stack.fill`, `magnifyingglass`), label about 10 pt under it. Over black the glass reads #202020 with a lighter rim at the top edge (#636363 fading to #2A2B2B over 1 pt); icons and labels #F9F9F9; the selected tab sits in a darker inner capsule (#0A0A0A) and its icon and label turn system blue (#0697FF sampled, HIG Blue #0091FF). The bar stays on screen on every page, detail pages included (shots `ios-tv-library.png`, `ios-tv-show-top.png`).
- Detail pages have no title bar: a 44x44 Back button (a 36 pt glass circle) at x 16, y 47 and a 36x36 Share circle at x 334 float over the artwork. Once the hero scrolls away, the primary pill (178x48) and the + circle move up between them into the top bar, so the main action is always one tap away (shot `ios-tv-show-1.png`).

### Home

- Hero: full bleed 390x680 (81 percent of the screen), running under the status bar and the navigation bar. Bottom up: the page indicator (170x61 hit area centered at y 624; the current page is a short capsule, the others dots), the button row at y 576 (primary pill 178x48, fill #F5F5F5, dark label about 17 pt semibold (by eye); a 48x48 glass circle with + 12 pt to its right, #393A3B over the art), a meta line at y 537 (a 21 pt provider glyph, genres about 14 pt (box 17.0) separated by dots, a 12x12 age rating glyph), the title as a 306x60 logo, an award line above it. Everything centered in a 350 pt column (shot `ios-tv-home-top.png`, tree `ios-tv-home-top`).
- Shelf header: a 51 pt row (72 pt with a subtitle) with the title about 19 pt bold white (box 23.0, cap 13.3) and a chevron after it; the subtitle about 14 pt (box 17.0) in #8D8D93, which is iOS secondary label (rgba(235,235,245,.6)) on black.
- Posters 99.3x149 (2:3), 12 pt apart, radius about 11 pt, 3.4 visible so the fourth peeks. No title under the poster; the name lives in the accessibility label only. Top 10 rows put an ordinal (about 27 pt bold, box 32.3) at the top left and a genre caption (about 11 pt, box 13.3) at the bottom, both over the art (shot `ios-tv-home-rows-1.png`).
- Rhythm: header 51 + posters 149 + 18 to the next header, a 218 pt pitch.
- Feature card ("Ice-Cold Survival"): 350x450 portrait (7:9), radius about 17 pt, 12 pt to the next card which peeks at the right; bottom left 18 pt in: a "New" capsule, the title logo, the meta line and "#1 Movie on Apple TV"; a 28 pt mute control at the top right; the preview autoplays with the app's own mute default (shot `ios-tv-home-rows-3.png`).
- Brand card: a full-width card with a poster collage, the brand logo, two centered lines of large semibold text (by eye) and the white pill (shot `ios-tv-home-rows-4.png`).

### Title page (show and movie)

- The same 390x680 hero. Bottom up, centered: a schedule or award capsule ("All Episodes Now Available", "2026 Oscar Winner"; text about 14 pt, box 17.0), the title logo, the genres line with a provider chevron link, then the primary pill 178x48 and the 48 pt + circle (y 520 on a show, 541 on a movie). Under the buttons, left aligned at x 20: two lines of synopsis with an inline MORE pill (55x21), then the meta line "2022 · 53m" with rating and format badges (about 14 pt, box 17.0, #C5C9C8 over the faded art). Shots `ios-tv-show-top.png`, `ios-tv-movie-top.png`.
- Season picker: "Season 1" with an up-down chevron as a 98.7x32 button in title weight, 8 pt above the cards.
- Episode cards 238.7x226.3 (about 1.05:1), radius about 13 pt, 12 pt apart, 1.5 cards visible. The still fills the card and the text sits on it over a dark ramp, 12 pt in: tag "EPISODE 1" about 11 pt uppercase (box 13.3, #D0D4D1), title about 14 pt semibold white (box 17.0), three lines of synopsis about 13 pt (box 46.3, #C8C8C6), duration about 12 pt (box 14.3) and a More (ellipsis) button at the bottom right (shot `ios-tv-show-1.png`).
- Trailers: 238.7x168.3 cards with the title and "▶ 2m" over the bottom of the art.
- Related: the Home poster lockup (99.3x149).
- Cast and Crew: 99.3 pt circles, the name 8 pt under the circle about 12 pt white (box 14.3), the role about 11 pt grey (box 13.3), centered, up to two lines (shot `ios-tv-show-2.png`).
- How to Watch: a 322x79 card, fill #1C1C1E, radius about 13 pt, with the channel tile and "Sign Up to Watch".
- About: a 322x153 card in the same #1C1C1E: title, genre in small caps, four lines of synopsis with MORE (shot `ios-tv-show-3.png`).
- Information, Languages, Accessibility: section titles about 22 pt bold (box 26.0, iOS Title 2 emphasized), each term about 12 pt white (box 14.0) with its value in grey under it, one item every 49 pt (shot `ios-tv-show-4.png`).

### Search

- Search tab: large title, then a 358x42 search field at x 16 ("Shows, Movies, and More", dictation glyph at the right), then category tiles 108.7x163.3, three per row, 12 pt apart, radius about 11 pt, the name about 16 pt bold white at the bottom left 8 pt in (shot `ios-tv-search.png`).
- Results: the field moves into the top bar (305x42 with a clear button) next to a 42 pt Close circle; scope pills 32 pt tall (Top Results, TV Shows, Movies), the selected one filled system blue with white text, the others plain white text; result rows 350x92.3 with a 48 pt wide poster thumbnail (radius about 7 pt), title about 15 pt (box 18.0), meta "TV Show · Thriller" about 12 pt (box 14.3) in grey, an ellipsis More at the right and a hairline between rows that starts at the text (shot `ios-tv-search-show-results.png`).

### Library

- Large title and account button as on Home. The empty state is two centered lines: "Your Library Is Empty" about 21 pt bold (box 25.3) and the explanation about 14 pt grey (box 17.0); while loading, "Loading Your Library" in the same style (shot `ios-tv-library.png`).

### Account sheet (TV app, view only)

- A sheet over Home: fill #1C1C1E, a 54 pt header at y 63 with a 28 pt Apple Account glyph and "Apple Account" about 17 pt semibold (box 20.3) at x 52, and a 36 pt Close circle at the top right.
- Grouped rows in #2C2C2E, 358 wide at x 16, radius about 26 pt: the account row 85 pt tall (avatar, name about 16 pt (box 19.3), "Account info, payments and settings" about 12 pt in #9F9FA5); then 51 pt rows (Purchase History, Playback, Sports, Privacy & Access) with the label at x 32 and a chevron; 35 pt between groups; a footnote under a group about 12 pt in #98989F; three 114x56.3 action tiles 8 pt apart whose glyph and label are system blue (#0091FF sampled) (shot `ios-tv-account.png`).

### iOS Settings

- Root: page #000000; a 106 pt navigation bar with the large title "Settings" (34 pt bold, box 39.7) at x 16, y 105.7. Inset grouped cards 358 wide at x 16 (16 pt margins), fill #1C1C1E, radius about 26 pt, 35 pt between groups. The account card is 90 pt tall with a 60 pt avatar at x 32 (16 pt in), the name about 19 pt semibold (box 23.0) and "Apple Account, iCloud, and more" about 12 pt (box 14.3) in #98989F. Standard rows are 51 pt tall: a 28 pt rounded-square icon at x 29.7, the label at x 71 about 16 pt white (box 19.3), the value right aligned ending at x 336.6 in #98989F, a 6.7x11.3 chevron at x 349 in #5A5A5E; separators #38383B, 1 pt, starting at the label (x 71), not at the card edge. The search field is a floating glass capsule at the bottom (334x28 field at x 28, y 778) above the content (shot `ios-settings-root.png`).
- Colors match iOS's semantic dark palette: label #FFFFFF, secondary label #98989F on #1C1C1E (rgba(235,235,245,.6)), tertiary #5A5A5E (rgba(235,235,245,.3)), grouped background #000000 and #1C1C1E, the sheet's inner groups #2C2C2E.
- Apple Account page: inline title "Apple Account" about 17 pt semibold (box 20.7) centered in a 54 pt bar with a 44 pt Back glass circle; a 100 pt contact photo centered at y 121, the name about 27 pt bold (box 32.3) and the email about 16 pt grey under it; rows 52 pt tall in 358 wide cards, radius about 26 pt (shot `ios-settings-apple-account.png`; it shows the owner's personal data and stays private).
- General: inline title as above; a header card 358x195.7 with a 60 pt icon, "General" about 21 pt bold (box 25.3) and a four line description about 13 pt; then 51 pt rows in groups (three, one, then a longer one), 35 pt apart (shot `ios-settings-general.png`).

### Full capture, second pass (2026-10-21)

Every reachable screen of the TV app on iphone-a, numbered and indexed in `ref/apple-tv-iphone/` (143 screenshots, `index.md`), and tv.apple.com at 1440 in `ref/apple-tv-web/` (34 screenshots, `index.md`). Both folders are gitignored. The iPhone pass covers: Home (19 screens), the Apple TV channel tab (17), Store (17), Search with categories, a category page, suggestions, results and scopes, Slow Horses with the season menu and Season 2, F1 The Movie, badge rows on Severance, Napoleon, Oppenheimer and Dune, Library, the account sheet with Privacy & Access and Sports, and Settings > Apps > TV. Frames below come from the accessibility trees of that pass.

#### Badge row (title page meta line)

iPhone (shots `ref/apple-tv-iphone/070-show-top.png`, `105-badges-oppenheimer-top.png`, `101-badges-severance-top.png`, `103-badges-napoleon-top.png`; trees `show-top`, `badges-*-top`):
- One line under the synopsis at x 20, text about 14 pt (box 17.0) in light grey over the faded art, then glyphs 13 pt tall, vertically centered on the text (y +2), 8 pt apart. The line wraps at the 350 pt column with a 23 pt line pitch: on a phone AD usually drops to the second line.
- Order, as measured on four titles:
  1. Year.
  2. A centered dot separator (4.3 pt box).
  3. Duration ("53m", "2h 37m", "3h").
  4. On store titles only: the Rotten Tomatoes glyph (13x13) and the score ("%93", 14 pt).
  5. The age rating glyph (13x13: `tr-18plus`, `tr-13plus`).
  6. Content advisory glyphs (13x13 each; three on Slow Horses).
  7. 4K (22.5x13).
  8. Dolby Vision (30.7x13).
  9. Dolby Atmos (30.7x13), absent on Oppenheimer.
  10. CC (22.5x13).
  11. SDH (24.8x13).
  12. AD (22.5x13).
- The accessibility labels spell each badge out ("Closed Captions Available", "Subtitles for the Deaf and Hard of Hearing Available", "Audio Descriptions Available").
- HDR10, HD and 5.1 did not appear on these titles in this storefront; Dolby 5.1 shows only in the Languages section ("English (Dolby 5.1, AAC, AD)").

web (web, Slow Horses and Napoleon at 1440):
- `ul.badge-row` is flex with a 6 px gap. Every badge is 13 px tall at opacity .8 in 64 percent white.
- The age rating badge (13x13, opacity 1) sits at the end of the genres line, 6 px after it.
- On the year line ("2022 · 53m", 13/600 at 64 percent) come content advisories (13x13 each), then 4K (22.5x13), Dolby Vision (30.7x13), Dolby Atmos (30.7x13), CC (22.5x13), SDH (27.7x13) and AD (22.5x13).
- Same order and the same widths as on the iPhone, except SDH (27.7 vs 24.8) and the web's 6 px gap against the phone's 8 pt.

#### Title page sections, top to bottom

- Apple Original show (iPhone and web): hero and buttons, the badge row, the Season picker with episode cards, Trailers, Related, Cast and Crew, How to Watch, About, Information (Released, Rated, Content Advisories, Region of Origin), Languages (Original Audio, Audio, Subtitles), Accessibility (one paragraph per badge: CC, SDH, AD, each led by its glyph).
- Store movie on iPhone (Oppenheimer, shots 105 to 116):
  - The hero buttons become two off-white pills, "Buy ₺169,99" (one row) and "Rent ₺24,99" with the + circle (the next row), both 48 pt tall.
  - Then Trailers, Related, How to Watch, Cast and Crew, About and Information.
  - About scrolls sideways: the title card, then a second card.
  - Information lists Studio, Released, Run Time, Rated and Region of Origin, then a copyright line in grey, about 12 pt.
  - Languages and Accessibility follow, as on the show.
  - There is no separate Ratings section: the critic score lives only in the badge row.
- Section titles about 22 pt bold (box 26.0); each Information item is a 12 pt white term over a grey value, one item every 49 to 50 pt (iPhone trees).

#### Season picker and episodes

- iPhone: the 98.7x32 "Season 1" button opens a glass menu 250x260 anchored under it at x 8, with one 40 pt row per season (Season 1 to 6). Choosing Season 2 swaps the episode row in place and the button reads "Season 2"; nothing else on the page moves (shots `071-show-season-picker.png`, `072-show-season-2.png`).
- web: the season select opens a menu over the page (`ref/apple-tv-web/033-show-season-menu.png`). The "..." on an episode card opens a 185x96 menu with Go to Episode, Share and Copy Link (radius 6) (`034-show-episode-card-menu.png`). An episode has its own page (`027`, `028`).

#### Search

- iPhone suggestions while typing (shot `066-search-suggestions.png`): the field moves into the top bar with a Close circle; text hints come first as full-width 45 pt rows (a magnifier glyph, the hint about 15 pt at x 46.7), then title rows (350x92.3, as in results), over the keyboard.
- iPhone category page (Search > Drama, `059` onward): a header image with the category name, then a large feature card and shelves.
- web suggestions (`032-search-suggestions.png`): a dropdown under the header field, x 1010 and 288 px wide. Three text hints come first (23 px rows, 13/400 at 92 percent), then title rows 66 px tall with a thumbnail, title and "TV Show · Thriller".

#### Apple TV channel tab and Store (iPhone, browse only)

- Apple TV tab: large title "Apple TV", a collage hero with "Watch Apple Originals", the same 178x48 pill, then shelves (shots `020` to `036`).
- Store tab: large title "Store", a full-bleed hero for one film (logo, "Movie · Horror", a three-line synopsis, a "More Info" pill with an info glyph and the + circle), then "Top Movies Chart" with ordinal posters and further rows (shots `037` to `053`).

#### Account sheet subpages and TV app settings

- Privacy & Access shows a single destructive row "Clear Play History" in red (not tapped) with a footnote; Sports is one toggle row, "Sync My Sports", with a footnote and a link (shots `128` to `135`).
- iOS Settings > Apps > TV (shots `137` to `143`) uses the standard inset grouped style measured above, in these groups: Allow TV to Access (Siri, Search, Live Activities, Cellular Data), Streaming Options (Use Cellular Data, Wi-Fi: High Quality), Download Options (Use Cellular Data, Wi-Fi: Fast Downloads, Languages), Automatic Downloads with a footnote, Audio (Audio Language: Auto, with a footnote), Appearance (Dark), Device Preferences (Show Sports Scores, Use Play History) and more below.

### What the iPhone adds to the web and HIG picture

- Black, not #1F1F1F: on the phone the page is pure black and grouped surfaces are #1C1C1E (one level) and #2C2C2E (inside a sheet).
- The secondary text is iOS's semantic grey (#8D8D93 on black, #98989F on #1C1C1E), not white at an opacity as on the web.
- The phone uses system blue for state (selected tab, selected search scope, account sheet actions); the primary pill stays off-white (#F5F5F5) with dark text.
- Radii grow with size: about 7 pt thumbnails, 11 pt posters and tiles, 13 pt episode and info cards, 17 pt feature cards, 26 pt grouped lists, full capsules for buttons, the tab bar and search fields.
- Rows: 51 pt list rows (52 on the account page), 35 pt between groups, 16 pt side margins in Settings and 20 pt in the TV app's content.

## What makes it feel Apple

1. The artwork is the only color. Chrome is black, #1F1F1F and white at four fixed opacities (100, 92, 70, 64 percent; 40 for tertiary), and a hairline is 10 percent white, not a grey hex.
2. One accent, used for state and the few true actions: the selected tab, the selected search scope and sheet actions on iPhone (#0091FF), the sheet primary and links on the web (#0A84FF). Progress bars and the hero primary pill stay white (#F5F5F5 with dark text on iPhone).
3. Full-bleed 16:9 heroes with layered, eased scrims (a left wash, a bottom ramp in six stops, a soft vignette) instead of one flat gradient, and a logo instead of typed titles.
4. One family, two to three weights per screen. Hierarchy is size and weight; captions drop to 70 percent white, never to a tinted grey.
5. A fixed column grid with 20 px (web) or 40 pt (tvOS) gutters and identical card sizes per row; the next card peeks at the edge.
6. Corners that scale with the card: on iPhone about 7 pt thumbnails, 11 pt posters, 13 pt episode cards, 17 pt feature cards, 26 pt grouped lists; on the web 14 px cards, 8 px thumbnails, 12 px grouped cards, 20 px banners; full capsules for buttons, search fields and the tab bar.
7. A 0.5 px inner keyline at 25 percent white on every artwork, so dark art never melts into the page.
8. Text on art only over an eased legibility gradient (0, .13, .21, .30, .40 over about 140 px), and always bottom aligned.
9. Primary buttons are 40 px white pills with 30 px side padding; secondary actions are 40 px round glass buttons with a single glyph.
10. Glass is reserved for controls that float over content (the iPhone tab bar as a capsule 21 pt off the edges, Back and Share circles, page dots, More buttons); content surfaces are opaque (#1C1C1E on iPhone, 5 to 10 percent white on the web).
11. On TV focus is scale, lift, shadow, parallax and sheen together; color never marks focus alone, and focused posters reveal their title only on focus.
12. Motion is short and quiet: 0.1 s ease-in fades and presses on the web, and on TV a parallax sway that the HIG calls "designed to be almost unnoticeable".

## Screenshot files (private, `apple-ref/` in the session scratchpad)

- `web-home.png`, `web-home2.png`, `web-home-hdr.png`, `web-home-1440.png` (1440x1080), `web-home-1920.png` (1920x1080): home, hero carousel and header.
- `web-show.png`, `web-show-s1.png` to `web-show-s5.png`: Slow Horses product page, episodes, trailers, related, cast, About and Information.
- `web-show-episode-hover.png`: an episode card under the pointer.
- `web-movie.png`, `web-movie2.png`, `web-movie-s1.png`, `web-movie-s2.png`: F1 The Movie product page.
- `web-search.png`: search results for "slow horses".
- `web-signin.png`: the sign-in sheet, view only.
- iPhone (`ios-*.png`, trees in `trees/`): `ios-tv-root.png`, `ios-tv-home-top.png`, `ios-tv-home-rows-1.png` to `-4.png`, `ios-tv-home-back-top.png`, `ios-tv-account.png`, `ios-tv-account-1.png`, `ios-tv-search.png`, `ios-tv-search-show-results.png`, `ios-tv-search-movie-results.png`, `ios-tv-show-top.png`, `ios-tv-show-1.png` to `-5.png`, `ios-tv-movie-top.png`, `ios-tv-movie-1.png` to `-3.png`, `ios-tv-library.png`, `ios-tv-library-1.png`, `ios-settings-root.png`, `ios-settings-root-1.png`, `ios-settings-root-2.png`, `ios-settings-apple-account.png` (personal data), `ios-settings-apple-account-1.png`, `ios-settings-general.png`, `ios-settings-general-1.png`.
