/** The recorder behind MQ_DEV_REPLAY: every request a core makes in the e2e to
 * a host off this machine (Cinemeta, OpenSubtitles, Torrentio, their
 * subtitle files, any addon a test installs) comes here as
 * `/replay/<scheme>/<host><path>`, and so does every request the browser
 * makes off this machine (startBrowserRecorder). The first run asks the live
 * host once and keeps the answer in one cache per machine, shared by every
 * checkout and never committed (these are other services' data):
 * MQ_E2E_REPLAY_DIR, else ~/.cache/marquee-e2e/replay. Every later run is
 * served from there, so the whole e2e runs with the network off.
 *
 * Only answers below 500, and not 429, are kept, and a name that does not
 * resolve only when it cannot exist (a test's made-up addon): a service's bad
 * moment is never recorded, and the next run asks again.
 *
 * MQ_E2E_OFFLINE=1: never ask a live host; a missing answer is a 504 (an
 * image a grey placeholder), and the run lists every one at its end.
 * MQ_E2E_RECORD=1: ask every live host again and keep the new answers. */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { createServer as createSecureServer } from "node:https";
import { homedir } from "node:os";
import { join } from "node:path";

const ROOT = join(homedir(), ".cache", "marquee-e2e");
const DIR = process.env.MQ_E2E_REPLAY_DIR ?? join(ROOT, "replay");
const OFFLINE = process.env.MQ_E2E_OFFLINE === "1";
const RECORD = process.env.MQ_E2E_RECORD === "1";
/** Request headers that change an answer, passed to the live host. */
const PASSED = ["accept", "accept-language", "content-type", "range", "user-agent"];
/** Answer headers kept: the type, and CORS (a page reads a logo's pixels only when allowed). */
const KEPT = ["content-type", "access-control-allow-origin", "access-control-expose-headers"];
/** A one-pixel grey PNG: what an image missing from the recording shows offline. */
const GREY = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR4nGOwAgAAPAA7Rl0XFwAAAABJRU5ErkJggg==", "base64");

/** An answer as kept, or a host that cannot exist. */
type Kept = { url: string; status: number; headers: Record<string, string>; body: string } | { url: string; unknownHost: true };

/** Answers this run had to ask a live host for, and those it could not serve. */
export const replayed = { asked: 0, missed: new Set<string>() };

/** Whether an answer with `status` is kept: a server's failure or a refusal to answer now is not. */
export const keeps = (status: number) => status < 500 && status !== 429;

/** A name under a top-level domain reserved never to resolve (RFC 2606): a test's made-up host. */
export const madeUp = (host: string) => /(^|\.)(example|invalid|test)$/i.test(host.replace(/:\d+$/, ""));

/** The made-up Premiumize key the specs type (MQDUMMYE2EKEY1, and the same with a letter added). */
const DUMMY_KEY = /^MQDUMMYE2EKEY/;

/** What Premiumize answers for the specs' made-up key: an account that works and has nothing
 * cached. The live service refuses the key ("Not logged in"), and that refusal, once recorded,
 * would turn every later run's account to Sign in again, so the key never leaves this machine.
 * Any other call with it is not answered (a 504, a passing failure to the core). */
export function standIn(url: string, sent: Buffer | undefined): Kept | null {
  const asked = new URL(url);
  if (!/(^|\.)premiumize\.me$/.test(asked.hostname)) return null;
  const form = new URLSearchParams(sent?.toString() ?? "");
  if (!DUMMY_KEY.test(asked.searchParams.get("apikey") ?? form.get("apikey") ?? "")) return null;
  if (asked.pathname !== "/api/cache/check") return { url, status: 504, headers: {}, body: "" };
  const answer = { status: "success", response: asked.searchParams.getAll("items[]").map(() => false) };
  return { url, status: 200, headers: { "content-type": "application/json" }, body: Buffer.from(JSON.stringify(answer)).toString("base64") };
}

/** What an unrecorded answer becomes offline: a grey image when an image was asked for. */
export const placeholder = (accept: string | undefined): Buffer | null => (accept?.includes("image/") ? GREY : null);

async function body(req: IncomingMessage): Promise<Buffer | undefined> {
  if (req.method === "GET" || req.method === "HEAD") return undefined;
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks);
}

async function ask(url: string, req: IncomingMessage, sent: Buffer | undefined): Promise<Kept | null> {
  const headers: Record<string, string> = {};
  for (const name of PASSED) {
    const value = req.headers[name];
    if (typeof value === "string") headers[name] = value;
  }
  try {
    const res = await fetch(url, { method: req.method, headers, body: sent ? new Uint8Array(sent) : undefined, redirect: "follow", signal: AbortSignal.timeout(30_000) });
    const answer = Buffer.from(await res.arrayBuffer());
    const kept = Object.fromEntries(KEPT.flatMap((name) => (res.headers.get(name) === null ? [] : [[name, res.headers.get(name) as string]])));
    return { url, status: res.status, headers: kept, body: answer.toString("base64") };
  } catch (e) {
    // A name that cannot exist stays so; any other failure (a DNS hiccup, a
    // timeout, a machine offline while recording) is not an answer.
    const code = (e as { cause?: { code?: string } }).cause?.code;
    return code === "ENOTFOUND" && madeUp(new URL(url).host) ? { url, unknownHost: true } : null;
  }
}

/** Writes `kept` whole or not at all: checkouts running at once share the cache. */
function store(dir: string, file: string, kept: Kept) {
  mkdirSync(dir, { recursive: true });
  const part = `${file}.${process.pid}.part`;
  writeFileSync(part, JSON.stringify(kept));
  renameSync(part, file);
}

/** Serves `rest` (`<scheme>/<host><path>?<query>`) from the recording. */
export async function replay(req: IncomingMessage, res: ServerResponse, rest: string): Promise<void> {
  const slash = rest.indexOf("/");
  const url = `${rest.slice(0, slash)}://${rest.slice(slash + 1)}`;
  const host = new URL(url).host.replace(/[^\w.-]/g, "_");
  const sent = await body(req);
  const key = createHash("sha256").update(`${req.method} ${url} ${req.headers.range ?? ""} `).update(sent ?? Buffer.alloc(0)).digest("hex").slice(0, 40);
  const file = join(DIR, host, `${key}.json`);
  let kept: Kept | null = standIn(url, sent) ?? (!RECORD && existsSync(file) ? (JSON.parse(readFileSync(file, "utf8")) as Kept) : null);
  if (!kept && !OFFLINE) {
    replayed.asked++;
    kept = await ask(url, req, sent);
    if (kept && ("unknownHost" in kept || keeps(kept.status))) store(join(DIR, host), file, kept);
  }
  if (!kept) {
    if (OFFLINE) replayed.missed.add(`${req.method} ${url}`);
    const grey = OFFLINE ? placeholder(req.headers.accept) : null;
    res.writeHead(grey ? 200 : 504, { "content-type": grey ? "image/png" : "text/plain" });
    res.end(grey ?? "not recorded");
    return;
  }
  // As the core would see a name that does not resolve: no answer at all.
  if ("unknownHost" in kept) {
    req.socket.destroy();
    return;
  }
  res.writeHead(kept.status, kept.headers);
  res.end(Buffer.from(kept.body, "base64"));
}

/** A certificate made once per machine (~/.cache/marquee-e2e/tls): the
 * browser is told to ignore its errors, so any name may be served with it. */
function certificate(): { key: Buffer; cert: Buffer } {
  const dir = join(ROOT, "tls");
  const [key, cert] = [join(dir, "key.pem"), join(dir, "cert.pem")];
  if (!existsSync(cert)) {
    mkdirSync(dir, { recursive: true });
    execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-subj", "/CN=marquee-e2e", "-days", "3650"], { stdio: "ignore" });
  }
  return { key: readFileSync(key), cert: readFileSync(cert) };
}

/** The browser's side of the same recording: playwright.config maps every
 * name but this machine's to these two ports (443 to `https`, 80 to `http`),
 * so the images, logos and frames a page loads are recorded and served like
 * the cores' requests, and a run with the network off still shows them. */
export async function startBrowserRecorder(ports: { https: number; http: number }): Promise<Server[]> {
  const handle = (scheme: string) => (req: IncomingMessage, res: ServerResponse) => void replay(req, res, `${scheme}/${req.headers.host ?? ""}${req.url ?? "/"}`);
  const servers: Server[] = [createSecureServer(certificate(), handle("https")), createServer(handle("http"))];
  await Promise.all(servers.map((s, i) => new Promise<void>((ok) => s.listen(i ? ports.http : ports.https, "127.0.0.1", ok))));
  return servers;
}
