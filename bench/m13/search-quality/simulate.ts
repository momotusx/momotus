// What the viewer would see first with the proposed design: a top row of
// the id source's exact and prefix title matches (movies and series), then
// Cinemeta's movie and series rows as the core shows them today (exact
// title first in each), each title once. Usage: bun simulate.ts
import { queries } from "./queries.ts";
const fold = (s: string) => s.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).join(" ");
type Hit = { id: string; name: string };
const curl = (url: string, headers: string[] = []) => {
  const r = Bun.spawnSync(["curl", "-s", "-m", "15", ...headers.flatMap((h) => ["-H", h]), url]);
  try { return JSON.parse(r.stdout.toString()); } catch { return null; }
};
const imdb = (q: string): Hit[] =>
  (curl(`https://v3.sg.media-imdb.com/suggestion/x/${encodeURIComponent(q.toLowerCase())}.json`)?.d ?? [])
    .filter((d: any) => d.id?.startsWith("tt") && ["movie", "short", "tvMovie", "video", "tvSpecial", "tvShort", "tvSeries", "tvMiniSeries"].includes(d.qid))
    .map((d: any) => ({ id: d.id, name: d.l }));
const key = process.env.MARQUEE_TRAKT_CLIENT_ID!;
const trakt = (q: string): Hit[] =>
  (curl(`https://api.trakt.tv/search/movie,show?query=${encodeURIComponent(q)}&limit=10`, ["trakt-api-version: 2", `trakt-api-key: ${key}`, "content-type: application/json"]) ?? [])
    .map((x: any) => x.movie ?? x.show).filter((m: any) => m?.ids?.imdb).map((m: any) => ({ id: m.ids.imdb, name: m.title }));
const cinemeta = (q: string, type: string): Hit[] => {
  const hits: Hit[] = (curl(`https://v3-cinemeta.strem.io/catalog/${type}/top/search=${encodeURIComponent(q)}.json`)?.metas ?? []).map((m: any) => ({ id: m.id, name: m.name }));
  return [...hits.filter((h) => fold(h.name) === fold(q)), ...hits.filter((h) => fold(h.name) !== fold(q))];
};
const top = (hits: Hit[], q: string) => {
  const w = fold(q);
  return [...hits.filter((h) => fold(h.name) === w), ...hits.filter((h) => fold(h.name) !== w && fold(h.name).startsWith(w))];
};
const tally: Record<string, number> = { today: 0, imdb: 0, trakt: 0 };
for (const [kind, q, want] of queries) {
  const rows = [...cinemeta(q, "movie"), ...cinemeta(q, "series")];
  const first = (lead: Hit[]) => {
    const seen = new Set<string>();
    return [...lead, ...rows].filter((h) => !seen.has(h.id) && seen.add(h.id))[0]?.id;
  };
  const ih = imdb(q), th = trakt(q);
  const lead = (hits: Hit[]) => { const m = top(hits, q); return m.length ? m : hits.slice(0, 1); };
  const today = first([]), i = first(top(ih, q)), t = first(top(th, q)), i2 = first(lead(ih)), t2 = first(lead(th));
  tally.imdb2 = (tally.imdb2 ?? 0) + +(!!i2 && want.includes(i2)); tally.trakt2 = (tally.trakt2 ?? 0) + +(!!t2 && want.includes(t2));
  const ok = (id?: string) => (id && want.includes(id) ? "yes" : "no ");
  tally.today += +(ok(today) === "yes"); tally.imdb += +(ok(i) === "yes"); tally.trakt += +(ok(t) === "yes");
  console.log(`${kind.padEnd(11)} ${q.padEnd(36)} today ${ok(today)} imdb ${ok(i)} trakt ${ok(t)}`);
  await Bun.sleep(200);
}
console.log(`first tile right: today ${tally.today}/30, IMDb name matches ${tally.imdb}/30, Trakt name matches ${tally.trakt}/30; plus the first hit when no name matches: IMDb ${tally.imdb2}/30, Trakt ${tally.trakt2}/30`);
