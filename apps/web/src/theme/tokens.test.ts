import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { cssPath, generate, tokensPath } from "../../scripts/tokens";

test("tokens.css matches design/tokens.json (run `bun run tokens`)", () => {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  expect(readFileSync(cssPath, "utf8")).toBe(css);
  expect(css).toContain("--accent: #FFFFFF;");
  expect(css).toContain("--primary: rgba(255, 255, 255, 0.92);");
  expect(css).toContain("--key: #0A84FF;");
  expect(css).toContain('--font-display: "Inter Variable", Inter, system-ui, sans-serif;');
  expect(css).toContain("--weight-display: 700;");
  expect(css).toContain("--tracking-display: -0.022em;");
  // The key keeps its v2 name; in v3 it holds the focus lift's drop shadow.
  expect(css).toContain("--lift-focus-edge: 0 30px 60px rgba(0, 0, 0, 0.6);");
  expect(css).toContain("--lift-primary-shadow: 0 3px 16px rgba(0, 0, 0, 0.28);");
  // tv.apple.com's hover over a desktop poster.
  expect(css).toContain("--scrim-hover: rgba(51, 51, 51, 0.3);");
  expect(css).toMatch(/:root\[data-form="phone"\] \{\n {2}--type-hero: 34px;/);
  // Dark only: no light theme block; the halo is the lift's soft 30 px shadow.
  expect(css).not.toContain('data-theme="light"');
  expect(css).toMatch(/:root \{[^}]*--focus-halo: 30px;/);
});

test("selected states use the generated tokens, not raw white edges", () => {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  // v3 selects with a fill, not an edge: the edge token is a no-op.
  expect(css).toContain("--selected-edge: inset 0 0 0 0 rgba(255, 255, 255, 0);");
  for (const file of ["title.css", "browse.css", "search.css"]) {
    const sheet = readFileSync(new URL(`../screens/${file}`, import.meta.url), "utf8");
    expect(sheet).not.toMatch(/inset 0 0 0 2px rgba\(255, 255, 255, \.5\d?\)/);
  }
});

test("the browser bar takes the page color, and the page says it is dark only", () => {
  const tokens = JSON.parse(readFileSync(tokensPath, "utf8"));
  const html = readFileSync(new URL("../../index.html", import.meta.url), "utf8");
  expect(html).toContain(`<meta name="theme-color" content="${tokens.color.dark.bg}" />`);
  expect(html).toContain('<meta name="color-scheme" content="dark" />');
});

// WCAG contrast of a token color on another, each translucent one laid on what it sits on.
type RGBA = [number, number, number, number];
const rgba = (c: string): RGBA => {
  const hex = /^#([0-9a-f]{6})$/i.exec(c)?.[1];
  if (hex) return [parseInt(hex.slice(0, 2), 16), parseInt(hex.slice(2, 4), 16), parseInt(hex.slice(4, 6), 16), 1];
  const n = (c.match(/[\d.]+/g) ?? []).map(Number);
  return [n[0] ?? 0, n[1] ?? 0, n[2] ?? 0, n[3] ?? 1];
};
const over = ([r, g, b, a]: RGBA, [ur, ug, ub]: RGBA): RGBA => [r * a + ur * (1 - a), g * a + ug * (1 - a), b * a + ub * (1 - a), 1];
const lum = (c: RGBA) => {
  const lin = (v: number) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
};
const contrast = (fg: string, bg: string, page: string) => {
  const back = over(rgba(bg), rgba(page));
  const [a, b] = [lum(over(rgba(fg), back)) + 0.05, lum(back) + 0.05];
  return Math.max(a, b) / Math.min(a, b);
};

test("the control edge keeps 3:1 on the page until 1b removes it", () => {
  const c = JSON.parse(readFileSync(tokensPath, "utf8")).color.dark;
  expect(contrast(c.control, c.bg, c.bg)).toBeGreaterThanOrEqual(3);
});

test("a selected chip's note reads on the selected fill", () => {
  // browse.css: `.chip.on` fills with `selected.fill` and its note keeps full 92 percent text
  // (5.37:1); the note's own .8 opacity gave 4.11. The note's alpha is read from the sheet.
  const tokens = JSON.parse(readFileSync(tokensPath, "utf8"));
  const c = tokens.color.dark;
  const sheet = readFileSync(new URL("../screens/browse.css", import.meta.url), "utf8");
  expect(sheet).toMatch(/\.chip\.on \{[^}]*background: var\(--selected-fill\)/);
  const opacity = Number(/\.chip\.on small \{ opacity: ([\d.]+); \}/.exec(sheet)?.[1] ?? /\.chip small \{[^}]*opacity: ([\d.]+)/.exec(sheet)?.[1] ?? 1);
  const [r, g, b, a] = rgba(c.text);
  expect(contrast(`rgba(${r}, ${g}, ${b}, ${a * opacity})`, tokens.selected.fill, c.bg)).toBeGreaterThanOrEqual(4.5);
});
