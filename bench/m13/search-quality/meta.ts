// Cinemeta meta for each wanted id (the type from the IMDb answer), as the
// top row would resolve it: time and whether a meta came back.
import { queries } from "./queries.ts";
const rows = (await Bun.file(process.argv[2]).text()).trim().split("\n").map((l) => JSON.parse(l));
const ms: number[] = []; let ok = 0;
for (const [kind, q, want] of queries) {
  const r = rows.find((x) => x.query === q);
  const series = /TV series|TV mini/.test(r?.top?.[0] ?? "") || ["Severance", "Breaking Bad", "The Office", "breaking b", "game of thr", "succes", "La casa de papel", "Attack on Titan", "Shingeki no Kyojin", "Frieren", "brekaing bad"].includes(q);
  const out = Bun.spawnSync(["curl", "-sL", "-m", "15", "-o", "/dev/stdout", "-w", "\n%{time_total}", `https://v3-cinemeta.strem.io/meta/${series ? "series" : "movie"}/${want[0]}.json`]).stdout.toString();
  const t = Number(out.trim().split("\n").pop());
  const has = out.includes('"meta":{');
  ms.push(Math.round(t * 1000)); ok += +has;
  await Bun.sleep(150);
}
ms.sort((a, b) => a - b);
console.log(`cinemeta meta: ${ok}/30 answered; p50 ${ms[14]} ms, p90 ${ms[26]} ms, max ${ms[29]} ms`);
