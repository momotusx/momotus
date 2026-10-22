/** Starts a fresh core for the e2e run and seeds it only through the bridge
 * protocol, the way a user would: install real addons, open a title, play a
 * version and report progress. */

import type { Page } from "@playwright/test";
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { BBB_ID } from "./source";

/** MQ_E2E_PORT_OFFSET moves every port, so two checkouts can run their
 * suites at once (each run needs its own core and dev server). */
const shift = Number(process.env.MQ_E2E_PORT_OFFSET) || 0;
export const PORTS = { bridge: 17801 + shift, vite: 15301 + shift, source: 17802 + shift };
/** The sync spec's own: a relay and one core per device, so the shared core never joins an account. */
export const SYNC_PORTS = { relay: 17803 + shift, a: 17804 + shift, b: 17805 + shift, c: 17806 + shift };
/** The Trakt spec's own: a core whose Trakt is the fake, the fake, and a core built without a Trakt app. */
export const TRAKT_PORTS = { bridge: 17807 + shift, fake: 17808 + shift, plain: 17809 + shift };
/** The default addons spec's own: a fresh core per walk, so the shared one never gets the defaults. */
export const DEFAULTS_PORT = 17810 + shift;
/** The subtitle notes spec's own: a core with one addon whose subtitle files fail. */
export const NOTES_PORT = 17811 + shift;
/** The subtitle languages spec's own: a core whose stream names subtitles in many languages. */
export const LANGS_PORT = 17812 + shift;
/** The next version spec's own: a core whose first versions cannot play here. */
export const NEXT_PORT = 17813 + shift;
/** The journey API spec's own: a core with a local addon's series, one episode not out yet. */
export const JOURNEY_PORT = 17814 + shift;
/** The Search Top results spec's own: a core whose Trakt is a local fake. */
export const SEARCH_TOP_PORT = 17815 + shift;
/** The browser's recorder (e2e/replay.ts): every name but this machine's resolves here. */
export const BROWSER_PORTS = { https: 17816 + shift, http: 17817 + shift };
/** The accounts spec's own: a relay with sign-in, the mock provider, and one core per device. */
export const ACCOUNTS_PORTS = { relay: 17818 + shift, mock: 17819 + shift, a: 17820 + shift, b: 17821 + shift, c: 17822 + shift };
/** The ratings spec's own: a core whose MDBList is a local fake (MQ_DEV_MDBLIST). */
export const RATINGS_PORT = 17823 + shift;
/** The player loading spec's own: a core whose one source holds its first byte until the test lets it go. */
export const LOADING_PORT = 17824 + shift;
/** The addon names spec's own: a core with two local addons whose catalogs share a title. */
export const NAMES_PORT = 17825 + shift;
/** Every browser a run launches takes these: each name but this machine's resolves to the browser's
 * recorder, whose certificate it does not check, so pages load images and frames from the
 * recording and a run needs the network only to record (none with MQ_E2E_OFFLINE=1). A spec that
 * sets its own launch arguments adds these to them. */
export const BROWSER_ARGS = [
  `--host-resolver-rules=MAP *:443 127.0.0.1:${BROWSER_PORTS.https}, MAP *:80 127.0.0.1:${BROWSER_PORTS.http}, EXCLUDE localhost, EXCLUDE 127.0.0.1`,
  "--ignore-certificate-errors",
];
export const GOT = { id: "tt0944947", resume: "tt0944947:2:3" };
/** Where every spec writes its shots: SHOTS_DIR, else the run's own ignored
 * output. The committed set in e2e/shots/ changes only when a run is asked
 * to (SHOTS_DIR=e2e/shots), so a plain run leaves the tree clean. */
export const SHOTS = process.env.SHOTS_DIR ?? "test-results/shots";

const repo = resolve(fileURLToPath(new URL(".", import.meta.url)), "../../..");
// This checkout's own target dir unless CARGO_TARGET_DIR names one: cargo
// gives workspace crates the same artifact names in every checkout, so a
// shared one would run another checkout's build (the repository rules). The
// name says which checkout; the hash keeps two of the same name apart.
const checkout = `${basename(repo)}-${createHash("sha256").update(repo).digest("hex").slice(0, 8)}`;
const targetDir = process.env.CARGO_TARGET_DIR ?? join(homedir(), `.cargo-targets/marquee-e2e-${checkout}`);

/** A binary to test. MQ_BRIDGE_BIN (or MQ_SYNC_SERVER_BIN) names one
 * explicitly; otherwise this checkout's sources are always built first (a
 * no-op build takes about a second), so a stale binary is never what the
 * tests run against. The bridge is built without its `torrent` feature (the
 * dev bridge has it): a run never touches BitTorrent. */
function binary(pkg: "mq-bridge" | "mq-sync-server", env: string, example?: string): string {
  const explicit = process.env[env];
  if (explicit) {
    if (!existsSync(explicit)) throw new Error(`${env} does not exist: ${explicit}`);
    return explicit;
  }
  const r = spawnSync("cargo", ["build", "-p", pkg, ...(example ? ["--example", example] : []), "-j", "4"], {
    cwd: repo,
    stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: targetDir, CARGO_INCREMENTAL: "0" },
  });
  if (r.status !== 0) throw new Error(`cargo build -p ${pkg} failed`);
  return join(targetDir, example ? `debug/examples/${example}` : `debug/${pkg}`);
}

/** The mock OpenID provider the relay's tests use (crates/mq-sync-server/examples/mock-oidc.rs):
 * a subject picker, Cancel and Stall, on loopback. Answers its URL once it listens. */
export function startMockProvider(port: number): Promise<{ proc: ChildProcess; url: string }> {
  const proc = spawn(binary("mq-sync-server", "MQ_MOCK_OIDC_BIN", "mock-oidc"), [String(port)], { stdio: ["ignore", "pipe", "inherit"] });
  return new Promise((ok, fail) => {
    const timer = setTimeout(() => fail(new Error("no mock_oidc_ready in 30 s")), 30_000);
    proc.stdout?.on("data", (chunk: Buffer) => {
      const ready = /"url":"([^"]+)"/.exec(chunk.toString());
      if (ready?.[1]) {
        clearTimeout(timer);
        ok({ proc, url: ready[1] });
      }
    });
    proc.on("exit", (code) => fail(new Error(`mock provider exited with ${code}`)));
  });
}

export const bridgeBinary = () => binary("mq-bridge", "MQ_BRIDGE_BIN");

/** With the network off (MQ_E2E_OFFLINE=1), a request that slipped past the recorder goes to a
 * proxy that is not there and fails, instead of reaching the internet. */
const NO_NETWORK = { HTTP_PROXY: "http://127.0.0.1:9", HTTPS_PROXY: "http://127.0.0.1:9", ALL_PROXY: "http://127.0.0.1:9", NO_PROXY: "127.0.0.1,localhost" };

/** A core on `port` with a data directory of its own; `env` adds variables such as MQ_DEV_TRAKT. Skip
 * segments come from the source server's synthetic answers, and every request to a host off this
 * machine from its recorder (e2e/replay.ts), never a live service once recorded. */
export function startBridge(port = PORTS.bridge, env: Record<string, string> = {}): Promise<{ proc: ChildProcess; data: string }> {
  const data = mkdtempSync(join(tmpdir(), "mq-e2e-"));
  const local = `http://127.0.0.1:${PORTS.source}`;
  const offline = process.env.MQ_E2E_OFFLINE === "1" ? NO_NETWORK : {};
  const hooks = { MQ_DEV_SEGMENTS: `${local}/segments`, MQ_DEV_REPLAY: `${local}/replay`, ...offline };
  const proc = spawn(bridgeBinary(), [], {
    env: { ...process.env, ...hooks, ...env, MQ_DATA: data, MQ_BRIDGE_PORT: String(port) },
    stdio: ["ignore", "pipe", "inherit"],
  });
  return ready(proc, "bridge_ready", data);
}

/** A sync server (crates/mq-sync-server) on loopback, where plain http is
 * allowed, with a database of its own in `data` (`relay.sqlite` and its
 * roster journal `relay.roster.jsonl`). `args` adds flags such as
 * `--new-generation` for a restore. */
export function startRelay(port: number, data = mkdtempSync(join(tmpdir(), "mq-relay-")), args: string[] = []): Promise<{ proc: ChildProcess; data: string }> {
  const proc = spawn(binary("mq-sync-server", "MQ_SYNC_SERVER_BIN"), ["--db", join(data, "relay.sqlite"), "--listen", `127.0.0.1:${port}`, ...args], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  return ready(proc, "sync_server_ready", data);
}

function ready(proc: ChildProcess, word: string, data: string): Promise<{ proc: ChildProcess; data: string }> {
  return new Promise((ok, fail) => {
    const timer = setTimeout(() => fail(new Error(`no ${word} in 30 s`)), 30_000);
    proc.stdout?.on("data", (chunk: Buffer) => {
      if (chunk.toString().includes(word)) {
        clearTimeout(timer);
        ok({ proc, data });
      }
    });
    proc.on("exit", (code) => fail(new Error(`${word.split("_")[0]} exited with ${code}`)));
  });
}

export function cleanup(proc: ChildProcess, data: string) {
  proc.kill();
  rmSync(data, { recursive: true, force: true });
}

/** A minimal protocol client (see plans/core-protocol.md). */
export class Client {
  private ws: WebSocket;
  private id = 0;
  private waiters = new Map<number, (m: { ok: boolean; result?: unknown; error?: string }) => void>();
  private last = new Map<number, unknown>();
  // Home and Title push typed updates; their latest header is kept here.
  private headers = new Map<number, unknown>();
  ready: Promise<void>;

  constructor(url = `ws://127.0.0.1:${PORTS.bridge}/ws`) {
    this.ws = new WebSocket(url);
    this.ready = new Promise((ok) => this.ws.addEventListener("open", () => ok()));
    this.ws.addEventListener("message", (e) => {
      const m = JSON.parse(String(e.data));
      if ("sub" in m && "updates" in m) {
        this.last.set(m.sub, m.updates[m.updates.length - 1]);
        for (const u of m.updates) if (u && typeof u === "object" && "header" in u) this.headers.set(m.sub, u.header);
      }
      else this.waiters.get(m.id)?.(m);
    });
  }

  async req<T>(op: string, fields: Record<string, unknown> = {}): Promise<T> {
    await this.ready;
    const id = ++this.id;
    const reply = await new Promise<{ ok: boolean; result?: unknown; error?: string }>((ok) => {
      this.waiters.set(id, ok);
      this.ws.send(JSON.stringify({ id, op, ...fields }));
    });
    if (!reply.ok) throw new Error(`${op}: ${reply.error}`);
    return reply.result as T;
  }

  async subscribe<S>(view: string, params: Record<string, unknown> = {}): Promise<{ sub: number; now: () => S }> {
    const { sub, snapshot } = await this.req<{ sub: number; snapshot: S }>("subscribe", { view, params });
    return { sub, now: () => (this.last.get(sub) as S | undefined) ?? snapshot };
  }

  header<H>(sub: number): H | undefined {
    return this.headers.get(sub) as H | undefined;
  }

  command(sub: number, command: Record<string, unknown>) {
    return this.req("command", { sub, command });
  }

  close() {
    this.ws.close();
  }
}

export async function until<T>(what: string, probe: () => T | null | undefined | false, ms = 30_000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const v = probe();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

/** Browsers that block autoplay leave the player paused on Play; a press of
 * the focused button is the gesture they need. Where autoplay is allowed it
 * is already playing, and a press would pause it. */
export async function startIfPaused(page: Page) {
  await page.waitForFunction(() => {
    const v = document.querySelector("video");
    return !!v && v.readyState >= 1;
  });
  await page.waitForTimeout(500);
  const toggle = page.locator('[data-f="player:toggle"]');
  if ((await toggle.getAttribute("aria-label")) === "Play") await toggle.press("Enter");
}

type Row = { key: string; via: string };
type Versions = { recommended: Row | null; rows: Row[]; done: boolean };

/** Plays the pick for a video long enough to record progress, like a viewer
 * who watched `positionMs` and left. */
export async function watch(
  c: Client,
  p: { kind: string; itemId: string; videoId: string; title: string; season?: number; episode?: number; via?: string },
  positionMs: number,
  durationMs: number,
) {
  const versions = await c.subscribe<Versions>("versions", {
    kind: p.kind,
    videoId: p.videoId,
    title: p.title,
    season: p.season,
    episode: p.episode,
  });
  const key = await until(`versions for ${p.title}`, () => {
    const v = versions.now();
    if (p.via) return [v.recommended, ...v.rows].find((r) => r?.via === p.via)?.key ?? null;
    return v.recommended?.key ?? null;
  });
  const playback = await c.subscribe("playback", {
    versionsSub: versions.sub,
    key,
    kind: p.kind,
    itemId: p.itemId,
    videoId: p.videoId,
    name: p.title,
    season: p.season,
    episode: p.episode,
  });
  await c.command(playback.sub, { type: "progress", positionMs, durationMs });
  await c.req("unsubscribe", { sub: playback.sub });
  await c.req("unsubscribe", { sub: versions.sub });
}

/** Puts Big Buck Bunny back where the seed left it (2:30, unwatched) after a
 * test played it to the end. The player's page goes first: closing its
 * playback view saves the end position, which would land after the reseed.
 * Finishing marks the film watched, so that mark is cleared before the
 * position is written again. */
export async function reseedBbb(page: Page) {
  await page.goto("about:blank");
  // The bridge drops the page's session when its socket closes; give it a
  // moment so the view's last save is behind us.
  await new Promise((r) => setTimeout(r, 1000));
  const c = new Client();
  try {
    type Header = { state: { state: string } };
    const title = await c.subscribe<{ header?: Header }>("title", { kind: "movie", id: BBB_ID });
    await until("Big Buck Bunny details", () => (c.header<Header>(title.sub) ?? title.now().header)?.state.state === "ready");
    await c.command(title.sub, { type: "setWatched", videoId: BBB_ID, value: false });
    await c.req("unsubscribe", { sub: title.sub });
    await watch(c, { kind: "movie", itemId: BBB_ID, videoId: BBB_ID, title: "Big Buck Bunny", via: "Sample Source" }, 150_000, 596_000);
  } finally {
    c.close();
  }
}
