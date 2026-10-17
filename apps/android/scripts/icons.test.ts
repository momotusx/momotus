import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { generate, ICONS, iconsPath, readNodes } from "./icons";

test("Icons.kt matches the Lucide set (run `bun run icons`)", () => {
  expect(readFileSync(iconsPath, "utf8")).toBe(generate(readNodes()));
});

test("circles, rects and lines become path data", () => {
  const kotlin = generate(readNodes());
  expect(kotlin).toContain("val Search: ImageVector by lazy");
  expect(kotlin).toContain('"M3 11a8 8 0 1 0 16 0a8 8 0 1 0 -16 0z"'); // search circle at (11, 11) r 8
  expect(kotlin).toContain("val LibraryBig: ImageVector by lazy");
});

test("every icon the screens need exists in the set", () => {
  const nodes = readNodes();
  for (const name of ICONS) expect(nodes[name]).toBeDefined();
});
