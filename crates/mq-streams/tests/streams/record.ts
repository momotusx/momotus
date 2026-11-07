// Records the stream-choice corpus (plans/m13-stream-choice.md 6.1): Torrentio's
// answers for 21 titles in two configurations (the one Marquee ships and
// Torrentio's default) and the Cinemeta facts the ranking needs. Each stream
// keeps only name, title, infoHash, fileIdx and behaviorHints, and every info hash
// is stored as its own SHA-1 (still unique, so ranking and dedupe behave the
// same, but no stored hash names a real torrent).
//
//   bun crates/mq-streams/tests/streams/record.ts             record now
//   bun crates/mq-streams/tests/streams/record.ts --from DIR  convert a recording
//     made earlier: DIR/corpus-shipped, DIR/corpus (default), DIR/meta, one
//     file per title as { id, kind, name, fetched, body }; a series' full
//     Cinemeta meta is read from DIR/meta/<show>.json, else fetched.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const OUT = import.meta.dir;
const TORRENTIO = "https://torrentio.strem.fun";
const CINEMETA = "https://v3-cinemeta.strem.io";
// The link Marquee ships (plans/m13-default-addons.md section 1).
const SHIPPED =
  "providers=yts,eztv,rarbg,1337x,ext,thepiratebay,kickasstorrents,torrentgalaxy,magnetdl,horriblesubs,nyaasi,tokyotosho,anidex,nekobt|qualityfilter=threed,scr,cam|sizefilter=15GB,5GB";

// The 21 titles of section 6.1, in its order; episodes as id:season:episode.
const TITLES = [
  "tt0047478", "tt0111161", "tt0317248", "tt0386676:2:1", "tt0388629:1:1", "tt0468569",
  "tt0816692", "tt0903747:5:14", "tt0944947:1:1", "tt11280740:2:1", "tt1375666", "tt15239678",
  "tt15398776", "tt1757678", "tt2758880", "tt31193180", "tt3581920:2:3", "tt4520988",
  "tt5491994:1:1", "tt6751668", "tt9253284:2:1",
];

type Stream = Record<string, unknown>;
type Answer = { id: string; kind: string; name: string; fetched: string; streams: Stream[] };

const kindOf = (id: string) => (id.includes(":") ? "series" : "movie");
const fileOf = (id: string) => `${id.replaceAll(":", "_")}.json`;

async function get(url: string): Promise<any> {
  const res = await fetch(url, { headers: { "user-agent": "Momotus corpus recorder" } });
  if (!res.ok) throw new Error(`${res.status} for ${url}`);
  return res.json();
}

/** Only what the ranking reads; no URL can carry a token. */
function strip(s: any): Stream {
  if (s.url || s.externalUrl) throw new Error(`a stream with a URL: ${s.name}`);
  const kept: Stream = { name: s.name, title: s.title, infoHash: s.infoHash };
  if (s.fileIdx !== undefined) kept.fileIdx = s.fileIdx;
  if (s.behaviorHints) kept.behaviorHints = s.behaviorHints;
  return kept;
}

async function answers(config: string | null, from: string | null): Promise<Answer[]> {
  const out: Answer[] = [];
  for (const id of TITLES) {
    if (from) {
      const r = JSON.parse(readFileSync(join(from, fileOf(id)), "utf8"));
      out.push({ id, kind: r.kind, name: r.name, fetched: r.fetched, streams: r.body.streams.map(strip) });
    } else {
      const prefix = config ? `${config}/` : "";
      const body = await get(`${TORRENTIO}/${prefix}stream/${kindOf(id)}/${id}.json`);
      const meta = await get(`${CINEMETA}/meta/${kindOf(id)}/${id.split(":")[0]}.json`);
      out.push({ id, kind: kindOf(id), name: meta.meta.name, fetched: new Date().toISOString(), streams: body.streams.map(strip) });
    }
  }
  return out;
}

/** A title's full Cinemeta meta: DIR/<show>.json when recorded, else fetched. */
async function fullMeta(id: string, from: string | null): Promise<any> {
  const show = id.split(":")[0];
  const local = from ? join(from, `${show}.json`) : null;
  const recorded = local && existsSync(local) ? JSON.parse(readFileSync(local, "utf8")) : null;
  return recorded?.meta ?? (await get(`${CINEMETA}/meta/${kindOf(id)}/${show}.json`)).meta;
}

/** The facts the ranking reads: years, runtime, the episode's date and its season's size. */
async function metas(from: string | null) {
  const out = [];
  for (const id of TITLES) {
    const [, season, episode] = id.split(":").map(Number);
    // A recording keeps these as a summary; recording now reads them from the meta.
    const summary = from ? JSON.parse(readFileSync(join(from, fileOf(id)), "utf8")) : null;
    const meta = summary && !season ? null : await fullMeta(id, from);
    const videos: any[] = meta?.videos ?? [];
    const ep = videos.find((v) => v.season === season && (v.episode ?? v.number) === episode);
    const from_ = summary ?? meta;
    out.push({
      id,
      name: from_.name,
      year: from_.year ?? null,
      releaseInfo: from_.releaseInfo ?? null,
      released: from_.released ?? null,
      runtime: from_.runtime ?? null,
      country: from_.country ?? null,
      episodeReleased: season ? (summary?.episodeReleased ?? ep?.released ?? ep?.firstAired ?? null) : null,
      episodesInSeason: season ? videos.filter((v) => v.season === season).length : null,
    });
  }
  return out;
}

/** One stream per line: small, and a changed row shows as one changed line. */
function write(file: string, head: Record<string, unknown>, titles: any[], rows: (t: any) => string) {
  const body = titles.map(rows).join(",\n");
  const lines = Object.entries(head).map(([k, v]) => `  ${JSON.stringify(k)}: ${JSON.stringify(v)},`);
  const text = `{\n${lines.join("\n")}\n  "titles": [\n${body}\n  ]\n}\n`;
  writeFileSync(join(OUT, file), text.replace(/\b[0-9a-fA-F]{40}\b/g, (h) => createHash("sha1").update(h.toLowerCase()).digest("hex")));
}

const fromIdx = process.argv.indexOf("--from");
const from = fromIdx > 0 ? process.argv[fromIdx + 1] : null;
const recorded = from
  ? JSON.parse(readFileSync(join(from, "corpus-shipped", fileOf(TITLES[0])), "utf8")).fetched.slice(0, 10)
  : new Date().toISOString().slice(0, 10);

for (const [file, config, dir, what] of [
  ["shipped.json", SHIPPED, "corpus-shipped", "the configuration Marquee ships"],
  ["default.json", null, "corpus", "Torrentio's default configuration"],
] as const) {
  const titles = await answers(config, from && join(from, dir));
  write(file, { recorded, source: `Torrentio stream answers, ${what}`, config }, titles, (t) => {
    const streams = t.streams.map((s: Stream) => `      ${JSON.stringify(s)}`).join(",\n");
    const head = JSON.stringify({ id: t.id, kind: t.kind, name: t.name, fetched: t.fetched });
    return `    ${head.slice(0, -1)}, "streams": [\n${streams}\n    ]}`;
  });
  console.log(`${file}: ${titles.length} titles, ${titles.reduce((n, t) => n + t.streams.length, 0)} streams`);
}

const meta = await metas(from && join(from, "meta"));
write("meta.json", { recorded, source: "Cinemeta meta, the facts the ranking reads" }, meta, (m) => `    ${JSON.stringify(m)}`);
console.log(`meta.json: ${meta.length} titles`);
