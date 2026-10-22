import { expect, test } from "bun:test";
import { type Clock, delayText, delayWords, hold, holdStep, MAX_DELAY, nudge } from "./delay";

/** A clock that only moves when told to, running each timer at its time. */
function testClock() {
  let at = 0;
  let next = 1;
  const timers = new Map<number, { due: number; f: () => void }>();
  const clock: Clock = {
    after: (ms, f) => (timers.set(next, { due: at + ms, f }), next++),
    cancel: (id) => void timers.delete(id),
    now: () => at,
  };
  const run = (ms: number) => {
    const end = at + ms;
    for (;;) {
      const [id, t] = [...timers].sort((a, b) => a[1].due - b[1].due)[0] ?? [];
      if (!t || id === undefined || t.due > end) break;
      timers.delete(id);
      at = t.due;
      t.f();
    }
    at = end;
  };
  return { clock, run };
}

/** What a press held `ms` and then released moves, step by step, rounded to a tenth. */
function pressed(ms: number) {
  const { clock, run } = testClock();
  const moves: number[] = [];
  const h = hold((by) => moves.push(Math.round(by * 10) / 10), clock);
  h.start(1);
  run(ms);
  h.stop(true);
  return moves;
}

test("the first repeat comes at 350 ms, as Stremio's 250 ms wait and its first 100 ms tick", () => {
  // Released at 300 ms: a press, one 0.1 s step.
  expect(pressed(300)).toEqual([0.1]);
  // Released at 400 ms: the tick at 350 ms moved 0.3 s, and the release adds nothing.
  expect(pressed(400)).toEqual([0.3]);
  // Then one every 100 ms, 0.4 s a tick from the first second on.
  expect(pressed(1050)).toEqual([0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 0.4]);
});

test("a hold cut off (blur, a lost pointer, another track) moves nothing more", () => {
  const { clock, run } = testClock();
  const moves: number[] = [];
  const h = hold((by) => moves.push(by), clock);
  h.start(-1);
  run(200);
  h.stop(false);
  run(2000);
  expect(moves).toEqual([]);
});

test("a held press speeds up as Stremio's: 0.3 s a tick, then 0.4, 0.5 and 0.6 from the third second", () => {
  expect(holdStep(350)).toBeCloseTo(0.3, 10);
  expect(holdStep(999)).toBeCloseTo(0.3, 10);
  expect(holdStep(1000)).toBeCloseTo(0.4, 10);
  expect(holdStep(2500)).toBeCloseTo(0.5, 10);
  expect(holdStep(3000)).toBeCloseTo(0.6, 10);
  expect(holdStep(60_000)).toBeCloseTo(0.6, 10);
});

test("a step lands on a tenth and stays within 60 s either way", () => {
  // 0.2 + 0.1 is 0.30000000000000004 in floating point: the value is the tenth.
  expect(nudge(0.2, 0.1)).toBe(0.3);
  expect(nudge(0.1, -0.1)).toBe(0);
  expect(Object.is(nudge(0.1, -0.1), -0)).toBe(false);
  expect(nudge(-1.1, -0.6)).toBe(-1.7);
  expect(nudge(59.9, 0.6)).toBe(MAX_DELAY);
  expect(nudge(-59.9, -0.6)).toBe(-MAX_DELAY);
  expect(nudge(60, 0.1)).toBe(60);
});

test("the value shows its sign and one decimal, and reads in words", () => {
  expect(delayText(0)).toBe("0.0 s");
  expect(delayText(0.3)).toBe("+0.3 s");
  expect(delayText(-1.2)).toBe("−1.2 s");
  expect(delayText(60)).toBe("+60.0 s");
  expect(delayWords(0)).toBe("0.0 seconds");
  expect(delayWords(0.3)).toBe("Plus 0.3 seconds");
  expect(delayWords(-1.2)).toBe("Minus 1.2 seconds");
});
