/** Phone controls are drawn 48 px, not reached by an invisible box a neighbour or a scroller can cut (QA r5 R5-5 to R5-8:
 * title Back 44, Trailer 46, More 21, Versions tabs 36, Discover's chips and rating links 39, every Details 24, Privacy 16,
 * the subtitle sheet's chips, field and Done 44). Each row names the phone rule that sets a control's drawn size and the
 * least it may say; the e2e pin (e2e/phone-targets.spec.ts) measures the boxes in a browser. */

import { expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dir;

function sheets(dir: string): string[] {
  return readdirSync(dir)
    .sort()
    .flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? sheets(path) : name.endsWith(".css") ? [path] : [];
    });
}

const phoneRules = sheets(here).flatMap((file) =>
  [...readFileSync(file, "utf8").matchAll(/([^{}]*)\{([^{}]*)\}/g)].flatMap(([, selector = "", body = ""]) =>
    selector
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part.startsWith('[data-form="phone"]'))
      .map((part) => ({ part: part.slice('[data-form="phone"]'.length).trim(), body })),
  ),
);

/** The first px number of `prop` in the last phone rule for exactly `selector` that sets it, or null. */
function drawn(selector: string, prop: string): number | null {
  const found = phoneRules
    .filter((r) => r.part === selector)
    .flatMap((r) => [...r.body.matchAll(new RegExp(`(?:^|[;\\s])${prop}:\\s*(\\d+(?:\\.\\d+)?)px`, "g"))])
    .map((m) => Number(m[1]));
  return found.length ? (found[found.length - 1] ?? null) : null;
}

// [selector after the phone prefix, property, least px]
const CONTROLS: [string, string, number][] = [
  [".chip", "height", 48],
  [".vkinds .chip", "height", 48],
  ['.chip[role="checkbox"]', "min-height", 48],
  [".dv-clear", "height", 48],
  [".back", "height", 48],
  [".backbar:not(.bar) .back", "width", 48],
  [".info .acts .btn", "min-width", 48],
  [".syn-more", "min-height", 48],
  [".why-raw summary", "line-height", 48],
  [".note a", "min-height", 48],
  [".lp-field", "height", 48],
  [".lp-chip", "height", 48],
  [".lp-done", "height", 48],
];

for (const [selector, prop, least] of CONTROLS) {
  test(`phone ${selector} says ${prop} ${least} px or more`, () => {
    const px = drawn(selector, prop);
    expect(px, `${selector} ${prop}`).not.toBeNull();
    expect(px ?? 0).toBeGreaterThanOrEqual(least);
  });
}

test("phone a.score is 24 px of row plus 12 px of padding each side", () => {
  expect(drawn("a.score", "padding")).toBe(12);
});

test("phone lp-retry and lp-clear are 48 high", () => {
  const css = readFileSync(join(here, "screens/language-picker.css"), "utf8");
  expect(css).toMatch(/\[data-form="phone"\] :is\(\.lp-retry, \.lp-clear\) \{ height: 48px;/);
});
