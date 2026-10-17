import { expect, test } from "bun:test";
import { visibleRange } from "./range";

test("mounts the visible span plus a buffer", () => {
  // 272 px stride, 1600 px wide: tiles 0..5 visible, plus 3 after.
  expect(visibleRange(0, 1600, 272, 20, -1)).toEqual([0, 9]);
  expect(visibleRange(2720, 1600, 272, 20, -1)).toEqual([7, 19]);
});

test("keeps the focused tile's neighbors even before the scroll lands", () => {
  expect(visibleRange(0, 1600, 272, 20, 12)).toEqual([0, 16]);
});

test("empty rows mount nothing", () => {
  expect(visibleRange(0, 1600, 272, 0, -1)).toEqual([0, 0]);
});
