/** Generates src/theme/tokens.css from design/tokens.json, the one token
 * source. Run with `bun run tokens`; a unit test fails when the committed CSS
 * is stale. */

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Scale = Record<string, number>;
export interface Tokens {
  color: { dark: Record<string, string>; light: Record<string, string> };
  font: Record<string, string[]>;
  type: { tv: Scale; phone: Scale };
  space: Scale;
  radius: Scale;
  safeArea: { tv: Scale; phone: Scale };
  focus: { ring: number; halo: { dark: number; light: number }; scaleCard: number; scaleTile: number; rise: number; scaleArt: number; riseArt: number };
  motion: { focusMs: number; panelMs: number; ease: string };
  poster: Record<string, { width: number; height: number }>;
  weight: Scale;
  tracking: Scale;
  glass: { blur: number; saturate: number; highlight: string; shadow: string };
  /** The white edge a card takes on focus or hover, and the primary pill's shadow. */
  lift: { focusEdge: string; focusArt: string; hoverEdge: string; primaryShadow: string };
  /** Selected, never focused: a lighter fill and a bright 2 px edge (tabs, chips, pressed toggles). */
  selected: { fill: string; edge: string };
  /** Laid over art: the desktop poster's hover (tv.apple.com). */
  scrim: Record<string, string>;
  /** First run's poster wall under a sheet: the dim with a blur, the dim where blur is costly (TV), the blur. */
  wall: { dim: string; dimNoBlur: string; blur: number };
  /** First run's sheets: their fill and corner. */
  sheet: { fill: string; radius: number };
}

// The self-hosted @fontsource-variable packages register these family names.
const selfHosted: Record<string, string> = { Inter: "Inter Variable" };

const kebab = (s: string) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
const quote = (f: string) => (/[\s]/.test(f) ? `"${f}"` : f);
const block = (selector: string, lines: string[]) => `${selector} {\n${lines.map((l) => `  ${l}`).join("\n")}\n}\n`;
const px = (prefix: string, scale: Scale) => Object.entries(scale).map(([k, v]) => `--${prefix}-${kebab(k)}: ${v}px;`);
const colors = (c: Record<string, string>) => Object.entries(c).map(([k, v]) => `--${kebab(k)}: ${v};`);
// A form's own page colors (`bgPhone`, `surfacePhone`, `bgDesktop`) replace the base keys on that
// form; a key with no base (`barDesktop`, `bandDesktop`) is only read by name.
const formColors = (c: Record<string, string>, form: string) => {
  const suffix = form[0]!.toUpperCase() + form.slice(1);
  return Object.keys(c)
    .filter((k) => k.endsWith(suffix) && k.slice(0, -suffix.length) in c)
    .map((k) => `--${kebab(k.slice(0, -suffix.length))}: var(--${kebab(k)});`);
};

export function generate(t: Tokens): string {
  const fonts = Object.entries(t.font).map(([role, stack]) => {
    const first = stack[0] ?? "";
    const families = selfHosted[first] ? [selfHosted[first], ...stack] : stack;
    return `--font-${role}: ${families.map(quote).join(", ")};`;
  });
  const posters = Object.entries(t.poster).flatMap(([k, v]) => [
    `--poster-${k}-w: ${v.width}px;`,
    `--poster-${k}-h: ${v.height}px;`,
  ]);
  const root = [
    "color-scheme: dark;",
    ...colors(t.color.dark),
    ...fonts,
    ...px("type", t.type.tv),
    ...px("space", t.space),
    ...px("radius", t.radius),
    ...px("safe", t.safeArea.tv),
    `--focus-ring: ${t.focus.ring}px;`,
    `--focus-halo: ${t.focus.halo.dark}px;`,
    // Dark: a soft halo that melts into the background. A matte of the
    // background fills the gap between element and ring.
    "--focus-shadow: 0 0 0 var(--focus-ring) var(--bg), 0 0 var(--focus-halo) calc(var(--focus-ring) * 2) var(--halo);",
    `--focus-scale-card: ${t.focus.scaleCard};`,
    `--focus-scale-tile: ${t.focus.scaleTile};`,
    // Buttons, rows and cards rise this far on TV focus, over `lift.focusEdge`.
    `--focus-rise: ${t.focus.rise}px;`,
    // TV art (posters, stills, avatars) lifts more than a button: the frames' 10 ft focus.
    `--focus-scale-art: ${t.focus.scaleArt};`,
    `--focus-rise-art: ${t.focus.riseArt}px;`,
    ...Object.entries(t.weight).map(([k, v]) => `--weight-${kebab(k)}: ${v};`),
    ...Object.entries(t.tracking).map(([k, v]) => `--tracking-${kebab(k)}: ${v}em;`),
    `--glass-blur: ${t.glass.blur}px;`,
    `--glass-saturate: ${t.glass.saturate};`,
    `--glass-highlight: ${t.glass.highlight};`,
    `--glass-shadow: ${t.glass.shadow};`,
    ...Object.entries(t.lift).map(([k, v]) => `--lift-${kebab(k)}: ${v};`),
    ...Object.entries(t.selected).map(([k, v]) => `--selected-${kebab(k)}: ${v};`),
    ...Object.entries(t.scrim).map(([k, v]) => `--scrim-${kebab(k)}: ${v};`),
    `--wall-dim: ${t.wall.dim};`,
    `--wall-dim-no-blur: ${t.wall.dimNoBlur};`,
    `--wall-blur: ${t.wall.blur}px;`,
    `--sheet-fill: ${t.sheet.fill};`,
    `--sheet-radius: ${t.sheet.radius}px;`,
    `--motion-focus: ${t.motion.focusMs}ms;`,
    `--motion-panel: ${t.motion.panelMs}ms;`,
    `--ease: ${t.motion.ease};`,
    ...posters,
  ];
  return [
    "/* Generated by scripts/tokens.ts from design/tokens.json. Do not edit. */\n",
    block(":root", root),
    block(':root[data-form="phone"]', [...px("type", t.type.phone), ...px("safe", t.safeArea.phone), ...formColors(t.color.dark, "phone")]),
    block(':root[data-form="desktop"]', formColors(t.color.dark, "desktop")),
    // Dark only (design/DECISION.md v2): no light theme block.
    // Surfaces that stay dark in either theme (the player over video).
    block('[data-scheme="dark"]', [
      "color-scheme: dark;",
      ...colors(t.color.dark),
      `--focus-halo: ${t.focus.halo.dark}px;`,
      "--focus-shadow: 0 0 0 var(--focus-ring) var(--bg), 0 0 var(--focus-halo) calc(var(--focus-ring) * 2) var(--halo);",
    ]),
    "@media (prefers-reduced-motion: reduce) {\n  :root {\n    --motion-focus: 0ms;\n    --motion-panel: 0ms;\n  }\n}\n",
  ].join("\n");
}

export const tokensPath = join(import.meta.dir, "../../../design/tokens.json");
export const cssPath = join(import.meta.dir, "../src/theme/tokens.css");

if (import.meta.main) {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  writeFileSync(cssPath, css);
  console.log(`wrote ${cssPath}`);
}
