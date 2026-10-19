import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { cssPath, generate, tokensPath } from "../../scripts/tokens";

test("tokens.css matches design/tokens.json (run `bun run tokens`)", () => {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  expect(readFileSync(cssPath, "utf8")).toBe(css);
  expect(css).toContain("--accent: #E5372A;");
  expect(css).toContain("--primary: #FFFFFF;");
  expect(css).toContain('--font-display: "Inter Variable", Inter, system-ui, sans-serif;');
  expect(css).toContain("--weight-display: 800;");
  expect(css).toContain("--tracking-display: -0.035em;");
  expect(css).toContain("--lift-focus-edge: inset 0 0 0 1px rgba(255, 255, 255, 0.5);");
  expect(css).toContain("--lift-primary-shadow: 0 3px 16px rgba(0, 0, 0, 0.28);");
  expect(css).toMatch(/:root\[data-form="phone"\] \{\n {2}--type-hero: 34px;/);
  // Dark only: no light theme block; the dark theme keeps its soft 14 px halo.
  expect(css).not.toContain('data-theme="light"');
  expect(css).toMatch(/:root \{[^}]*--focus-halo: 14px;/);
});

test("selected states use the generated tokens, not raw white edges", () => {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  expect(css).toContain("--selected-edge: inset 0 0 0 2px rgba(255, 255, 255, 0.55);");
  for (const file of ["title.css", "browse.css", "search.css"]) {
    const sheet = readFileSync(new URL(`../screens/${file}`, import.meta.url), "utf8");
    expect(sheet).not.toMatch(/inset 0 0 0 2px rgba\(255, 255, 255, \.5\d?\)/);
  }
});
