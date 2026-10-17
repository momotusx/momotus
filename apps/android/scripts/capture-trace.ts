/** Records golden traces from a real core: connects to a host `mq-bridge` (real addons,
 * real network) and writes one JSONL file per view under core/src/test/resources/traces.
 * The Kotlin tests decode every line strictly, so a protocol change fails them.
 *
 * Needs: a running bridge (`MQ_DATA=<empty dir> MQ_BRIDGE_PORT=7790 mq-bridge`) and the local
 * test addon (`bun scripts/sintel-addon.ts`). Run: bun scripts/capture-trace.ts */

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const bridge = process.env.MQ_BRIDGE ?? "ws://127.0.0.1:7790/ws";
const addon = process.env.TEST_ADDON ?? "http://127.0.0.1:7780/manifest.json";
const outDir = join(import.meta.dir, "../core/src/test/resources/traces");

type Json = Record<string, any>;
interface Line {
  t: number;
  dir: "out" | "in";
  msg: Json;
}

const socket = new WebSocket(bridge);
await new Promise<void>((resolve, reject) => {
  socket.addEventListener("open", () => resolve());
  socket.addEventListener("error", () => reject(new Error(`cannot reach ${bridge}`)));
});

let lines: Line[] = [];
let started = performance.now();
const waiting = new Map<number, (m: Json) => void>();
let nextId = 1;

socket.addEventListener("message", (event) => {
  const msg = JSON.parse(String(event.data)) as Json;
  lines.push({ t: Math.round(performance.now() - started), dir: "in", msg });
  if (typeof msg.id === "number") waiting.get(msg.id)?.(msg);
});

function request(op: string, fields: Json = {}): Promise<Json> {
  const id = nextId++;
  const msg = { id, op, ...fields };
  lines.push({ t: Math.round(performance.now() - started), dir: "out", msg });
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${op} timed out`)), 60_000);
    waiting.set(id, (m) => {
      clearTimeout(timer);
      resolve(m);
    });
    socket.send(JSON.stringify(msg));
  });
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function open(view: string, params: Json = {}): Promise<number> {
  const reply = await request("subscribe", { view, params });
  if (!reply.ok) throw new Error(`${view}: ${reply.error}`);
  return reply.result.sub as number;
}

const command = (sub: number, c: Json) => request("command", { sub, command: c });

/** The loopback stream URL carries a per-session bearer token and a random port: keep the shape,
 * drop the values (and secret scanners stay quiet). */
const redact = (text: string) =>
  text.replace(/http:\/\/127\.0\.0\.1:\d+\/s\/[0-9a-f]{64}\//g, "http://127.0.0.1:PORT/s/TOKEN/");

function save(name: string) {
  mkdirSync(outDir, { recursive: true });
  const body = redact(lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
  writeFileSync(join(outDir, `${name}.jsonl`), body);
  console.log(`${name}: ${lines.length} lines, ${(body.length / 1024).toFixed(0)} KB`);
  lines = [];
  started = performance.now();
}

async function trace(name: string, body: () => Promise<void>) {
  lines = [];
  started = performance.now();
  await body();
  save(name);
}

// Refuse to record from a bridge that holds anyone's data: addon URLs can hold API keys and the
// library is personal. The bridge must start on an empty MQ_DATA directory.
{
  const addons = await open("addons");
  const library = await open("library");
  const seen = lines.filter((l) => l.dir === "in" && l.msg.result?.snapshot);
  const installed = seen.find((l) => l.msg.result.snapshot.addons)?.msg.result.snapshot.addons.length ?? -1;
  const items = seen.find((l) => l.msg.result.snapshot.items)?.msg.result.snapshot.items.length ?? -1;
  if (installed !== 0 || items !== 0) {
    throw new Error(`refusing to record: the bridge already has ${installed} addons and ${items} library items`);
  }
  await request("unsubscribe", { sub: addons });
  await request("unsubscribe", { sub: library });
}

// Addons first, recorded with the addons view.
await trace("addons", async () => {
  const cinemeta = await request("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
  if (!cinemeta.ok && !String(cinemeta.error).includes("already")) throw new Error(cinemeta.error);
  const local = await request("installAddon", { url: addon });
  if (!local.ok && !String(local.error).includes("already")) throw new Error(local.error);
  const sub = await open("addons");
  await sleep(300);
  await request("unsubscribe", { sub });
});

await trace("home", async () => {
  const sub = await open("home");
  await sleep(7000);
  await request("unsubscribe", { sub });
});

await trace("title-movie", async () => {
  const sub = await open("title", { kind: "movie", id: "tt1727587" });
  await sleep(3000);
  await command(sub, { type: "setInLibrary", value: true });
  await sleep(300);
  await command(sub, { type: "setInLibrary", value: false });
  await request("unsubscribe", { sub });
});

await trace("title-series", async () => {
  const sub = await open("title", { kind: "series", id: "tt0944947" });
  await sleep(3500);
  await command(sub, { type: "selectSeason", season: 2 });
  await sleep(800);
  await command(sub, { type: "setWatched", videoId: "tt0944947:2:1", value: true });
  await sleep(300);
  await command(sub, { type: "setWatched", videoId: "tt0944947:2:1", value: false });
  await request("unsubscribe", { sub });
});

await trace("discover", async () => {
  const sub = await open("discover");
  await sleep(2500);
  await command(sub, { type: "select", kind: "series" });
  await sleep(2500);
  await command(sub, { type: "loadMore" });
  await sleep(2000);
  await request("unsubscribe", { sub });
});

await trace("search", async () => {
  const sub = await open("search");
  await command(sub, { type: "setQuery", query: "sintel" });
  await sleep(4000);
  await command(sub, { type: "remember" });
  await request("unsubscribe", { sub });
});

await trace("library", async () => {
  const sub = await open("library");
  await sleep(300);
  await command(sub, { type: "setFilter", kind: "movie" });
  await command(sub, { type: "setSort", sort: "name" });
  await sleep(300);
  await request("unsubscribe", { sub });
});

await trace("settings", async () => {
  const sub = await open("settings");
  await request("setPrefs", { patch: { autoplayNext: true, streams: { languages: ["en"] } } });
  await sleep(300);
  await request("unsubscribe", { sub });
});

await trace("versions-playback", async () => {
  const versions = await open("versions", {
    kind: "movie",
    videoId: "tt1727587",
    title: "Sintel",
    displayHeight: 2160,
    hdr: ["hdr10"],
  });
  await sleep(6000);
  await command(versions, { type: "freeze" });
  // Versions is a record view: the pushes carry whole snapshots, the last one wins.
  const snapshots = lines
    .filter((l) => l.dir === "in" && l.msg.sub === versions)
    .flatMap((l) => l.msg.updates as Json[]);
  const key = (snapshots.findLast((s) => s.recommended)?.recommended.key ?? "") as string;
  if (!key) throw new Error("no recommended version; is the local test addon installed?");
  const playback = await open("playback", {
    versionsSub: versions,
    key,
    kind: "movie",
    itemId: "tt1727587",
    videoId: "tt1727587",
    name: "Sintel",
  });
  await sleep(8000);
  await command(playback, { type: "progress", positionMs: 4000, durationMs: 52000 });
  await request("unsubscribe", { sub: playback });
  await request("unsubscribe", { sub: versions });
});

socket.close();
