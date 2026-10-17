// Records real bridge traffic into Tests/Fixtures for the Swift decoding tests.
// Needs a running bridge over an empty data dir, for example:
//   MQ_DATA=$(mktemp -d) MQ_BRIDGE_PORT=7791 cargo run -p mq-bridge
// and the sample source (scripts/sample-source.ts) on port 7792, then:
//   bun apps/apple/scripts/capture-fixtures.ts ws://127.0.0.1:7791/ws
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const url = process.argv[2] ?? "ws://127.0.0.1:7791/ws";
const out = join(import.meta.dir, "../Tests/Fixtures");
const socket = new WebSocket(url);
const messages: any[] = [];
const waiters = new Map<number, (m: any) => void>();
let nextId = 1;

socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  messages.push(message);
  if ("id" in message) waiters.get(message.id)?.(message);
});
await new Promise((resolve) => socket.addEventListener("open", resolve));

function request(op: string, fields: object = {}): Promise<any> {
  const id = nextId++;
  return new Promise((resolve) => {
    waiters.set(id, resolve);
    socket.send(JSON.stringify({ id, op, ...fields }));
  });
}
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const pushesFor = (sub: number) => messages.filter((m) => m.sub === sub);
const save = (name: string, value: unknown) =>
  writeFileSync(join(out, name), JSON.stringify(value, null, 1) + "\n");

const empty = await request("subscribe", { view: "home", params: {} });
await request("unsubscribe", { sub: empty.result.sub });
const installed = await request("installAddon", { url: "https://v3-cinemeta.strem.io/manifest.json" });
if (!installed.ok) throw new Error(installed.error);

const home = await request("subscribe", { view: "home", params: {} });
await sleep(8000);
save("home-empty-reply.json", empty);
save("home-reply.json", home);
save("home-pushes.json", pushesFor(home.result.sub));

const title = await request("subscribe", { view: "title", params: { kind: "series", id: "tt0903747" } });
await sleep(5000);
const sub = title.result.sub;
const before = messages.length;
await request("command", { sub, command: { type: "selectSeason", season: 2 } });
await sleep(500);
const header = [...messages].reverse().find((m) => m.sub === sub && m.updates.some((u: any) => u.kind === "header"));
const firstEpisode = messages
  .slice(before)
  .flatMap((m) => (m.sub === sub ? m.updates : []))
  .flatMap((u: any) => (u.kind === "episodes" ? u.diffs : []))
  .flatMap((d: any) => d.values ?? (d.value ? [d.value] : []))[0];
await request("command", { sub, command: { type: "setWatched", videoId: firstEpisode.videoId, value: true } });
await request("command", { sub, command: { type: "setInLibrary", value: true } });
await sleep(500);
const bad = await request("command", { sub, command: { type: "nope" } });
save("title-reply.json", title);
save("title-pushes.json", pushesFor(sub));
save("command-error-reply.json", bad);
// Versions and playback for the sample source's Big Buck Bunny, with the
// capabilities the Apple app sends (AVPlayer: MP4 and MOV only).
await request("installAddon", { url: "http://127.0.0.1:7792/manifest.json" });
const versions = await request("subscribe", {
  view: "versions",
  params: {
    kind: "movie", videoId: "tt1254207", title: "Big Buck Bunny", displayHeight: 1206,
    hdr: ["hdr10", "hlg"], decoders: ["h264", "h265"], tenBitHevc: true,
    audioDecoders: ["aac", "ac3", "eac3", "atmos", "flac", "mp3", "pcm"],
    containers: ["mp4", "m4v", "mov"], hasFallbackEngine: false, language: "en",
  },
});
await sleep(4000);
const vsub = versions.result.sub;
const vpushes = pushesFor(vsub);
const latest = vpushes.at(-1)?.updates.at(-1) ?? versions.result.snapshot;
const playback = await request("subscribe", {
  view: "playback",
  params: { versionsSub: vsub, key: latest.recommended.key, kind: "movie", itemId: "tt1254207", videoId: "tt1254207", name: "Big Buck Bunny" },
});
await sleep(3000);
save("versions-reply.json", versions);
save("versions-pushes.json", vpushes);
save("playback-reply.json", playback);
save("playback-pushes.json", pushesFor(playback.result.sub));
// Record views: the snapshot on open and the last snapshot pushed after a command.
async function record(view: string, name: string, command?: object, wait = 3000) {
  const opened = await request("subscribe", { view, params: {} });
  if (command) await request("command", { sub: opened.result.sub, command });
  await sleep(wait);
  const last = pushesFor(opened.result.sub).at(-1)?.updates.at(-1) ?? opened.result.snapshot;
  save(`${name}.json`, last);
  await request("unsubscribe", { sub: opened.result.sub });
}
await record("search", "search", { type: "setQuery", query: "thrones" }, 6000);
await record("discover", "discover", { type: "select", kind: "series" }, 5000);
await record("library", "library");
await record("addons", "addons");
await record("settings", "settings");
await record("import", "import", undefined, 4000);
console.log(`home rows ${home.result.snapshot.rows.length}, title pushes ${pushesFor(sub).length}, header ${!!header}, versions pushes ${vpushes.length}`);
socket.close();
