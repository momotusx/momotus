import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { cssPath, generate, tokensPath } from "../../scripts/tokens";

test("tokens.css matches design/tokens.json (run `bun run tokens`)", () => {
  const css = generate(JSON.parse(readFileSync(tokensPath, "utf8")));
  expect(readFileSync(cssPath, "utf8")).toBe(css);
  expect(css).toContain("--accent: #FFB547;");
  expect(css).toContain('--font-display: "Fraunces Variable", Fraunces, Georgia, serif;');
  expect(css).toMatch(/:root\[data-form="phone"\] \{\n {2}--type-hero: 34px;/);
});
