import { expect, test } from "bun:test";
import type { HomeRow } from "../core/generated/HomeRow";
import type { RowState } from "../core/generated/RowState";
import { allFailed } from "./rows";

const row = (state: RowState, tiles = 0): HomeRow => ({
  rowId: "r",
  title: "Popular",
  source: "Cinemeta",
  state,
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
