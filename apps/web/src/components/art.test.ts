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
type Spot = [name: string, ramp: Ramp, page: string, box: [number, number], at: [number, number], label: "pill" | "word" | "line"];
// "pill": a white label or glyph on the fill; "word": a round button's word under its circle, 70 percent white on
// the art; "line": the hero's reason, 92 percent white on the art at its 560 px end. The phone title's rows sit under
// its art, on the page.
const spots: Spot[] = [
  ["home tv, Details circle", "hero", dark.bg, [1832, 600], [0.1662, 0.755], "pill"],
  ["home tv, Details word", "hero", dark.bg, [1832, 600], [0.1695, 0.89], "word"],
  ["home desktop, Details circle", "hero", dark.bgDesktop, [2212, 600], [0.1376, 0.755], "pill"],
  ["home phone, Details circle", "heroPhone", dark.bgPhone, [358, 520], [0.4976, 0.8731], "pill"],
  ["home tv, the reason", "hero", dark.bg, [1832, 600], [0.3253, 0.68], "line"],
  ["home desktop, the reason", "hero", dark.bgDesktop, [2212, 600], [0.2694, 0.68], "line"],
  ["home phone, the reason", "heroPhone", dark.bgPhone, [358, 520], [0.95, 0.819], "line"],
  ["title tv series, Start over", "title", dark.bg, [1920, 1080], [0.4398, 0.4054], "pill"],
  ["title tv series, Remove from Continue Watching circle", "title", dark.bg, [1920, 1080], [0.3184, 0.5503], "pill"],
  ["title tv series, Remove from Continue Watching word", "title", dark.bg, [1920, 1080], [0.3535, 0.63], "word"],
  ["title tv movie, Watched", "titleMovie", dark.bg, [1920, 1080], [0.3179, 0.3968], "pill"],
  ["title tv movie, In Library word", "titleMovie", dark.bg, [1920, 1080], [0.2311, 0.63], "word"],
  ["title desktop series, Start over", "title", dark.bgDesktop, [2300, 1438], [0.3671, 0.3046], "pill"],
  ["title desktop movie, Watched", "titleMovie", dark.bgDesktop, [2300, 1438], [0.2624, 0.3054], "pill"],
];

test("every secondary over a backdrop keeps 4.5:1 over white art", () => {
  const rows = spots.map(([name, ramp, page, [w, h], [x, y], label]) => {
    const art = under(ramp, page, w, h, x, y);
    const ratio = label === "pill" ? contrast(WHITE, over(WHITE, FILL, art)) : contrast(over(WHITE, label === "word" ? 0.7 : 0.92, art), art);
    return { name, ratio: +ratio.toFixed(2) };
  });
  console.log(rows.map((r) => `${r.ratio.toFixed(2)}  ${r.name}`).join("\n"));
  for (const r of rows) expect(r.ratio, r.name).toBeGreaterThanOrEqual(4.5);
});
