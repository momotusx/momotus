import { expect, test } from "bun:test";
import { createStore, reconcile } from "solid-js/store";
import type { CalendarSnapshot } from "../core/generated/CalendarSnapshot";
import { calendarIds } from "./ids";

const item = (videoId: string, date: string) => ({ itemId: "tt1", videoId, name: "Show", season: 1, episode: 1, title: videoId, date, poster: null, still: null, watched: false });
const snap = (days: [string, string[]][]): CalendarSnapshot => ({
  today: "2026-10-20",
  days: days.map(([date, ids]) => ({ date, items: ids.map((v) => item(v, date)) })),
  pending: 0,
  failed: [],
  noSchedule: [],
  series: 1,
});

// As liveView applies a push: reconcile, merging, matched by `id`.
function store(first: CalendarSnapshot) {
  const [state, setState] = createStore<{ data: CalendarSnapshot | null }>({ data: null });
  const push = (s: CalendarSnapshot) => setState("data", reconcile(calendarIds(s) as never, { merge: true, key: "id" }));
  push(first);
  return { state, push };
}

test("a day inserted above keeps each day and card its own element", () => {
  const { state, push } = store(snap([["2026-10-21", ["a:1:1", "a:1:2"]]]));
  const day = state.data?.days[0];
  const card = state.data?.days[0]?.items[1];
  push(snap([["2026-10-20", ["b:1:1"]], ["2026-10-21", ["a:1:1", "a:1:2"]]]));
  expect(state.data?.days[1]).toBe(day);
  expect(state.data?.days[1]?.items[1]).toBe(card);
  expect(state.data?.days[0]?.date).toBe("2026-10-20");
});

test("an episode inserted above keeps the cards after it", () => {
  const { state, push } = store(snap([["2026-10-21", ["a:1:2"]]]));
  const card = state.data?.days[0]?.items[0];
  push(snap([["2026-10-21", ["a:1:1", "a:1:2"]]]));
  expect(state.data?.days[0]?.items[1]).toBe(card);
  expect(card?.videoId).toBe("a:1:2");
});
