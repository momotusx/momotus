/** The live race of plans/m13-stream-choice.md 6.4 on the Mac: the core's
 * recommendation against Torrentio's first row, on legally free titles only,
 * through `mq play` (headless mpv with no audio device or video output, the
 * engine uploading nothing, a fresh data directory and torrent folder per run,
 * deleted after). Arms alternate A B B A per title (2 rounds).
 *
 * Run: bun bench/m13/race.ts <mq binary> [built_from]   appends to bench/m13/race.jsonl
 *      bun bench/m13/race.ts summary [race]              p50 and p90 first frame, success, stalls
 *
 * Race 2 (plans/m13-stream-choice.md 6.4) adds live seeders, the moov start
 * rule, the early cut, moving on and uTP: the recommended arm plays as the
 * apps play a pick, Torrentio's first as a plain client. Rows carry `race`;
 * race 1's rows have none. From race 3 the arms read Versions once the live
 * counts landed, not a fixed 2.5 s after `done`.
 */

import { appendFileSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const out = join(import.meta.dir, "race.jsonl");
const TITLES = [
  { id: "tt1727587", title: "Sintel" },
  { id: "tt1254207", title: "Big Buck Bunny" },
  { id: "tt2285752", title: "Tears of Steel" },
  { id: "tt4957236", title: "Cosmos Laundromat" },
];
// The Torrentio configuration Marquee ships (crates/mq-runtime/src/defaults.json).
const TORRENTIO =
  "https://torrentio.strem.fun/providers=yts,eztv,rarbg,1337x,ext,thepiratebay,kickasstorrents,torrentgalaxy,magnetdl,horriblesubs,nyaasi,tokyotosho,anidex,nekobt|qualityfilter=threed,scr,cam|sizefilter=15GB,5GB/manifest.json";
const ARMS = ["recommended", "first", "first", "recommended"] as const;
const SECONDS = 120;
const RACE = 3;

type Json = Record<string, any>;

function lines(text: string): Json[] {
  return text
    .split("\n")
    .filter((l) => l.startsWith("{"))
    .map((l) => {
      try {
        return JSON.parse(l);
      } catch {
        return {};
      }
    });
}

function race(mq: string, builtFrom: string) {
  let run = 0;
  for (const t of TITLES) {
    for (const [i, arm] of ARMS.entries()) {
      run++;
      const data = mkdtempSync(join(tmpdir(), "mq-race-"));
      const env = { ...process.env, MQ_DATA: data };
      for (const url of ["https://v3-cinemeta.strem.io/manifest.json", TORRENTIO]) {
        spawnSync(mq, ["install", url], { env, encoding: "utf8" });
      }
      const started = Date.now();
      const r = spawnSync(
        mq,
        ["play", "movie", t.id, "--headless", "--seconds", String(SECONDS), "--pick", arm, "--fresh-cache"],
        { env, encoding: "utf8", timeout: 480_000, maxBuffer: 16 << 20 },
      );
      rmSync(data, { recursive: true, force: true });
      const events = lines(`${r.stdout}\n${r.stderr}`);
      const result = events.find((e) => e.event === "race") ?? {};
      const failure = events.find((e) => e.event === "error")?.message ?? r.error?.message ?? null;
      const row = {
        race: RACE,
        date: new Date().toISOString().slice(0, 10),
        device: "mac",
        title: t.title,
        id: t.id,
        arm,
        round: i < 2 ? 1 : 2,
        run,
        key: result.key ?? null,
        release: result.release ?? null,
        sizeBytes: result.sizeBytes ?? null,
        seeders: result.seeders ?? null,
        bitrateMbps: result.bitrateMbps ?? null,
        line: result.line?.kind ?? null,
        readyMs: result.readyMs ?? null,
        firstFrameMs: result.firstFrameMs ?? null,
        stalls: result.stalls ?? null,
        stallMs: result.stallMs ?? null,
        attempts: result.attempts ?? null,
        failures: result.failures ?? null,
        played: result.played ?? null,
        moved: result.moved ?? null,
        versionsMs: result.versionsMs ?? null,
        countedMs: result.countedMs ?? null,
        nobodySharing: result.nobodySharing ?? null,
        ok: result.ok === true,
        error: result.ok ? null : failure,
        wallMs: Date.now() - started,
        built_from: builtFrom,
      };
      appendFileSync(out, JSON.stringify(row) + "\n");
      console.log(`${t.title} ${arm}: ok=${row.ok} first=${row.firstFrameMs} stalls=${row.stalls} ${row.release ?? row.error}`);
    }
  }
}

function pct(xs: number[], p: number): number | null {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.ceil((p / 100) * s.length) - 1)];
}

function summary(race: number) {
  const rows = readFileSync(out, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter((r) => (r.race ?? 1) === race);
  for (const arm of ["recommended", "first"]) {
    const mine = rows.filter((r) => r.arm === arm);
    const ok = mine.filter((r) => r.ok);
    const first = ok.map((r) => r.firstFrameMs as number);
    const playedMin = ok.length * (SECONDS / 60);
    const stalls = ok.reduce((n, r) => n + (r.stalls ?? 0), 0);
    console.log(
      JSON.stringify({
        race,
        arm,
        runs: mine.length,
        moved: mine.filter((r) => r.moved).length,
        success: mine.length ? Math.round((ok.length / mine.length) * 100) : null,
        p50FirstFrameMs: pct(first, 50),
        p90FirstFrameMs: pct(first, 90),
        stallsPer10Min: playedMin ? Math.round((stalls / playedMin) * 10 * 100) / 100 : null,
      }),
    );
  }
}

const [first, builtFrom] = process.argv.slice(2);
if (first === "summary") summary(Number(builtFrom ?? RACE));
else if (first) race(first, builtFrom ?? "unknown");
else console.error("usage: bun bench/m13/race.ts <mq binary> [built_from] | summary");
