# The Momotus mark

The owner's own drawing of a motmot (the genus is Momotus), `drawing.jpeg`, is the app's mark. The owner's picks are in `plans/owner-picks.md` (2026-10-26, app logo).

| File | What |
|---|---|
| `motmot.svg` | The master: the drawing redrawn at any size. The outline, the silhouette and every region are vector paths traced from the drawing; the airbrush shading inside them is the drawing's own, as smooth layers clipped to those paths. The eye is recoloured to #E0381C (the owner's pick E2), the pupil, ring and lashes as drawn. |
| `motmot-small.svg` | For 32 px and below: the same silhouette with heavier lines and the finest detail (eyelashes, the racket's split, the wing's tuft) left out. All vector. |
| `motmot-mono.svg` | One colour with the inner lines and the eye cut through, for Android's themed icons. |
| `wordmark.svg` | The name in Unbounded SemiBold at -40 thousandths of an em, outlined and kerned, in mint #B3E9C2. Unbounded is SIL Open Font License 1.1 (`OFL-Unbounded.txt`); no font file ships in the apps. |
| `build.ts` | Writes every platform's icons, banners, launch art and marks from these files: `bun design/logo/build.ts` (needs `rsvg-convert` and ImageMagick). |
| `build.test.ts` | Checks the outputs: sizes, the kept parts inside each safe zone, and that the Apple asset catalog compiles clean. |

The icon is pick 4c: the bird at 1.28 of the tile, its body cut by the left and bottom edges, the head, eye, beak and racket tail whole, on near-black #0B0D0E. On Android the art shrinks to 0.95 so those parts stay inside the 66 dp circle no mask cuts; the maskable web icon shrinks it to 0.83 for its 80 percent circle.

Never restyle the drawing: its shapes, line, colours and shading are the owner's.
