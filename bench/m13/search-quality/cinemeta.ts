// Baseline: Cinemeta's own search (movie and series catalogs, as the core
// asks them), the rank of the wanted id in each. Usage: bun cinemeta.ts <out.jsonl>
import { queries } from "./queries.ts";
const rows = [];
for (const [kind, query, want] of queries) {
  const found: Record<string, number | null> = {};
  let worst = 0;
  for (const type of ["movie", "series"]) {
    const url = `https://v3-cinemeta.strem.io/catalog/${type}/top/search=${encodeURIComponent(query)}.json`;
    const r = Bun.spawnSync(["curl", "-s", "-m", "15", "-o", "/dev/stdout", "-w", "\n%{time_total}", url]);
    const text = r.stdout.toString();
    const secs = Number(text.trim().split("\n").pop());
    worst = Math.max(worst, Math.round(secs * 1000));
    let metas: { id: string }[] = [];
    try { metas = JSON.parse(text.slice(0, text.lastIndexOf("}") + 1)).metas ?? []; } catch {}
    const i = metas.findIndex((m) => want.includes(m.id));
    found[type] = i < 0 ? null : i + 1;
  }
  const rank = found.movie ?? found.series;
  rows.push({ kind, query, ms: worst, rank, found });
  await Bun.sleep(200);
}
await Bun.write(process.argv[2], rows.map((r) => JSON.stringify(r)).join("\n") + "\n");
const ms = rows.map((r) => r.ms).sort((a, b) => a - b);
const pct = (p: number) => ms[Math.ceil(p * ms.length) - 1];
console.log(`cinemeta (slower of the two catalogs): p50 ${pct(0.5)} ms, p90 ${pct(0.9)} ms; found ${rows.filter((r) => r.rank != null).length}/30, first ${rows.filter((r) => r.rank === 1).length}/30`);
for (const r of rows) console.log(`${r.kind.padEnd(11)} ${r.query.padEnd(36)} ${String(r.ms).padStart(5)} ms rank ${r.rank ?? "-"}`);
