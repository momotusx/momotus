/** Worst-case contrast of the player's words over the brightest possible picture (pure white behind
 * the scrim), from the scrim stops in PlayerParts.kt (`scrimStops`) and the layout arithmetic of
 * TvPlayer.kt and PhonePlayer.kt. Rows are appended to bench/m6/contrast.jsonl. The 4.5:1 floor is
 * WCAG 1.4.3 for text; the picture can be anything, so white is the case that decides.
 *
 * Positions are the centres of the words, in dp from the screen edge the scrim grows from, taken from
 * the layouts: change a layout, change the number here (the Kotlin test pins the scrim itself).
 *
 * Run: bun scripts/player-contrast.ts [--write] */

import { appendFileSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { argb } from "./tokens";

const root = join(import.meta.dir, "../../..");
const tokens = JSON.parse(readFileSync(join(root, "design/tokens.json"), "utf8"));
type RGB = [number, number, number];
/** A token color (hex or rgba, through the generator's parser) as its channels and alpha. */
const parse = (css: string): { rgb: RGB; alpha: number } => {
  const n = parseInt(argb(css), 16);
  return { rgb: [(n >> 16) & 255, (n >> 8) & 255, n & 255], alpha: ((n >>> 24) & 255) / 255 };
};
const over = (top: RGB, alpha: number, under: RGB): RGB => top.map((c, i) => c * alpha + (under[i] as number) * (1 - alpha)) as RGB;
const lin = (c: number) => {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const lum = (c: RGB) => 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
const ratio = (a: RGB, b: RGB) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x) as [number, number];
  return Math.round(((hi + 0.05) / (lo + 0.05)) * 100) / 100;
};

// The stops of PlayerParts.scrimStops, (position from the top of the scrim, black alpha).
const stops = (top: boolean, peak: number): [number, number][] =>
  top ? [[0, peak], [0.7, peak * 0.7], [1, 0]] : [[0, 0], [0.25, peak * 0.8], [1, peak]];
const alphaAt = (top: boolean, peak: number, at: number) => {
  const s = stops(top, peak);
  const i = Math.max(1, s.findIndex(([p]) => p >= at));
  const [x0, a0] = s[i - 1] as [number, number];
  const [x1, a1] = s[i] as [number, number];
  return a0 + (a1 - a0) * ((at - x0) / (x1 - x0));
};

const dark = tokens.color.dark;
const text = parse(dark.text);
const muted = parse(dark.muted);
const white: RGB = [255, 255, 255];
const tv = tokens.safeArea.tv;
const phone = tokens.safeArea.phone;

// [where, scrim top?, peak, height dp, centre dp from its screen edge, colour, extra fill over the scrim]
const cases: [string, boolean, number, number, number, ReturnType<typeof parse>, number?][] = [
  // TV foot (380 dp, peak 0.85): bottom margin, then the 120 dp control row, a 34 dp gap, the bar.
  ["tv position bar clocks", false, 0.85, 380, tv.bottom + 120 + 34 + 11, text],
  ["tv action label", false, 0.85, 380, tv.bottom + 10 + 28 + 14, text],
  ["tv action detail line", false, 0.85, 380, tv.bottom + 14, muted],
  ["tv skip buttons (14% fill)", false, 0.85, 380, tv.bottom + 60, text, 0.14],
  // TV top (200 dp, peak 0.75): the title and "Ends at".
  ["tv title", true, 0.85, 200, tv.top + 32, text],
  ["tv ends at", true, 0.85, 200, 70 + 14, text],
  // Phone foot (240 dp, peak 0.85): 16 dp margin, actions (58), 4, bar row (48), 4, transport (60).
  ["phone action detail line", false, 0.85, 240, 16 + 10, muted],
  ["phone action label", false, 0.85, 240, 16 + 32, text],
  ["phone position bar clocks", false, 0.85, 240, 16 + 58 + 4 + 24, text],
  ["phone skip buttons (14% fill)", false, 0.85, 240, 16 + 58 + 4 + 48 + 4 + 30, text, 0.14],
  // Phone top (120 dp, peak 0.75): title and episode label under a 16 dp margin.
  ["phone title", true, 0.85, 120, 16 + 14, text],
  ["phone episode label", true, 0.85, 120, 16 + 14 + 26, text],
];

const date = new Date().toISOString().slice(0, 10);
const rows = cases.map(([where, top, peak, height, centre, fg, fill]) => {
  const at = top ? centre / height : 1 - centre / height;
  const alpha = alphaAt(top, peak, Math.min(1, Math.max(0, at)));
  let ground = over([0, 0, 0], alpha, white);
  if (fill) ground = over(text.rgb, fill, ground);
  // A translucent word is laid on the ground it sits on before it is measured.
  return { date, phase: "P5", check: "player_text_contrast_worst_case_white_art", where, scrimAlpha: Math.round(alpha * 100) / 100, ratio: ratio(over(fg.rgb, fg.alpha, ground), ground) };
});
let failed = 0;
for (const row of rows) {
  const ok = row.ratio >= 4.5;
  if (!ok) failed++;
  console.log(ok ? "ok  " : "FAIL", row.where, row.scrimAlpha, row.ratio);
  if (process.argv.includes("--write")) appendFileSync(join(root, "bench/m6/contrast.jsonl"), `${JSON.stringify(row)}\n`);
}
void phone;
process.exit(failed ? 1 : 0);
