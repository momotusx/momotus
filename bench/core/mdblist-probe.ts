/** The first real MDBList call (plans/research/ratings.md, review P1-1): three ids (a popular
 * title, a new release, an id that does not exist) through `POST /rating/movie/{imdb,tomatoes}`,
 * the way the core asks. It appends one row with the raw answers to bench/core/ratings.jsonl and
 * writes them as fixtures for crates/mq-runtime/tests/ratings.rs. The key comes from
 * MARQUEE_MDBLIST_KEY or .env.mdblist.local; it is never printed or written, and any echo of it in
 * an answer is replaced before anything is kept.
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
const hide = (text: string) => text.split(key).join("<key>");
const fixtures = join(root, "crates/mq-runtime/tests/fixtures/mdblist");
mkdirSync(fixtures, { recursive: true });

const answers: Record<string, unknown> = {};
for (const source of ["imdb", "tomatoes"]) {
  const url = `https://api.mdblist.com/rating/movie/${source}?apikey=${encodeURIComponent(key)}`;
  const started = performance.now();
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ provider: "imdb", ids: IDS }),
  });
  const body = hide(await response.text());
  answers[source] = {
    status: response.status,
    ms: Math.round(performance.now() - started),
    rateLimit: {
      limit: response.headers.get("x-ratelimit-limit"),
      remaining: response.headers.get("x-ratelimit-remaining"),
      reset: response.headers.get("x-ratelimit-reset"),
    },
    retryAfter: response.headers.get("retry-after"),
    body,
  };
  writeFileSync(join(fixtures, `rating-movie-${source}.json`), `${body}\n`);
}

const row = { date: new Date().toISOString().slice(0, 10), kind: "probe", milestone: "ratings", ids: IDS, answers };
appendFileSync(join(import.meta.dir, "ratings.jsonl"), `${JSON.stringify(row)}\n`);
console.log(JSON.stringify(row, null, 2));
