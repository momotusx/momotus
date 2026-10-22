import { expect, test } from "bun:test";
import tokens from "../../../../design/tokens.json";
import { type Layer, type Ramp, ramps } from "./ramps";

// The glass check (plans/design-v3-rollout.md, 6): white art, then a backdrop's ramps at a
// secondary's spot, then its 16 percent fill; the white label on top keeps 4.5:1. A spot is the
// label's top edge at its far end, as a fraction of the backdrop, measured on each form's layout.
type RGB = [number, number, number];
const hex = (c: string): RGB => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as RGB;
const over = (fg: RGB, a: number, under: RGB): RGB => fg.map((v, i) => v * a + under[i]! * (1 - a)) as RGB;
const lum = (c: RGB) => {
  const lin = (v: number) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
};
const contrast = (a: RGB, b: RGB) => {
  const [x, y] = [lum(a) + 0.05, lum(b) + 0.05];
  return Math.max(x, y) / Math.min(x, y);
};

/** A layer's alpha at a point of a W x H box, as CSS lays a linear-gradient's line. */
const alphaAt = ({ deg, stops }: Layer, w: number, h: number, x: number, y: number) => {
  const r = (deg * Math.PI) / 180;
  const len = Math.abs(w * Math.sin(r)) + Math.abs(h * Math.cos(r));
  const t = 0.5 + ((x - 0.5) * w * Math.sin(r) - (y - 0.5) * h * Math.cos(r)) / len;
  const i = stops.findIndex(([at]) => at >= t);
  if (i === 0) return stops[0]![1];
  if (i === -1) return stops[stops.length - 1]![1];
  const [[p0, a0], [p1, a1]] = [stops[i - 1]!, stops[i]!];
  return a0 + ((t - p0) / (p1 - p0)) * (a1 - a0);
};
/** White art under `ramp` at (x, y), the bottom layer painted first. */
const under = (ramp: Ramp, page: string, w: number, h: number, x: number, y: number) =>
  [...ramps[ramp]].reverse().reduce<RGB>((c, l) => over(hex(page), alphaAt(l, w, h, x, y), c), [255, 255, 255]);

const { dark } = tokens.color;
const WHITE: RGB = [255, 255, 255];
const FILL = 0.16;
type Label = "pill" | "word" | "line" | "muted" | "dim" | "white";
type Spot = [name: string, ramp: Ramp, page: string, box: [number, number], at: [number, number], label: Label];
// "pill": a white label or glyph on the fill; "word": a round button's word under its circle, 70 percent white on
// the art; "line": text at 92 percent on the art (the reason, the genre line); "muted": 70 percent (the synopsis);
// "dim": 64 percent (the meta line). Text holds 4.5:1 on every form (DECISION, contrast floors). The phone title's
// rows sit under its art, on the page.
const LEVEL: Record<Exclude<Label, "pill">, number> = { word: 0.7, line: 0.92, muted: 0.7, dim: 0.64, white: 1 };
// TV's top shelf at 1920x1080, measured on the e2e Home: every line of the column is taken at the column's
// 820 px edge (x 900, 0.4688), wherever its text ends, and at its first line's top.
const TV_EDGE = 0.4688;
const spots: Spot[] = [
  ["home tv, the genre line", "hero", dark.bg, [1920, 1080], [TV_EDGE, 0.4083], "line"],
  ["home tv, the synopsis", "hero", dark.bg, [1920, 1080], [TV_EDGE, 0.4509], "muted"],
  ["home tv, the meta line", "hero", dark.bg, [1920, 1080], [TV_EDGE, 0.5491], "dim"],
  ["home tv, Details pill", "hero", dark.bg, [1920, 1080], [0.2365, 0.663], "pill"],
  ["home tv, the reason", "hero", dark.bg, [1920, 1080], [TV_EDGE, 0.6046], "line"],
  ["title tv series, Start over", "title", dark.bg, [1920, 1080], [0.4398, 0.3817], "pill"],
  ["title tv series, Remove from Continue Watching circle", "title", dark.bg, [1920, 1080], [0.3184, 0.5503], "pill"],
  ["title tv series, Remove from Continue Watching word", "title", dark.bg, [1920, 1080], [0.3535, 0.63], "word"],
  ["title tv movie, Watched", "titleMovie", dark.bg, [1920, 1080], [0.3179, 0.3968], "pill"],
  ["title tv movie, In Library word", "titleMovie", dark.bg, [1920, 1080], [0.2311, 0.63], "word"],
  ["title desktop series, Start over", "title", dark.bgDesktop, [2300, 1438], [0.3671, 0.3046], "pill"],
  ["title desktop movie, Watched", "titleMovie", dark.bgDesktop, [2300, 1438], [0.2624, 0.3054], "pill"],
];

// Desktop's hero at its extreme boxes (CSS px): the 400 floor at 900 and 1440 wide, 1280x720, 1440x900 and
// 2560x1080 windows. The column sits at 40 with a 75 foot, each line no wider than min(560, 48 percent of the
// hero less 40), so its end is at min(600, 0.48 w); the lines from the foot: buttons 40, reason 16 (16 above),
// synopsis 2 x 21, meta 18, genre 20, 9 apart; Details' glyph at 196 from the left, 104 above the foot.
for (const [w, h] of [[900, 400], [1440, 400], [1280, 578], [1440, 758], [2560, 938]] as const) {
  const edge = Math.min(600, 0.48 * w) / w;
  const y = (up: number) => (h - up) / h;
  const at = `home desktop ${w}x${h}`;
  spots.push(
    [`${at}, the genre line`, "hero", dark.bgDesktop, [w, h], [edge, y(254)], "line"],
    [`${at}, the meta line`, "hero", dark.bgDesktop, [w, h], [edge, y(225)], "dim"],
    [`${at}, the synopsis`, "hero", dark.bgDesktop, [w, h], [edge, y(198)], "muted"],
    [`${at}, the reason`, "hero", dark.bgDesktop, [w, h], [edge, y(147)], "line"],
    [`${at}, Details`, "hero", dark.bgDesktop, [w, h], [196 / w, y(104)], "pill"],
  );
}

// The phone's hero at its boxes: min(width x 680/390, the window less 164) at 360x640, 375x667,
// 390x844 and 430x932. The large title's words end near 150 at the top 53; the centered lines may reach
// 20 from the edge; from the foot: the buttons 56 to 104, the reason 140, the genre and ratings line 166;
// Details' glyph 136 right of the middle.
for (const [w, h] of [[360, 476], [375, 503], [390, 680], [430, 750]] as const) {
  const edge = (w - 20) / w;
  const y = (up: number) => (h - up) / h;
  const at = `home phone ${w}x${h}`;
  spots.push(
    [`${at}, the large title`, "heroPhone", dark.bgPhone, [w, h], [150 / w, 53 / h], "white"],
    [`${at}, the genre line`, "heroPhone", dark.bgPhone, [w, h], [edge, y(166)], "line"],
    [`${at}, the ratings source`, "heroPhone", dark.bgPhone, [w, h], [edge, y(166)], "muted"],
    [`${at}, the reason`, "heroPhone", dark.bgPhone, [w, h], [edge, y(140)], "line"],
    [`${at}, Details`, "heroPhone", dark.bgPhone, [w, h], [(w / 2 + 136) / w, y(91)], "pill"],
  );
}

test("every secondary over a backdrop keeps 4.5:1 over white art", () => {
  const rows = spots.map(([name, ramp, page, [w, h], [x, y], label]) => {
    const art = under(ramp, page, w, h, x, y);
    const ratio = label === "pill" ? contrast(WHITE, over(WHITE, FILL, art)) : contrast(over(WHITE, LEVEL[label], art), art);
    return { name, ratio: +ratio.toFixed(2) };
  });
  console.log(rows.map((r) => `${r.ratio.toFixed(2)}  ${r.name}`).join("\n"));
  for (const r of rows) expect(r.ratio, r.name).toBeGreaterThanOrEqual(4.5);
});
