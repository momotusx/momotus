import { expect, test } from "bun:test";
import { bufferShare, loadingLines, pickedListed, START_AHEAD_S, STALL_AHEAD_S, startFill } from "./loading";

test("the buffer's share: seconds ahead of the playhead over what the player wants", () => {
  expect(bufferShare([], 0)).toBe(0);
  expect(bufferShare([[0, 0.5]], 0)).toBe(0.5 / START_AHEAD_S);
  expect(bufferShare([[0, 1.5]], 0, STALL_AHEAD_S)).toBe(1.5 / STALL_AHEAD_S);
  expect(bufferShare([[0, 10]], 0)).toBe(1);
  // What counts is ahead of the playhead, not the whole range (a stall late in a range).
  expect(bufferShare([[0, 10]], 9, 3)).toBeCloseTo(1 / 3, 5);
  // A resume: the range around the start point counts, not the one before it.
  expect(bufferShare([[0, 5], [600, 601.5]], 600, 3)).toBe(0.5);
  expect(bufferShare([[0, 5]], 600, 3)).toBe(0);
  // A playhead a hair before its range (the element rounds) still counts it, from the range's start.
  expect(bufferShare([[600.1, 603.1]], 600, 3)).toBeCloseTo(1, 5);
  expect(bufferShare([[0.2, 1.0]], 0, 1)).toBeCloseTo(0.8, 5);
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

test("a version picked by hand waits for the section that lists it, not only the addons' pass", () => {
  const row = (key: string) => ({ key }) as never;
  const view = (o: { done?: boolean; rows?: string[]; archive?: string[]; archivePending?: boolean; pluginsPending?: boolean }) => ({
    done: o.done ?? false,
    recommended: null,
    rows: (o.rows ?? []).map(row),
    archive: { rows: (o.archive ?? []).map(row), pending: o.archivePending ?? false },
    plugins: { rows: [], pending: o.pluginsPending ?? false },
  });
  const k = "archive:film:film.mp4";
  expect(pickedListed(null, k)).toBe(false);
  // The addons are done and the Archive still works: not yet (it would open as "no longer listed").
  expect(pickedListed(view({ done: true, archivePending: true }), k)).toBe(false);
  expect(pickedListed(view({ done: true, pluginsPending: true }), k)).toBe(false);
  // Listed in its section, before or after the addons are done.
  expect(pickedListed(view({ archive: [k], archivePending: false }), k)).toBe(true);
  expect(pickedListed(view({ rows: [k] }), k)).toBe(true);
  // Every pass over and it is nowhere: it opens, and the playback says so.
  expect(pickedListed(view({ done: true }), k)).toBe(true);
});
