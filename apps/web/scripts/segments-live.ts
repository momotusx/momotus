/** The opt-in live check behind the web e2e's skip data (`bun run e2e:live`).
 * The e2e plays synthetic answers in TheIntroDB's and IntroDB's shapes
 * (e2e/fixtures, reached through MQ_DEV_SEGMENTS), so no run waits on a live
 * service. This asks both services for the test film the way the core does
 * and checks that their answers still have the shape the core and the
 * fixtures assume. It stores no answer: segment data never goes to disk or a
 * bench file (plans/m8-skip.md 3.4). The bench row holds hit or miss, the
 * kinds present and the latency per source; a changed shape fails the run. */

import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

type Shape = { problems: string[]; kinds: string[] };
const FILM = "tt1254207";
const isTime = (v: unknown) => v === null || typeof v === "number";

/** TheIntroDB: each kind absent, null, or a list of `{start_ms, end_ms}`. */
function theIntroDb(body: Record<string, unknown>): Shape {
  const shape: Shape = { problems: [], kinds: [] };
  for (const kind of ["intro", "recap", "credits", "preview"]) {
    const items = body[kind];
    if (items === undefined || items === null) continue;
    if (!Array.isArray(items)) {
      shape.problems.push(`${kind} is not a list`);
      continue;
    }
    if (items.some((i) => typeof i !== "object" || i === null || !isTime(i.start_ms) || !isTime(i.end_ms))) shape.problems.push(`${kind} has an item without start_ms and end_ms`);
    if (items.length) shape.kinds.push(kind);
  }
  return shape;
}

/** IntroDB: each kind null or one `{start_ms, end_ms}` of numbers. */
function introDb(body: Record<string, unknown>): Shape {
  const shape: Shape = { problems: [], kinds: [] };
  for (const kind of ["intro", "recap", "outro"]) {
    const item = body[kind] as Record<string, unknown> | null | undefined;
    if (!(kind in body)) shape.problems.push(`${kind} is missing`);
    if (item === null || item === undefined) continue;
    if (typeof item.start_ms !== "number" || typeof item.end_ms !== "number") shape.problems.push(`${kind} lacks numeric start_ms and end_ms`);
    else shape.kinds.push(kind);
  }
  return shape;
}

const CASES = [
  { source: "TheIntroDB", url: `https://api.theintrodb.org/v3/media?imdb_id=${FILM}`, check: theIntroDb },
  { source: "IntroDB", url: `https://api.introdb.app/segments?imdb_id=${FILM}&is_movie=true`, check: introDb },
];

const results = [];
let changed = false;
for (const c of CASES) {
  const started = performance.now();
  const res = await fetch(c.url, { headers: { "user-agent": "Marquee e2e live check" }, signal: AbortSignal.timeout(15_000) });
  const ms = Math.round(performance.now() - started);
  if (res.status === 404) {
    results.push({ source: c.source, hit: false, kinds: [], ms });
    continue;
  }
  if (!res.ok) throw new Error(`${c.source} answered ${res.status}`);
  const shape = c.check((await res.json()) as Record<string, unknown>);
  if (shape.problems.length) {
    changed = true;
    console.error(`${c.source}: the answer's shape changed: ${shape.problems.join("; ")}`);
  }
  results.push({ source: c.source, hit: shape.kinds.length > 0, kinds: shape.kinds, ms, shape: shape.problems.length ? "changed" : "as the fixture" });
  console.log(`${c.source}: ${shape.kinds.length ? `hit (${shape.kinds.join(", ")})` : "miss"}, ${ms} ms`);
}
const row = { date: new Date().toISOString().slice(0, 10), check: "e2e skip fixtures' shape against the live services (no segment data kept)", film: FILM, results };
appendFileSync(fileURLToPath(new URL("../../../bench/m8/skip-live.jsonl", import.meta.url)), `${JSON.stringify(row)}\n`);
if (changed) {
  console.error("Update the synthetic fixtures in e2e/fixtures and the core's parser for the new shape.");
  process.exit(1);
}
