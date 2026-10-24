import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import type { HomeRow } from "../core/generated/HomeRow";
import type { RowState } from "../core/generated/RowState";
import { allFailed, rowWords } from "./rows";

const row = (state: RowState, tiles = 0): HomeRow => ({
  rowId: "r",
  title: "Popular",
  source: "Cinemeta",
  state,
  ranked: false,
  tiles: Array.from({ length: tiles }, (_, i) => ({ id: `t${i}`, kind: "movie", name: "T", posterShape: "poster" }) as HomeRow["tiles"][number]),
});

test("Home offers one Try again only when every lane ended with nothing and one failed", () => {
  const failed = row({ state: "failed", reason: "offline" });
  expect(allFailed([failed, failed])).toBe(true);
  expect(allFailed([failed, row({ state: "empty" })])).toBe(true);
  expect(allFailed([failed, row({ state: "loading" })])).toBe(false);
  expect(allFailed([failed, row({ state: "ready" }, 1)])).toBe(false);
  expect(allFailed([row({ state: "failed", reason: "late" }, 1)])).toBe(false);
  expect(allFailed([row({ state: "empty" })])).toBe(false);
  expect(allFailed([])).toBe(false);
});

test("no screen names a row's addon but Customize Home, not even beside a row of the same title", () => {
  // The owner, 2026-10-22 (design/DECISION.md, Shelves): Home, the hero, Search, Discover and a
  // row's states read no addon's name; Customize Home (screens/home-rows.tsx) shows it as a quiet line.
  const src = new URL("..", import.meta.url).pathname;
  const reads = ["components", "screens"]
    .flatMap((dir) => readdirSync(`${src}${dir}`).filter((f) => f.endsWith(".tsx")).map((f) => `${dir}/${f}`))
    .filter((f) => /\b(row|r|c)\.source\b/.test(readFileSync(`${src}${f}`, "utf8")));
  expect(reads).toEqual(["screens/home-rows.tsx"]);
  for (const line of Object.values(rowWords)) expect(line).not.toMatch(/Cinemeta|addon/i);
});
