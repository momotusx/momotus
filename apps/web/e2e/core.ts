/** Starts a fresh core for the e2e run and seeds it only through the bridge
 * protocol, the way a user would: install real addons, open a title, play a
 * version and report progress. */

import type { Page } from "@playwright/test";
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { BBB_ID } from "./source";

export const PORTS = { bridge: 17801, vite: 15301, source: 17802 };
/** The sync spec's own: a relay and one core per device, so the shared core never joins an account. */
export const SYNC_PORTS = { relay: 17803, a: 17804, b: 17805, c: 17806 };
export const GOT = { id: "tt0944947", resume: "tt0944947:2:3" };

const repo = resolve(fileURLToPath(new URL(".", import.meta.url)), "../../..");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(homedir(), ".cargo-targets/marquee-web");

/** A binary to test. MQ_BRIDGE_BIN (or MQ_SYNC_SERVER_BIN) names one
 * explicitly; otherwise this checkout's sources are always built first (a
 * no-op build takes about a second), so a stale binary is never what the
 * tests run against. */
function binary(pkg: "mq-bridge" | "mq-sync-server", env: string): string {
  const explicit = process.env[env];
  if (explicit) {
    if (!existsSync(explicit)) throw new Error(`${env} does not exist: ${explicit}`);
    return explicit;
  }
  const r = spawnSync("cargo", ["build", "-p", pkg, "-j", "4"], {
    cwd: repo,
    stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: targetDir, CARGO_INCREMENTAL: "0" },
  });
  if (r.status !== 0) throw new Error(`cargo build -p ${pkg} failed`);
  return join(targetDir, `debug/${pkg}`);
}

export const bridgeBinary = () => binary("mq-bridge", "MQ_BRIDGE_BIN");

export function startBridge(port = PORTS.bridge): Promise<{ proc: ChildProcess; data: string }> {
  const data = mkdtempSync(join(tmpdir(), "mq-e2e-"));
  const proc = spawn(bridgeBinary(), [], {
    env: { ...process.env, MQ_DATA: data, MQ_BRIDGE_PORT: String(port) },
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
