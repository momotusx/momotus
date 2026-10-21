/** The first real MDBList calls (plans/research/ratings.md, reviews P1-1 and round 2 P2-1), the
 * way the core asks, both sources each:
 * - three movie ids (a popular title, a new release, an id that does not exist);
 * - one series (Breaking Bad), since the core asks `show` for every series;
 * - one batch of 100 distinct real movie ids from Cinemeta's catalogs, the core's batch size (a
 *   free account takes 10; its refusal is what this would show).
 * It appends one row with the statuses, the raw answers of the small calls and the first error
 * body of the big one to bench/core/ratings.jsonl, and writes the answers as fixtures for
 * crates/mq-runtime/tests/ratings.rs. The key comes from MARQUEE_MDBLIST_KEY or
 * .env.mdblist.local; it is never printed or written, and any echo of it in an answer is replaced
 * before anything is kept.
 *
 *   bun bench/core/mdblist-probe.ts */

import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../..");
const fromFile = () => {
  const file = join(root, ".env.mdblist.local");
  if (!existsSync(file)) return undefined;
  const line = readFileSync(file, "utf8")
    .split("\n")
    .find((l) => l.trim().startsWith("MARQUEE_MDBLIST_KEY="));
  return line?.split("=").slice(1).join("=").trim().replace(/^["']|["']$/g, "");
};
const key = (process.env.MARQUEE_MDBLIST_KEY ?? fromFile())?.trim();
if (!key) {
  console.error("no key: set MARQUEE_MDBLIST_KEY or write it to .env.mdblist.local");
  process.exit(1);
}

const IDS = ["tt0111161", "tt34384900", "tt0000000"];
const SHOW = ["tt0903747"];
const hide = (text: string) => text.split(key).join("<key>");
const fixtures = join(root, "crates/mq-runtime/tests/fixtures/mdblist");
mkdirSync(fixtures, { recursive: true });

/** 100 distinct movie ids from Cinemeta's top and rated catalogs, page by page. */
async function hundredIds(): Promise<string[]> {
  const ids = new Set<string>();
  for (const catalog of ["top", "imdbRating", "year"]) {
    for (let skip = 0; skip < 300 && ids.size < 100; skip += 50) {
      const page = skip ? `/skip=${skip}` : "";
      const response = await fetch(`https://v3-cinemeta.strem.io/catalog/movie/${catalog}${page}.json`);
      if (!response.ok) break;
      const { metas = [] } = (await response.json()) as { metas?: { id: string }[] };
      if (!metas.length) break;
      for (const m of metas) if (/^tt\d+$/.test(m.id) && ids.size < 100) ids.add(m.id);
    }
  }
  return [...ids];
}

/** One call: its status, time, limits and body (the key replaced). */
async function ask(kind: "movie" | "show", source: string, ids: string[]) {
  const url = `https://api.mdblist.com/rating/${kind}/${source}?apikey=${encodeURIComponent(key!)}`;
  const started = performance.now();
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ provider: "imdb", ids }),
  });
  return {
    status: response.status,
    ms: Math.round(performance.now() - started),
    rateLimit: {
      limit: response.headers.get("x-ratelimit-limit"),
      remaining: response.headers.get("x-ratelimit-remaining"),
      reset: response.headers.get("x-ratelimit-reset"),
    },
    retryAfter: response.headers.get("retry-after"),
    body: hide(await response.text()),
  };
}

const answers: Record<string, unknown> = {};
for (const [kind, ids] of [["movie", IDS], ["show", SHOW]] as const) {
  for (const source of ["imdb", "tomatoes"]) {
    const answer = await ask(kind, source, ids);
    answers[`${kind}/${source}`] = answer;
    writeFileSync(join(fixtures, `rating-${kind}-${source}.json`), `${answer.body}\n`);
  }
}

// The core's batch size: the statuses, the scored count and the first error body only.
const hundred = await hundredIds();
const batch: Record<string, unknown> = { ids: hundred.length };
let firstError: string | null = null;
for (const source of ["imdb", "tomatoes"]) {
  const answer = await ask("movie", source, hundred);
  let scored: number | null = null;
  try {
    const items = (JSON.parse(answer.body) as { ratings?: { rating?: unknown }[] }).ratings;
    scored = items?.filter((r) => typeof r.rating === "number").length ?? null;
  } catch {
    // Not JSON: the body below says what it was.
  }
  if (answer.status !== 200 && firstError === null) firstError = answer.body.slice(0, 500);
  batch[source] = { status: answer.status, ms: answer.ms, rateLimit: answer.rateLimit, retryAfter: answer.retryAfter, scored };
}
batch.firstError = firstError;

const row = { date: new Date().toISOString().slice(0, 10), kind: "probe", milestone: "ratings", ids: IDS, show: SHOW, answers, batch };
appendFileSync(join(import.meta.dir, "ratings.jsonl"), `${JSON.stringify(row)}\n`);
console.log(JSON.stringify(row, null, 2));
