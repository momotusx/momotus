// A short burst of distinct queries (CDN misses), 10 at a time, to see
// whether the endpoint throttles. Usage: bun burst.ts <n>
const n = Number(process.argv[2] ?? 60);
const words = ["river", "night", "storm", "garden", "city", "ghost", "winter", "silver", "dragon", "ocean", "shadow", "fire"];
const qs = Array.from({ length: n }, (_, i) => `${words[i % words.length]} ${String.fromCharCode(97 + (i % 26))}${Math.floor(i / 26)}`);
const statuses: Record<string, number> = {};
const ms: number[] = [];
const t0 = performance.now();
for (let i = 0; i < qs.length; i += 10) {
  await Promise.all(qs.slice(i, i + 10).map(async (q) => {
    const s = performance.now();
    const r = await fetch(`https://v3.sg.media-imdb.com/suggestion/x/${encodeURIComponent(q)}.json`);
    await r.arrayBuffer();
    ms.push(performance.now() - s);
    const key = `${r.status} ${r.headers.get("x-cache") ?? ""}`;
    statuses[key] = (statuses[key] ?? 0) + 1;
  }));
}
ms.sort((a, b) => a - b);
console.log(`${n} requests in ${Math.round(performance.now() - t0)} ms, 10 at a time:`, statuses, `p50 ${Math.round(ms[Math.ceil(ms.length / 2) - 1])} ms p90 ${Math.round(ms[Math.ceil(ms.length * 0.9) - 1])} ms`);
