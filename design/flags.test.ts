// The language flags (plans/onboarding-language-picker.md, Flags): run with `bun test design/flags.test.ts`.
// Every core language finds a flag or is a glyph language, and the three platforms hold the same set.
import { expect, test } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const read = (path: string) => readFileSync(join(root, path), "utf8");
const overrides: { country: Record<string, unknown>; glyph: string[] } = JSON.parse(read("design/flag-overrides.json"));
const glyphs = new Set(overrides.glyph.map((t) => t.toLowerCase()));

const web = readdirSync(join(root, "apps/web/src/player/flags"))
  .filter((f) => f.endsWith(".svg"))
  .map((f) => f.slice(0, -4))
  .sort();
const android = [...read("apps/android/player/src/main/kotlin/app/marquee/player/Flags.kt").matchAll(/^\s+"([^"]+)" to R\.drawable/gm)].map((m) => m[1]!).sort();
const swift = read("apps/apple/Marquee/Player/FlagSet.swift");
const apple = [...swift.slice(0, swift.indexOf("static let glyphs")).matchAll(/^\s+"([^"]+)",/gm)].map((m) => m[1]!).sort();
const core = (() => {
  const rs = read("crates/mq-runtime/src/languages.rs");
  const table = rs.slice(rs.indexOf("const OPENSUBTITLES"), rs.indexOf("];", rs.indexOf("const OPENSUBTITLES")));
  return [...table.matchAll(/\(\s*"[^"]+",\s*"([^"]+)",\s*"[^"]+",\s*"[^"]+",?\s*\)/g)].map((m) => m[1]!);
})();
const flagFile = (tag: string, has: Set<string>) => {
  const lower = tag.toLowerCase();
  const base = lower.split("-")[0]!;
  return has.has(lower) ? lower : has.has(base) ? base : null;
};

test("the three platforms hold the same flags and the same glyph list", () => {
  expect(web.length).toBeGreaterThan(190);
  expect(android).toEqual(web);
  expect(apple).toEqual(web);
  const glyphList = (text: string) => [...text.matchAll(/"([a-z0-9-]+)"/g)].map((m) => m[1]!).sort();
  expect(glyphList(read("apps/web/src/lib/flag-glyphs.ts"))).toEqual([...glyphs].sort());
  // No glyph language keeps a file.
  expect(web.filter((t) => glyphs.has(t))).toEqual([]);
  const kotlin = read("apps/android/player/src/main/kotlin/app/marquee/player/Flags.kt");
  expect(glyphList(kotlin.slice(kotlin.indexOf("GLYPHS")))).toEqual([...glyphs].sort());
  expect(glyphList(swift.slice(swift.indexOf("static let glyphs")))).toEqual([...glyphs].sort());
});

test("every core language finds a flag or is a glyph language, never both", () => {
  expect(core.length).toBe(113);
  const has = new Set(web);
  for (const tag of core) {
    const file = flagFile(tag, has);
    const glyph = glyphs.has(tag.toLowerCase());
    expect([tag, file !== null || glyph]).toEqual([tag, true]);
    expect([tag, file !== null && glyph]).toEqual([tag, false]);
  }
  const flagged = core.filter((t) => flagFile(t, has) !== null).length;
  expect([flagged, core.length - flagged]).toEqual([97, 16]);
});

test("the policy calls hold", () => {
  for (const tag of ["zh-Hans", "zh-Hant", "yue", "zh-x-bilingual", "ar", "prs", "ps", "ku", "zgh", "syr", "tok"]) expect([tag, glyphs.has(tag.toLowerCase())]).toEqual([tag, true]);
  for (const tag of Object.keys(overrides.country)) expect([tag, web.includes(tag.toLowerCase())]).toEqual([tag, true]);
});
