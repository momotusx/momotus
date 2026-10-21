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
