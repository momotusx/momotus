// Measures the IMDb suggestion endpoint: one fresh curl per query (a new
// connection each, as an app's first search), the rank of the wanted id among
// title results, and the CDN cache state. Usage: bun imdb.ts <out.jsonl>
import { queries } from "./queries.ts";
const out = process.argv[2];
const rows = [];
for (const [kind, query, want] of queries) {
  const url = `https://v3.sg.media-imdb.com/suggestion/x/${encodeURIComponent(query.toLowerCase())}.json`;
  const r = Bun.spawnSync(["curl", "-s", "-m", "10", "-D", "-", "-o", "/dev/stdout", "-w", "\n%{time_total} %{http_code}", url]);
  const text = r.stdout.toString();
  const tail = text.trim().split("\n").pop()!;
  const [secs, code] = tail.split(" ");
  const cache = /x-cache: ([^\r\n]+)/i.exec(text)?.[1] ?? "";
  const body = text.slice(text.indexOf("{"), text.lastIndexOf("}") + 1);
  let titles: { id: string; l: string; q?: string; y?: number }[] = [];
  try { titles = (JSON.parse(body).d ?? []).filter((d: any) => d.id?.startsWith("tt")); } catch {}
  const rank = titles.findIndex((t) => want.includes(t.id));
  rows.push({ kind, query, ms: Math.round(Number(secs) * 1000), status: Number(code), cache, rank: rank < 0 ? null : rank + 1, top: titles.slice(0, 3).map((t) => `${t.l} (${t.y ?? ""}, ${t.q ?? ""}) ${t.id}`) });
  await Bun.sleep(300);
}
await Bun.write(out, rows.map((r) => JSON.stringify(r)).join("\n") + "\n");
const ms = rows.map((r) => r.ms).sort((a, b) => a - b);
const pct = (p: number) => ms[Math.min(ms.length - 1, Math.ceil(p * ms.length) - 1)];
console.log(`p50 ${pct(0.5)} ms, p90 ${pct(0.9)} ms, max ${ms.at(-1)} ms; found ${rows.filter((r) => r.rank !== null).length}/30, first ${rows.filter((r) => r.rank === 1).length}/30`);
for (const r of rows) console.log(`${r.kind.padEnd(11)} ${r.query.padEnd(36)} ${String(r.ms).padStart(5)} ms ${r.status} rank ${r.rank ?? "-"} | ${r.cache} | ${r.top[0] ?? ""}`);
