import { describe, expect, test } from "bun:test";
import { spring } from "./dom";

const FRAME = 1 / 60;

/** Runs a spring at 60 frames a second; `aim(frame)` may move its target. Returns each frame's position. */
function run(frames: number, aim: (frame: number) => number | undefined) {
  const a = { x: 0, v: 0, to: 0, room: 10_000 };
  const xs: number[] = [];
  for (let i = 0; i < frames; i++) {
    const to = aim(i);
    if (to !== undefined) a.to = to;
    spring(a, FRAME);
    xs.push(a.x);
  }
  return xs;
}

const steps = (xs: number[]) => xs.slice(1).map((x, i) => x - (xs[i] as number));

describe("glide spring", () => {
  test("one press eases to its aim in about 350 ms and never overshoots", () => {
    const xs = run(40, (i) => (i === 0 ? 490 : undefined));
    expect(xs[8]).toBeLessThan(470);
    expect(490 - (xs[20] as number)).toBeLessThan(5);
    expect(xs.findIndex((x) => x === 490)).toBeLessThan(30);
    expect(Math.max(...xs)).toBe(490);
    expect(steps(xs).every((d) => d >= 0)).toBe(true);
  });

  test("a held key keeps the speed: a new aim never brings the movement near a stop", () => {
    // A row of 490 every 120 ms (about every 7 frames), eight times, as a held remote button.
    const xs = run(90, (i) => (i % 7 === 0 && i < 56 ? (i / 7 + 1) * 490 : undefined));
    const d = steps(xs);
    const moving = d.slice(0, d.findIndex((s, i) => i > 56 && s < 1));
    for (let i = 1; i < moving.length; i++) expect(moving[i] as number).toBeGreaterThan((moving[i - 1] as number) / 3);
    expect(xs.at(-1)).toBe(8 * 490);
  });

  test("reverse at the end: an aim turned around at full speed never carries past the end", () => {
    // Heading for the end at 6000 px a second, 10 px short of it, when the aim turns back to the start.
    const a = { x: 990, v: 6000, to: 0, room: 1000 };
    const xs: number[] = [];
    for (let i = 0; i < 60; i++) {
      spring(a, FRAME);
      xs.push(a.x);
    }
    expect(Math.max(...xs)).toBeLessThanOrEqual(1000);
    expect(xs.at(-1)).toBe(0);
  });
});
