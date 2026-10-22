import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { argb, colors, colorsPath, generate, kotlinPath, type Tokens, tokensPath } from "./tokens";

const tokens = () => JSON.parse(readFileSync(tokensPath, "utf8")) as Tokens;

test("Tokens.kt and the color resources match design/tokens.json (run `bun run tokens`)", () => {
  expect(readFileSync(kotlinPath, "utf8")).toBe(generate(tokens()));
  expect(readFileSync(colorsPath, "utf8")).toBe(colors(tokens()));
});

test("colors become ARGB integers, rgba keeps its alpha", () => {
  expect(argb("#FFB547")).toBe("FFFFB547");
  expect(argb("rgba(12, 9, 6, 0.62)")).toBe("9E0C0906");
});

test("the output carries the values the design decision names", () => {
  const kotlin = generate(tokens());
  expect(kotlin).toContain("accent = Color(0xFFFFFFFF)");
  expect(kotlin).toContain("primary = Color(0xEBFFFFFF)");
  expect(kotlin).toContain("key = Color(0xFF0A84FF)");
  expect(kotlin).toContain("switchOn = Color(0xFF30D158)");
  expect(kotlin).toContain("val selectedFill = Color(0xFF636366)");
  expect(kotlin).toContain("body = 29.sp");
  expect(kotlin).toContain("const val FOCUS_SCALE_CARD = 1.06f");
  expect(kotlin).toContain("val focusRing = 4.dp");
  expect(kotlin).toContain("val focusHaloDark = 30.dp");
  expect(kotlin).toContain("val focusHaloLight = 30.dp");
  expect(kotlin).toContain("const val FOCUS_MS = 180");
  expect(kotlin).toContain("CubicBezierEasing(0.22f, 1f, 0.36f, 1f)");
  expect(kotlin).toContain("poster = TileSize(99.dp, 149.dp)");
  expect(kotlin).toContain("caption = 84.dp,");
  expect(kotlin).toContain("display = 700,");
  expect(kotlin).toContain("label = 600,");
  expect(kotlin).toContain("display = -0.022f,");
  expect(kotlin).toContain("heading = -0.019f,");
});
