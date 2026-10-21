import { expect, test } from "bun:test";
import { bufferShare, loadingLines, START_AHEAD_S, startFill } from "./loading";

test("the buffer's share: seconds ahead of the playhead over what the player wants", () => {
  expect(bufferShare([], 0)).toBe(0);
  expect(bufferShare([[0, 1.5]], 0)).toBe(1.5 / START_AHEAD_S);
  expect(bufferShare([[0, 10]], 0)).toBe(1);
  // A resume: the range around the start point counts, not the one before it.
  expect(bufferShare([[0, 5], [600, 601.5]], 600, 3)).toBe(0.5);
  expect(bufferShare([[0, 5]], 600, 3)).toBe(0);
  // A playhead a hair before its range (the element rounds) still counts it.
  expect(bufferShare([[600.1, 603.1]], 600, 3)).toBeCloseTo(1, 5);
});

test("the fill: the core's progress, then the player's buffer fills the rest, never 1", () => {
  expect(startFill(0, 0)).toBe(0);
  expect(startFill(0.4, 0)).toBe(0.4);
  expect(startFill(0.4, 0.5)).toBeCloseTo(0.7, 5);
  expect(startFill(0.4, 1)).toBe(0.99);
  expect(startFill(1, 0)).toBe(0.99);
  // Out of range inputs stay in range.
  expect(startFill(-1, 2)).toBe(0.99);
  expect(startFill(0.2, -1)).toBe(0.2);
  // More buffer never fills less.
  const steps = [0, 0.1, 0.3, 0.6, 0.9].map((s) => startFill(0.25, s));
  expect([...steps].sort((a, b) => a - b)).toEqual(steps);
});

test("the caption: the player's status, then the core's words, each once", () => {
  expect(loadingLines("Starting 1080p, 1.5 GB from Torrentio", null)).toEqual(["Starting 1080p, 1.5 GB from Torrentio"]);
  expect(loadingLines("Starting 4K, 15.8 GB from Torrentio", "Finding peers, 3 so far")).toEqual(["Starting 4K, 15.8 GB from Torrentio", "Finding peers, 3 so far"]);
  expect(loadingLines("Starting", "Starting")).toEqual(["Starting"]);
  expect(loadingLines(null, undefined)).toEqual([]);
  expect(loadingLines(null, "Opening the video")).toEqual(["Opening the video"]);
});
