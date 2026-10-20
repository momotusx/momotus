// The same 30 queries against Trakt's text search (movies and shows, the
// app's client id from the environment, never printed). Usage: bun trakt.ts <out.jsonl>
import { queries } from "./queries.ts";
const id = process.env.MARQUEE_TRAKT_CLIENT_ID;
if (!id) throw new Error("no client id");
const rows = [];
for (const [kind, query, want] of queries) {
  const t0 = performance.now();
  const r = await fetch(`https://api.trakt.tv/search/movie,show?query=${encodeURIComponent(query)}&limit=10`, {
    headers: { "trakt-api-version": "2", "trakt-api-key": id, "content-type": "application/json" },
  });
  const body = await r.json().catch(() => []);
  const ms = Math.round(performance.now() - t0);
  const ids = (Array.isArray(body) ? body : []).map((x: any) => (x.movie ?? x.show)?.ids?.imdb);
  const rank = ids.findIndex((i: string) => want.includes(i));
  const top = (Array.isArray(body) ? body : []).slice(0, 1).map((x: any) => { const m = x.movie ?? x.show; return `${m?.title} (${m?.year}) ${m?.ids?.imdb}`; })[0] ?? "";
  rows.push({ kind, query, ms, status: r.status, rank: rank < 0 ? null : rank + 1, top, limit: r.headers.get("x-ratelimit") ?? "" });
  await Bun.sleep(300);
}
await Bun.write(process.argv[2], rows.map((r) => JSON.stringify(r)).join("\n") + "\n");
const ms = rows.map((r) => r.ms).sort((a, b) => a - b);
const pct = (p: number) => ms[Math.ceil(p * ms.length) - 1];
console.log(`trakt: p50 ${pct(0.5)} ms, p90 ${pct(0.9)} ms (first request opens the connection); found ${rows.filter((r) => r.rank != null).length}/30, first ${rows.filter((r) => r.rank === 1).length}/30`);
for (const r of rows) console.log(`${r.kind.padEnd(11)} ${r.query.padEnd(36)} ${String(r.ms).padStart(5)} ms ${r.status} rank ${r.rank ?? "-"} | ${r.top}`);
console.log("rate limit header:", rows[0].limit);
