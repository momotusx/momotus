# Design decision (provisional)

Status: provisional pick made on 2026-10-17 so implementation can proceed. The owner may override by naming systems or pieces from `design/exploration/index.html`; the UI is token driven, so a different system or accent is a change to `design/tokens.json`, not a rewrite.

## The blend

| Piece | Taken from | Why |
|---|---|---|
| Palette, typography, hero | 01 Lobby | Calm, cinematic, premium at 3 m (text 17.0:1, accent 11.0:1 on dark). Fraunces display titles over Inter for every decision. Warm black reads as a cinema, not as another navy streaming app. |
| Accent `#FFB547` ("marquee bulb") | 01 Lobby | The name is a theatre marquee; the accent is its bulb. Used only for focus and the one primary action per screen. |
| Poster rows and the focus ring with a dark halo | 07 Billboard | Art sells titles (Netflix artwork research); the ring plus halo holds 3:1 on any artwork (WCAG 2.4.13). |
| Versions panel: "best pick" card left, grouped table right, hidden-with-reasons row | 10 Transit | The clearest form of the stream picker spec (research/14 5.3): three plain reasons, every intent group visible, full release name on the focused row. |
| Wayfinding | 10 Transit, toned down | Breadcrumbs on deep pages only; no "You are here" label on Home. |
| Episode list | 03 Folio | Reads like a table of contents: number, title, runtime, progress. |
| Onboarding setup check | 04 Readout | Network, Display, Audio, Sources: each a dot plus a word, with a fix action. |
| Phone and desktop light theme | 05 Daylight principles in Lobby's light tokens | Light theme for bright rooms and phones; TV defaults to dark. |

## Brand risk to check

The accent sits near Plex's amber (`#E5A00D`). Distinctiveness comes from the warm black, the serif display face and the marquee motif rather than hue alone. If the owner wants a hue no streaming brand uses, Readout's chartreuse `#B8F23B` is the alternative with no close brand.

## Rules every implementation follows

- One token source: `design/tokens.json`, generated to CSS variables (web), Compose and SwiftUI.
- TV canvas 1920x1080 points; body 29 pt, floor 23 pt; safe area 60 pt top and bottom, 80 pt sides.
- Focus: scale 1.05 on cards (1.025 on large tiles) plus a 4 px accent ring with a dark halo: 14 px soft on the dark theme, where it melts into the background, and a crisp 3 px outer line on the light theme, where a wide halo reads as a gray block (seen on Android P2); never color alone.
- Motion: 180 ms focus, 320 ms panels, interruptible; reduced motion honored.
- Every navigation icon has a visible label; no emoji anywhere; one outline icon set (Lucide).
