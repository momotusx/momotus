import { expect, test } from "bun:test";
import { bezier, HeroPager } from "./hero-pager";

test("the dwell advances to the next page and wraps to the first", () => {
  const p = new HeroPager(3, 8000);
  expect(p.tick(4000, true)).toBeNull();
  expect(p.progress).toBe(0.5);
  expect(p.tick(4000, true)).toBe(1);
  expect(p.progress).toBe(0);
  p.tick(8000, true);
  expect(p.tick(8000, true)).toBe(0);
});

test("one page never advances and a paused clock does not fill", () => {
  const one = new HeroPager(1, 8000);
  expect(one.tick(20_000, true)).toBeNull();
  const p = new HeroPager(3, 8000);
  p.tick(2000, true);
  expect(p.tick(60_000, false)).toBeNull();
  expect(p.progress).toBe(0.25);
});

test("a page by hand stops auto-advance for the session; input starts the dwell over", () => {
  const p = new HeroPager(3, 8000);
  p.tick(6000, true);
  p.input();
  expect(p.progress).toBe(0);
  p.go(2, "manual");
  expect(p.stopped).toBe(true);
  expect(p.tick(60_000, true)).toBeNull();
  expect(p.index).toBe(2);
});

test("a trailer writes the same value the pill shows", () => {
  const p = new HeroPager(3, 8000);
  p.trailer(0.4);
  expect(p.progress).toBe(0.4);
  p.trailer(2);
  expect(p.progress).toBe(1);
});

test("the list growing keeps the page shown", () => {
  const p = new HeroPager(2, 8000);
  p.go(1, "timer");
  p.resize(5, 1);
  expect([p.count, p.index]).toEqual([5, 1]);
  p.resize(1, 3);
  expect(p.index).toBe(0);
});

test("the design's ease starts fast and lands softly", () => {
  const ease = bezier(0.22, 1, 0.36, 1);
  expect(ease(0)).toBe(0);
  expect(ease(1)).toBe(1);
  expect(ease(0.25)).toBeGreaterThan(0.6);
  expect(ease(0.75)).toBeGreaterThan(0.97);
  const linear = bezier(0, 0, 1, 1);
  expect(linear(0.3)).toBeCloseTo(0.3, 3);
});
