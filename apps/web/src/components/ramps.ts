/** Every backdrop's scrim, the one stop list: Backdrop (art.tsx) draws it, art.test.ts checks it. */

/** A stop of a scrim: where along its direction (0 at the edge it starts from) and how much of
 * the page color covers the art there; past the last stop its alpha holds. */
export type Stop = readonly [at: number, alpha: number];
/** One layer: its CSS angle (0 starts at the bottom, 90 at the left, 180 at the top). */
export type Layer = { deg: number; stops: readonly Stop[] };

/** Every backdrop's scrim (design/DECISION.md, Backdrops and scrims), in the form's page color
 * and the top layer first, as CSS paints them. The glass check (art.test.ts) lays them over white
 * art: the stops are as deep as every secondary row on them needs for its white label. */
export const ramps = {
  /** TV Home's full-bleed top shelf: the page color from the leading edge over the column and from the
   * bottom up into the page, Android's checked stops (`Images.kt` `Scrims.topShelf*`, deepened from the
   * frame's until every word of the column keeps 4.5:1 over white art; art.test.ts). */
  heroTv: [
    { deg: 90, stops: [[0, 0.94], [0.48, 0.82], [0.64, 0.3], [0.8, 0]] },
    { deg: 0, stops: [[0, 1], [0.05, 1], [0.14, 0.8], [0.3, 0.45], [0.5, 0.12], [0.62, 0]] },
  ],
  /** Desktop's full-bleed hero (tv.apple.com): the same checked stops as TV's top shelf, in desktop's
   * page color; with the column held to 48 percent of the width they keep every word at 4.5:1 down to
   * the 400 floor and a 900 wide window (art.test.ts). */
  heroDesktop: [
    { deg: 90, stops: [[0, 0.94], [0.48, 0.82], [0.64, 0.3], [0.8, 0]] },
    { deg: 0, stops: [[0, 1], [0.05, 1], [0.14, 0.8], [0.3, 0.45], [0.5, 0.12], [0.62, 0]] },
  ],
  /** The phone's hero (the iPhone TV app): into the black page from the foot, Android's checked stops
   * (`Scrims.phoneHomeFoot`), and a wash under the large title a little deeper than Android's top, since the
   * web's hero gets shorter (the floating tab bar's cap: 476 on a 360x640 screen): every word keeps 4.5:1
   * over white art on every box (art.test.ts). */
  heroPhone: [
    { deg: 0, stops: [[0, 1], [0.12, 0.94], [0.3, 0.8], [0.42, 0.5], [0.56, 0.15], [0.66, 0]] },
    { deg: 180, stops: [[0, 0.72], [0.18, 0.5], [0.3, 0]] },
  ],
  /** A series: the episode cards sit on the right, so the art there is dimmed for their text, not to black. */
  title: [
    { deg: 90, stops: [[0, 1], [0.3, 0.9], [0.5, 0.7], [0.7, 0.62], [1, 0.7]] },
    { deg: 0, stops: [[0, 1], [0.38, 0]] },
  ],
  titleMovie: [
    { deg: 90, stops: [[0, 1], [0.28, 0.88], [0.55, 0.4], [0.78, 0]] },
    { deg: 0, stops: [[0, 1], [0.22, 0.6], [0.46, 0]] },
  ],
  /** The phone title: the art over the logo, fading into the page under it. */
  titlePhone: [{ deg: 0, stops: [[0.04, 1], [0.5, 0.45], [0.78, 0.1]] }],
} satisfies Record<string, readonly Layer[]>;
export type Ramp = keyof typeof ramps;

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ");
/** The CSS for `layers` over a page of `page` (a #RRGGBB token). */
export const rampCss = (layers: readonly Layer[], page: string) =>
  layers
    .map((l) => `linear-gradient(${l.deg}deg, ${l.stops.map(([at, a]) => `rgba(${rgb(page)}, ${a}) ${+(at * 100).toFixed(2)}%`).join(", ")})`)
    .join(", ");
