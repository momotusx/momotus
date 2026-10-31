/** Multi-device sync through the web screens, for real: a local sync server
 * (crates/mq-sync-server) and three cores of their own, one per device, each
 * page reaching its core through a routed WebSocket. The shared core of the
 * other specs never joins an account. Kits read and shown here belong to this
 * run's throwaway server and are gone when the run ends.
 *
 * Desk (desktop, on localhost with a virtual passkey and a fake camera)
 * starts the account and remembers it; Phone pairs by a scanned code and
 * makes a kit; Living room TV joins with that kit; Desk reports Phone lost
 * and keeps the TV; Phone pairs again (once removed for digits that differ,
 * once rejected); a restored sync server shows Desk a different list, which
 * Desk compares before continuing; Desk removes the TV and stops syncing; the
 * TV hears its account was deleted, then deletes one of its own.
 * Every secret that crossed a socket is then looked for in storage, history,
 * the URL and the console. */

import { type Browser, type CDPSession, chromium, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { once } from "node:events";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { encode } from "uqr";
import { BROWSER_ARGS, Client, PORTS, SHOTS, startBridge, startRelay, SYNC_PORTS } from "./core";

const SERVER = `http://127.0.0.1:${SYNC_PORTS.relay}`;
const RELAY_FILES = ["relay.sqlite", "relay.sqlite-wal", "relay.sqlite-shm", "relay.roster.jsonl"];
const bridges: { proc: ChildProcess; data: string }[] = [];
let relay: { proc: ChildProcess; data: string };
let desk: Page;
let phone: Page;
let tv: Page;
let passkey: { cdp: CDPSession; id: string };
let kit = "";
const errors: string[] = [];
const consoleLines: string[] = [];
/** Every secret that crossed a socket: record fields and kits. */
const secrets = new Set<string>();
/** The next request of this op from the page on a core's port is answered as a relay failure. */
const failOnce = new Map<number, { op: string; code?: string }>();
/** Every request a page sent its core, by the core's port: op and view, nothing else. */
const asked: { port: number; op?: string; view?: string }[] = [];
/** The next answer to this op on a core's port is edited before the page sees it. */
const editOnce = new Map<number, { op: string; edit: (result: Record<string, unknown>) => void }>();
/** Desk's fake camera films this file (Chromium reads it when the camera opens). */
let camera = "";
let scratch = "";
let deskBrowser: Browser;

/** A Y4M video of `text` as a QR code, black on white, for the fake camera. */
function film(text: string) {
  const [width, height] = [640, 480];
  const qr = encode(text, { ecc: "M", border: 4 });
  const px = Math.floor(440 / qr.size);
  const [x0, y0] = [(width - qr.size * px) / 2, (height - qr.size * px) / 2].map(Math.floor) as [number, number];
  const y = Buffer.alloc(width * height, 255);
  for (let r = 0; r < qr.size * px; r++)
    for (let c = 0; c < qr.size * px; c++) if (qr.data[Math.floor(r / px)]?.[Math.floor(c / px)]) y[(y0 + r) * width + x0 + c] = 0;
  const chroma = Buffer.alloc((width / 2) * (height / 2) * 2, 128);
  const frame = Buffer.concat([Buffer.from("FRAME\n"), y, chroma]);
  writeFileSync(camera, Buffer.concat([Buffer.from(`YUV4MPEG2 W${width} H${height} F10:1 Ip A1:1 C420jpeg\n`), frame, frame]));
}

/** Collects the secrets in one protocol message: every long string under an
 * object that carries a device token (the sync record), and every kit. */
function collect(text: string) {
  let message: unknown;
  try {
    message = JSON.parse(text);
  } catch {
    return;
  }
  const leaves = (v: unknown) => {
    if (typeof v === "string") {
      if (v.length >= 16 && !v.startsWith("http")) secrets.add(v);
    } else if (v && typeof v === "object") for (const x of Object.values(v)) leaves(x);
  };
  const walk = (v: unknown) => {
    if (!v || typeof v !== "object") return;
    const o = v as Record<string, unknown>;
    if ("deviceToken" in o) leaves(o);
    if (typeof o.kit === "string" && /^([0-9A-Za-z]{4}-?){8}$/.test(o.kit)) secrets.add(o.kit.toUpperCase());
    for (const x of Object.values(o)) walk(x);
  };
  walk(message);
}

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
};

/** On a load the test marks (a session flag), IndexedDB answers 5 s late: the check for a remembered account is slow.
 * Other marks make the first open fail, or every open on every load until the mark is removed. */
function slowVaultOnMark() {
  const failing = (ms = 0) => {
    const failed = { onsuccess: null, onerror: null, onupgradeneeded: null, result: null, error: new DOMException("unreadable", "UnknownError") } as unknown as IDBOpenDBRequest & Record<string, unknown>;
    setTimeout(() => (failed.onerror as ((e: Event) => void) | null)?.(new Event("error")), ms);
    return failed;
  };
  if (sessionStorage.getItem("mq.e2e.noIdb") === "1") {
    // Each open fails after 1.5 s while the mark is there; without it the storage works again.
    const open = IDBFactory.prototype.open;
    IDBFactory.prototype.open = function (this: IDBFactory, ...args: Parameters<typeof open>) {
      return sessionStorage.getItem("mq.e2e.noIdb") === "1" ? failing(1500) : open.apply(this, args);
    };
    return;
  }
  if (sessionStorage.getItem("mq.e2e.failIdb") === "1") {
    sessionStorage.removeItem("mq.e2e.failIdb");
    const open = IDBFactory.prototype.open;
    IDBFactory.prototype.open = function () {
      // Only the first open fails; the next one works again.
      IDBFactory.prototype.open = open;
      return failing();
    };
    return;
  }
  if (sessionStorage.getItem("mq.e2e.slowIdb") !== "1") return;
  sessionStorage.removeItem("mq.e2e.slowIdb");
  const real = IDBFactory.prototype.open;
  IDBFactory.prototype.open = function (this: IDBFactory, ...args: Parameters<typeof real>) {
    const late = { onsuccess: null, onerror: null, onupgradeneeded: null, result: null, error: null } as unknown as IDBOpenDBRequest & Record<string, unknown>;
    setTimeout(() => {
      const r = real.apply(this, args);
      r.onupgradeneeded = (e) => {
        (late as Record<string, unknown>).result = r.result;
        (late.onupgradeneeded as ((e: Event) => void) | null)?.(e);
      };
      r.onsuccess = (e) => {
        (late as Record<string, unknown>).result = r.result;
        (late.onsuccess as ((e: Event) => void) | null)?.(e);
      };
      r.onerror = (e) => {
        (late as Record<string, unknown>).error = r.error;
        (late.onerror as ((e: Event) => void) | null)?.(e);
      };
    }, 5000);
    return late;
  };
}

/** A browser context whose page talks to the core on `port`. */
async function device(browser: Browser, port: number, form: keyof typeof FORMS, host = "127.0.0.1"): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://${host}:${PORTS.vite}` });
  await context.grantPermissions(["clipboard-read", "clipboard-write", "camera"]);
  await context.addInitScript(slowVaultOnMark);
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    const ops = new Map<number, string>();
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => {
      collect(String(e.data));
      const reply = JSON.parse(String(e.data)) as { id?: number; result?: Record<string, unknown> };
      const op = reply.id === undefined ? undefined : ops.get(reply.id);
      const edit = editOnce.get(port);
      if (op && edit && edit.op === op && reply.result) {
        editOnce.delete(port);
        edit.edit(reply.result);
        ws.send(JSON.stringify(reply));
        return;
      }
      ws.send(String(e.data));
    });
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      collect(String(m));
      const request = JSON.parse(String(m)) as { id?: number; op?: string; view?: string };
      asked.push({ port, op: request.op, view: request.view });
      if (request.id !== undefined && request.op) ops.set(request.id, request.op);
      const fail = failOnce.get(port);
      if (request.op && request.op === fail?.op) {
        failOnce.delete(port);
        ws.send(JSON.stringify({ id: request.id, ok: false, error: fail.code ? "the relay shows a roster that conflicts" : "http status 502", kind: fail.code ? "failed" : "serverError", code: fail.code }));
        return;
      }
      if (core.readyState === WebSocket.OPEN) core.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  page.on("console", (m) => consoleLines.push(m.text()));
  await page.goto(`/settings?form=${form}`);
  return page;
}

const card = (page: Page) => page.locator(".sync-card");
const state = (page: Page) => page.locator(".sync-state .st").first();
/** The first round after joining has ended. */
async function synced(page: Page) {
  await expect(card(page)).toBeVisible();
  await expect(card(page)).not.toHaveAttribute("data-syncing", "", { timeout: 30_000 });
  await expect(state(page)).toContainText("Synced at");
}
/** Presses Sync now and waits for that round, not an earlier one, to end. */
async function syncNow(page: Page, remote = false) {
  const before = Number(await card(page).getAttribute("data-round"));
  if (remote) {
    await page.locator('[data-f="sync:now"]').focus();
    await page.keyboard.press("Enter");
  } else await page.locator('[data-f="sync:now"]').click();
  await expect.poll(async () => Number((await card(page).getAttribute("data-round")) ?? "Infinity"), { timeout: 30_000 }).toBeGreaterThan(before);
  // A round already running when the button was pressed ends first; the loop then runs the pressed one.
  await expect(card(page).or(page.locator("main > .st.bad")).first()).not.toHaveAttribute("data-syncing", "", { timeout: 30_000 });
}
const member = (page: Page, name: string) => page.locator(".sync-member").filter({ has: page.getByRole("heading", { name, exact: false }) });
const names = (page: Page) => page.locator(".sync-members h3").evaluateAll((hs) => hs.map((h) => h.firstChild?.textContent ?? ""));
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
const shot = async (page: Page, name: string) => {
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${SHOTS}/${name}.jpg`, type: "jpeg", quality: 80 });
};
/** In-app navigation: a reload would drop the tab's sync record, as it does for a user. */
const go = async (page: Page, path: string) => {
  await page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
  await expect(page).toHaveURL(new RegExp(`${path.replace(/[/?]/g, "\\$&")}$`));
};
/** IndexedDB `method` calls on `page` keep their transaction busy for `ms` (or until `release()`),
 * then abort it, or with `commit` let it commit. `release()` also puts the real method back. */
async function hold(page: Page, method: "put" | "delete", ms?: number, commit = false) {
  await page.evaluate(
    ({ method, ms, commit }) => {
      const real = IDBObjectStore.prototype[method] as (this: IDBObjectStore, ...a: unknown[]) => IDBRequest;
      let released = false;
      // Calls that ran their course (aborted or let commit), for a test to wait on.
      const w = window as unknown as { held: number };
      w.held = 0;
      (window as unknown as { release: () => void }).release = () => {
        released = true;
        IDBObjectStore.prototype[method] = real as never;
      };
      IDBObjectStore.prototype[method] = function (this: IDBObjectStore, ...args: unknown[]) {
        const store = this;
        const request = real.apply(this, args);
        const until = ms === undefined ? Infinity : Date.now() + ms;
        const spin = () => {
          if (!released && Date.now() < until) store.count().onsuccess = spin;
          else {
            if (!commit) store.transaction.abort();
            w.held++;
          }
        };
        request.addEventListener("success", spin);
        return request;
      } as never;
    },
    { method, ms, commit },
  );
}
const release = (page: Page) => page.evaluate(() => (window as unknown as { release: () => void }).release());

const signCount = async () => {
  const { credentials } = (await passkey.cdp.send("WebAuthn.getCredentials", { authenticatorId: passkey.id })) as { credentials: { signCount: number }[] };
  return credentials.reduce((n, c) => n + c.signCount, 0);
};

/** Everything a page keeps where other code could read it, as text: the URL,
 * history.state, both web storages and every IndexedDB store (bytes as base64
 * and as text). */
async function kept(page: Page): Promise<{ text: string; dbs: string[] }> {
  return page.evaluate(async () => {
    const out: string[] = [location.href, JSON.stringify(history.state)];
    for (const s of [localStorage, sessionStorage]) for (let i = 0; i < s.length; i++) out.push(String(s.key(i)), String(s.getItem(String(s.key(i)))));
    const bytes = (v: unknown) => (v instanceof ArrayBuffer ? new Uint8Array(v) : ArrayBuffer.isView(v) ? new Uint8Array(v.buffer, v.byteOffset, v.byteLength) : null);
    const dbs = (await indexedDB.databases()).map((d) => d.name ?? "");
    for (const name of dbs) {
      const db = await new Promise<IDBDatabase>((ok, fail) => {
        const r = indexedDB.open(name);
        r.onsuccess = () => ok(r.result);
        r.onerror = () => fail(r.error);
      });
      for (const store of Array.from(db.objectStoreNames)) {
        const rows = await new Promise<unknown[]>((ok, fail) => {
          const r = db.transaction(store).objectStore(store).getAll();
          r.onsuccess = () => ok(r.result);
          r.onerror = () => fail(r.error);
        });
        out.push(
          JSON.stringify(rows, (_, v) => {
            const b = bytes(v);
            return b ? `${btoa(String.fromCharCode(...b))} ${new TextDecoder().decode(b)}` : v;
          }),
        );
      }
      db.close();
    }
    return { text: out.join("\n"), dbs };
  });
}

async function noSecretKept(page: Page) {
  const { text } = await kept(page);
  expect(secrets.size).toBeGreaterThan(5);
  for (const secret of secrets) expect(text.includes(secret), `a secret is kept on ${page.url()}`).toBe(false);
}

test.describe.serial("sync across three devices", () => {
  test.beforeAll(async ({ browser }) => {
    scratch = mkdtempSync(join(tmpdir(), "mq-sync-e2e-"));
    camera = join(scratch, "camera.y4m");
    relay = await startRelay(SYNC_PORTS.relay);
    for (const port of [SYNC_PORTS.a, SYNC_PORTS.b, SYNC_PORTS.c]) bridges.push(await startBridge(port));
    // Something of Desk's to see on Phone: the local sample addon.
    const c = new Client(`ws://127.0.0.1:${SYNC_PORTS.a}/ws`);
    await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
    c.close();
    film("nothing to see yet");
    deskBrowser = await chromium.launch({
      args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", `--use-file-for-fake-video-capture=${camera}`, ...BROWSER_ARGS],
    });
    // A passkey needs a host name: Desk opens Marquee on localhost.
    desk = await device(deskBrowser, SYNC_PORTS.a, "desktop", "localhost");
    // Desk approves the others: as the e2e's peer it states a phone (only phones and tablets
    // approve, D6; the web itself states `web`).
    await desk.evaluate(() => localStorage.setItem("mq.devDeviceForm", "phone"));
    phone = await device(browser, SYNC_PORTS.b, "phone");
    tv = await device(browser, SYNC_PORTS.c, "tv");
  });

  test.afterAll(async () => {
    for (const p of [desk, phone, tv]) await p?.context().close();
    await deskBrowser?.close();
    relay?.proc.kill();
    for (const { proc, data } of [...bridges, relay]) {
      proc?.kill();
      if (data) rmSync(data, { recursive: true, force: true });
    }
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  test("Desk starts an account from Settings and is asked for its kit next", async () => {
    await go(desk, "/settings/account");
    await desk.locator('[data-f="settings:sync"]').click();
    await desk.locator('[data-f="sync:start"]').click();
    await desk.locator('[data-f="sync-form:server"]').fill(`${SERVER.replace("http://", "http://me:pw@")}`);
    await desk.locator('[data-f="sync-form:go"]').click();
    await expect(desk.locator(".sync-form .st.bad")).toHaveText("Enter the address without a user name or password.");
    await desk.locator('[data-f="sync-form:server"]').fill(SERVER);
    await desk.locator('[data-f="sync-form:name"]').fill("Desk");
    await desk.locator('[data-f="sync-form:go"]').click();
    await expect(desk).toHaveURL(/\/sync\/kit\?first=1$/);
    await expect(desk.locator(".lede")).toHaveText("Your account is ready. Make its recovery kit now: without one, losing this device means losing what it syncs.");
    await desk.locator('[data-f="sync-kit:cancel"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await synced(desk);
    await expect(member(desk, "Desk")).toContainText("This device");
    await expect(member(desk, "Desk")).toContainText(", the first device");
    await expect(desk.locator(".sync-callout")).toContainText("Add a second way in");
    await expect(desk.locator(".sync-fp")).toHaveText(/^[0-9A-Z]{4} [0-9A-Z]{4} [0-9A-Z]{4}$/);
    // A second account cannot start over a live one.
    await go(desk, "/sync/start");
    await expect(desk.locator(".problem")).toContainText("This device already syncs.");
    await expect(desk.locator('[data-f="sync-form:go"]')).toHaveCount(0);
    await go(desk, "/pair");
    await expect(desk.locator(".problem")).toContainText("This device already syncs.");
    await expect(desk.locator(".qrbox")).toHaveCount(0);
    // With lists that agree, the comparison page offers nothing to continue with.
    await go(desk, "/sync/fork");
    await expect(desk.locator(".problem")).toContainText("There is nothing to compare.");
    await expect(desk.locator('[data-f="sync-fork:go"]')).toHaveCount(0);
    await go(desk, "/sync");
  });

  test("Desk remembers this browser with a passkey and opens it again after a reload", async () => {
    passkey = { cdp: await desk.context().newCDPSession(desk), id: "" };
    await passkey.cdp.send("WebAuthn.enable", { enableUI: false });
    const added = (await passkey.cdp.send("WebAuthn.addVirtualAuthenticator", {
      options: { protocol: "ctap2", ctap2Version: "ctap2_1", transport: "internal", hasResidentKey: true, hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true },
    } as never)) as { authenticatorId: string };
    passkey.id = added.authenticatorId;
    // Every passkey the page tells its manager to drop.
    await desk.evaluate(() => {
      const w = window as unknown as { dropped: string[] };
      w.dropped = [];
      (PublicKeyCredential as unknown as { signalUnknownCredential: (o: { credentialId: string }) => Promise<void> }).signalUnknownCredential = async (o) => void w.dropped.push(o.credentialId);
    });
    const fp = await desk.locator(".sync-fp").textContent();

    // A passkey made without PRF guards nothing: the page says so and asks its manager to drop it.
    await desk.evaluate(() => {
      const create = navigator.credentials.create.bind(navigator.credentials);
      (window as unknown as { restoreCreate: () => void }).restoreCreate = () => (navigator.credentials.create = create);
      navigator.credentials.create = async (o) => {
        const made = (await create(o)) as PublicKeyCredential;
        const ext = made.getClientExtensionResults.bind(made);
        Object.defineProperty(made, "getClientExtensionResults", { value: () => ({ ...ext(), prf: { enabled: false } }) });
        return made;
      };
    });
    await desk.locator('[data-f="sync:remember"]').click();
    await expect(desk.getByText("This passkey cannot lock the sync key, so this browser keeps it in this tab only.")).toBeVisible();
    const orphan = ((await passkey.cdp.send("WebAuthn.getCredentials", { authenticatorId: passkey.id })) as { credentials: { credentialId: string }[] }).credentials;
    expect(orphan).toHaveLength(1);
    const asUrl = (b64: string) => b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    expect(await desk.evaluate(() => (window as unknown as { dropped: string[] }).dropped)).toEqual([asUrl(orphan[0]?.credentialId ?? "")]);
    await desk.evaluate(() => (window as unknown as { restoreCreate: () => void }).restoreCreate());
    // A Remember whose sealed write the storage does not answer for 10 s: the words name this
    // browser's storage, never the sync server, that passkey is dropped too, and the write never
    // lands later (released, it would commit).
    await hold(desk, "put", undefined, true);
    await desk.locator('[data-f="sync:remember"]').click();
    await expect(desk.getByText("This browser could not store the sync key, so it does not remember this account.")).toBeVisible({ timeout: 20_000 });
    await expect.poll(() => desk.evaluate(() => (window as unknown as { dropped: string[] }).dropped.length)).toBe(2);
    await release(desk);
    expect((await kept(desk)).text).not.toContain("credential");

    await desk.locator('[data-f="sync:remember"]').click();
    await expect(desk.getByText("Remembered in this browser, locked with a passkey.")).toBeVisible();
    const { credentials } = (await passkey.cdp.send("WebAuthn.getCredentials", { authenticatorId: passkey.id })) as { credentials: unknown[] };
    expect(credentials).toHaveLength(3);
    const { dbs } = await kept(desk);
    expect(dbs).toContain("marquee-vault");
    await noSecretKept(desk);

    // Reloaded on /pair and /sync/start while this browser is still being checked for a
    // remembered account (IndexedDB made slow for that load): nothing starts before the answer.
    for (const path of ["/pair", "/sync/start"]) {
      await desk.evaluate(() => sessionStorage.setItem("mq.e2e.slowIdb", "1"));
      const from = asked.length;
      await desk.goto(path);
      // The page is up while the check is still out: nothing that starts an account shows.
      await expect(desk.locator("main h1")).toBeVisible();
      expect(await desk.locator(".qrbox, [data-f=\"sync-form:go\"], [data-f=\"pair:start\"]").count()).toBe(0);
      await expect(desk.getByText("This browser remembers a sync account. Unlock it or forget it on the Account page before joining another.")).toBeVisible({ timeout: 10_000 });
      expect(asked.slice(from).filter((a) => a.port === SYNC_PORTS.a && a.view === "pairing")).toEqual([]);
    }
    // Storage that cannot be read at load may hold an account: /pair starts nothing and says why.
    await desk.evaluate(() => sessionStorage.setItem("mq.e2e.failIdb", "1"));
    let from = asked.length;
    await desk.goto("/pair");
    await expect(desk.getByText('This browser could not check whether it remembers a sync account, so nothing new starts here.')).toBeVisible();
    expect(await desk.locator(".qrbox").count()).toBe(0);
    expect(asked.slice(from).filter((a) => a.port === SYNC_PORTS.a && a.view === "pairing")).toEqual([]);
    // The same on the Account page, with Check again in place of Unlock.
    await desk.evaluate(() => sessionStorage.setItem("mq.e2e.failIdb", "1"));
    from = asked.length;
    await desk.goto("/sync");
    await expect(desk.locator(".set .st.bad")).toHaveText("This browser could not check whether it remembers a sync account, so nothing new starts here. Check again.");
    // A storage that cannot be read cannot delete either: no Forget.
    await expect(desk.locator('[data-f="sync:forget-browser"]')).toHaveCount(0);
    await expect(desk.locator('[data-f="sync:unlock"]')).toHaveCount(0);
    await desk.locator('[data-f="sync:recheck"]').click();
    await expect(desk.locator('[data-f="sync:unlock"]')).toBeVisible();

    // Forget whose delete the browser aborts: the page says so and stays locked.
    await desk.evaluate(() => {
      const del = IDBObjectStore.prototype.delete;
      (window as unknown as { restoreDelete: () => void }).restoreDelete = () => (IDBObjectStore.prototype.delete = del);
      IDBObjectStore.prototype.delete = function (...args: Parameters<typeof del>) {
        const request = del.apply(this, args);
        request.addEventListener("success", () => this.transaction.abort());
        return request;
      };
    });
    await desk.locator('[data-f="sync:forget-browser"]').click();
    await expect(desk.locator(".set .st.bad")).toHaveText("This browser could not forget the sync account: it still holds it, locked with your passkey.");
    await expect(desk.locator('[data-f="sync:unlock"]')).toBeVisible();
    expect((await kept(desk)).text).toContain("credential");
    await desk.evaluate(() => (window as unknown as { restoreDelete: () => void }).restoreDelete());
    await desk.goto("/sync");
    await expect(desk.locator('[data-f="sync:unlock"]')).toBeVisible();
    await expect(desk.locator(".sync-card")).toHaveCount(0);
    await shot(desk, "sync-locked-desktop");
    await desk.locator('[data-f="sync:unlock"]').click();
    await synced(desk);
    // The same device, not a new one, and the same 12 characters.
    await expect.poll(() => names(desk)).toEqual(["Desk"]);
    await expect(desk.locator(".sync-fp")).toHaveText(fp ?? "");
    await expect(desk.getByText("Remembered in this browser, locked with a passkey.")).toBeVisible();
  });

  test("Phone pairs: Desk scans its code, reads where it is from, then approves", async () => {
    await go(phone, "/pair");
    await phone.locator('[data-f="pair:server"]').fill(SERVER);
    await phone.locator('[data-f="pair:name"]').fill("Phone");
    await phone.locator('[data-f="pair:start"]').click();
    await expect(phone.locator(".qrbox svg")).toBeVisible();
    await phone.locator('[data-f="pair:copy"]').click();
    await expect(phone.locator('[data-f="pair:copy"]')).toHaveText("Copied");
    const code = await phone.evaluate(() => navigator.clipboard.readText());
    expect(code).toMatch(/^marquee:pair\?v=2&/);
    await shot(phone, "sync-pair-phone");

    await go(desk, "/sync/add");
    await expect(desk.locator(".sync-callout")).toContainText("Approve only a code on a screen in front of you.");
    await desk.locator('[data-f="sync-add:code"]').fill("hello");
    await desk.locator('[data-f="sync-add:approve"]').click();
    await expect(desk.locator("main .st.bad")).toHaveText("Not a pairing code.");
    // The camera films the code the phone shows; nothing is approved before the question.
    film(code);
    await desk.locator('[data-f="sync-add:scan"]').click();
    await expect(desk.locator("#sync-review")).toHaveText("Approve a new device?");
    await expect(desk.locator(".confirm").getByText("This asks for your passkey first.")).toBeVisible();
    await expect(desk.locator(".code")).toHaveCount(0);
    await shot(desk, "sync-review-desktop");
    // The browser aborts the sealed write that follows the approval.
    await desk.evaluate(() => {
      const put = IDBObjectStore.prototype.put;
      (window as unknown as { restorePut: () => void }).restorePut = () => (IDBObjectStore.prototype.put = put);
      IDBObjectStore.prototype.put = function (...args: Parameters<typeof put>) {
        const store = this;
        const request = put.apply(this, args);
        // The request succeeds; the transaction stays busy for 1.5 s, then aborts before it
        // commits (so a save nobody waits for would show the digits long before its note).
        const until = Date.now() + 1500;
        const spin = () => {
          if (Date.now() < until) store.count().onsuccess = spin;
          else store.transaction.abort();
        };
        request.addEventListener("success", spin);
        return request;
      };
    });
    await desk.locator('[data-f="sync-add:review-approve"]').click();
    await expect(desk.locator(".confirm .step")).toHaveText("Signing in");
    // The approval returns only after its record was sealed, or said not to be: the note is
    // already there when its answer is.
    expect(await desk.locator(".sync-storage").isVisible()).toBe(true);
    // Where the user is: the add page says the newest key was not stored.
    await expect(desk.locator(".sync-storage")).toContainText("could not store the newest sync key");
    await shot(desk, "sync-add-desktop");

    // Nothing is asked on Phone: it joins as soon as the approval reaches it.
    await expect(phone.locator(".summary h2")).toHaveText("This device joined the account");
    await phone.locator('[data-f="pair:done"]').click();
    await expect(phone).toHaveURL(/\/sync$/);
    await synced(phone);
    await desk.locator('[data-f="sync-add:done"]').click();
    await expect(member(desk, "Phone")).toContainText("through Desk");
    // The older sealed copy, the passkey gate and the claim stay; the note is at the top of the page.
    const note = "This browser could not store the newest sync key. The copy it keeps still opens this account, and it tries again with the next change.";
    await expect(desk.locator(".sync-storage")).toContainText(note);
    await expect(desk.getByText("Remembered in this browser, locked with a passkey.")).toBeVisible();
    expect((await kept(desk)).text).toContain("credential");
    await desk.evaluate(() => (window as unknown as { restorePut: () => void }).restorePut());
    await desk.locator('[data-f="sync:save-again"]').click();
    await expect(desk.locator(".sync-storage")).toHaveCount(0);
  });

  test("the camera stops when the page is left", async () => {
    await go(desk, "/sync/add");
    film("nothing to see yet");
    await desk.locator('[data-f="sync-add:scan"]').click();
    await desk.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 2);
    const live = await desk.evaluate(() => {
      const track = (document.querySelector("video")?.srcObject as MediaStream).getVideoTracks()[0];
      (window as unknown as { seen: MediaStreamTrack }).seen = track as MediaStreamTrack;
      return track?.readyState;
    });
    expect(live).toBe("live");
    // A QR that is no pairing code is never taken.
    await expect(desk.locator("#sync-review")).toHaveCount(0);
    await go(desk, "/sync");
    expect(await desk.evaluate(() => (window as unknown as { seen: MediaStreamTrack }).seen.readyState)).toBe("ended");
    await synced(desk);

    // A tab that goes to the background stops the camera too.
    await go(desk, "/sync/add");
    await desk.locator('[data-f="sync-add:scan"]').click();
    await desk.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 2);
    await desk.evaluate(() => {
      (window as unknown as { seen: MediaStreamTrack }).seen = (document.querySelector("video")?.srcObject as MediaStream).getVideoTracks()[0] as MediaStreamTrack;
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect(desk.locator(".sync-scan video")).toHaveCount(0);
    expect(await desk.evaluate(() => (window as unknown as { seen: MediaStreamTrack }).seen.readyState)).toBe("ended");
    await go(desk, "/sync");
    // Coming back to the tab syncs.
    const before = Number(await card(desk).getAttribute("data-round"));
    await desk.evaluate(() => {
      Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect.poll(async () => Number(await card(desk).getAttribute("data-round"))).toBeGreaterThan(before);
  });

  test("what Desk had arrives on Phone", async () => {
    await go(phone, "/addons");
    await expect(phone.locator(".addon h2", { hasText: "Sample Source" })).toBeVisible({ timeout: 30_000 });
    await go(phone, "/settings/account");
    await expect(phone.locator('[data-f="settings:sync"]')).toContainText(/On\. Synced at/);
  });

  test("Phone makes a recovery kit, shown once, and the clipboard is emptied after", async () => {
    await go(phone, "/sync");
    await phone.locator('[data-f="sync:kit"]').click();
    await expect(phone.locator(".lede")).toContainText("Anyone who has it can join your account");
    await phone.locator('[data-f="sync-kit:make"]').click();
    kit = (await phone.locator(".sync-kit-text").textContent()) ?? "";
    expect(kit).toMatch(/^([0-9A-Z]{4}-){7}[0-9A-Z]{4}$/);
    await expect(phone.locator(".sync-kit .sync-callout")).toContainText("It is not shown again");
    await phone.locator('[data-f="sync-kit:copy"]').click();
    await expect(phone.locator('[data-f="sync-kit:copy"]')).toHaveText("Copied");
    await expect(phone.locator(".sync-kit .st.warn")).toContainText("Momotus empties the clipboard when you leave this page.");
    expect(await phone.evaluate(() => navigator.clipboard.readText())).toBe(kit);
    // A throwaway kit of this run's own server.
    await shot(phone, "sync-kit-phone");
    await phone.locator('[data-f="sync-kit:saved"]').click();
    await expect(phone).toHaveURL(/\/sync$/);
    await expect.poll(() => phone.evaluate(() => navigator.clipboard.readText())).toBe("");
    expect(await phone.content()).not.toContain(kit);
    await expect(member(phone, "Recovery kit")).toContainText("on Phone");
    await expect(phone.locator(".sync-callout")).toHaveCount(0);
    await shot(phone, "sync-phone");
  });

  test("the TV joins with the kit, masked until shown, typed with the remote", async () => {
    await go(tv, "/sync");
    await expect.poll(() => focused(tv)).toBeTruthy();
    for (let i = 0; i < 4 && (await focused(tv)) !== "sync:recover"; i++) await tv.keyboard.press("ArrowDown");
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync\/recover$/);
    await tv.locator('[data-f="sync-form:server"]').fill(SERVER);
    const field = tv.locator('[data-f="sync-form:kit"]');
    await expect(field).toHaveAttribute("type", "password");
    await expect(field).toHaveAttribute("autocorrect", "off");
    await field.fill("AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA");
    await shot(tv, "sync-recover-tv");
    await tv.locator('[data-f="sync-form:kit:reveal"]').focus();
    await tv.keyboard.press("Enter");
    await expect(field).toHaveAttribute("type", "text");
    await tv.keyboard.press("Enter");
    await expect(field).toHaveAttribute("type", "password");
    await tv.locator('[data-f="sync-form:go"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv.locator(".sync-form .st.bad")).toHaveText("The sync server does not know this kit. Check the letters and try again.");
    await field.fill(kit.toLowerCase());
    await tv.locator('[data-f="sync-form:name"]').fill("Living room TV");
    await tv.locator('[data-f="sync-form:go"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync$/);
    await synced(tv);
    // A TV cannot make kits: the callout says where to.
    await expect(tv.locator(".sync-callout")).toHaveText("You joined with a recovery kit. Make a new one on your phone or computer, and remove the device you lost.");
    await expect(member(tv, "Living room TV")).toContainText("through Recovery kit");
    await expect(tv.locator('[data-f="sync:add"]')).toContainText("Use your phone to sign in other devices");
    await expect(tv.locator('[data-f="sync:kit"]')).toContainText("on a TV, anyone in the room");
    await expect.poll(() => focused(tv)).toBe("sync:now");
    await shot(tv, "sync-tv");
  });

  test("on the TV every action is reachable with the remote", async () => {
    const seen = new Set<string>();
    for (let i = 0; i < 24; i++) {
      seen.add(await focused(tv));
      await tv.keyboard.press("ArrowDown");
    }
    expect([...seen].join(" ")).toMatch(/sync:[0-9a-f]+:remove/);
    for (const k of ["sync:add", "sync:kit", "sync:leave", "sync:delete"]) expect(seen.has(k)).toBe(true);
    // Report lost sits right of Remove; it opens with the remote, its focus
    // starts on Cancel, and Back leaves it untouched.
    const row = member(tv, "Phone");
    await row.locator('[data-f$=":remove"]').focus();
    await tv.keyboard.press("ArrowRight");
    await expect.poll(() => focused(tv)).toBe(await row.locator('[data-f$=":lost"]').getAttribute("data-f"));
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync\/lost\//);
    await expect.poll(() => focused(tv)).toBe("sync-lost:cancel");
    await tv.keyboard.press("Escape");
    await expect(tv).toHaveURL(/\/sync$/);
  });

  test("Desk reports Phone lost from a list it read before the TV joined, and keeps the TV", async () => {
    // Desk's page still shows the list from before the kit and the TV.
    await expect.poll(() => names(desk)).toEqual(["Desk", "Phone"]);
    const before = await signCount();
    const fp = await desk.locator(".sync-fp").textContent();
    // The first read fails: the page says so and offers to try again or leave.
    failOnce.set(SYNC_PORTS.a, { op: "lostPreview" });
    await member(desk, "Phone").locator('[data-f$=":lost"]').click();
    await expect(desk.locator(".problem .st.bad")).toHaveText("The device list did not load. The sync server did not answer. Try again in a moment.");
    await expect(desk.locator('[data-f="sync-lost:cancel"]')).toBeVisible();
    await desk.locator('[data-f="sync-lost:retry"]').click();
    await expect(desk.locator("h1")).toHaveText("Report lost: Phone");
    // The page read the list again: the TV that joined since is there to untick.
    const tvRow = desk.locator(".sync-check", { hasText: "Living room TV" });
    await expect(tvRow).toHaveAttribute("aria-checked", "true");
    await expect(tvRow).toContainText("through Recovery kit");
    await expect(desk.locator(".sync-callout")).toHaveText("1 recovery kit made on these devices stops working. Make a new one afterwards.");
    await expect(desk.locator('[data-f="sync-lost:go"]')).toHaveText("Remove 2 devices");
    await tvRow.click();
    await expect(tvRow).toHaveAttribute("aria-checked", "false");
    await expect(desk.locator('[data-f="sync-lost:go"]')).toHaveText("Remove 1 device");
    await expect(desk.getByText("This asks for your passkey first.")).toBeVisible();
    await shot(desk, "sync-lost-desktop");
    await desk.locator('[data-f="sync-lost:go"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await expect.poll(() => names(desk)).toEqual(["Desk", "Living room TV"]);
    // The 12 characters follow the new list at once, not the last round's.
    await expect.poll(() => desk.locator(".sync-fp").textContent()).not.toBe(fp);
    // The passkey confirmed the report.
    expect(await signCount()).toBeGreaterThan(before);
  });

  test("Phone learns it was removed; the TV's edit reaches Desk in one Sync now", async () => {
    await phone.locator('[data-f="sync:now"]').click();
    await expect(phone.locator("main > .st.bad")).toHaveText("This device was removed from your sync account. Your library stays here as local data.");
    await expect(phone.locator('[data-f="sync:pair"]')).toBeVisible();
    await shot(phone, "sync-removed-phone");

    // An edit on the TV (made through its core, so the page stays put and
    // nothing else catches up first) before it learns of the device change.
    // One Sync now takes the new key, stores the record, and only then the
    // next round sends the edit.
    const c = new Client(`ws://127.0.0.1:${SYNC_PORTS.c}/ws`);
    await c.req("setPrefs", { patch: { autoplayNext: false } });
    c.close();
    await syncNow(tv, true);
    await expect(state(tv)).toHaveText(/^Synced at [\d:]+\s[AP]M$/);
    await syncNow(desk);
    await expect.poll(async () => (await tv.locator(".sync-fp").textContent()) === (await desk.locator(".sync-fp").textContent())).toBe(true);
    await go(desk, "/settings/playback");
    await expect(desk.locator('[data-f="pref-next:false"]')).toHaveAttribute("aria-checked", "true");
    await go(desk, "/sync");
  });

  test("Phone pairs again: an account started meanwhile is refused while its code shows, and the join lands", async () => {
    await go(phone, "/sync");
    await phone.locator('[data-f="sync:pair"]').click();
    await phone.locator('[data-f="pair:copy"]').click();
    // An account started in Phone's tab while its code shows (from code: no screen does that while
    // /pair is open) is refused: the code holds the browser's account slot (plans/tv-qr-login.md 5.5).
    const refused = await phone.evaluate(async (server) => {
      const url = "/src/core/sync.ts";
      const m = (await import(/* @vite-ignore */ url)) as { createAccount: (s: string, n: string, i: string) => Promise<void> };
      return m.createAccount(server, "Stray", "").then(
        () => "",
        (e: { code?: string }) => e.code ?? "",
      );
    }, SERVER);
    expect(refused).toBe("joining");
    const code = await phone.evaluate(() => navigator.clipboard.readText());
    await go(desk, "/sync/add");
    await desk.locator('[data-f="sync-add:code"]').fill(code);
    const signed = await signCount();
    await desk.locator('[data-f="sync-add:approve"]').click();
    // Approving hands over the account: the passkey signed first.
    await expect.poll(signCount).toBeGreaterThan(signed);
    await expect(desk.locator(".confirm .step")).toHaveText("Signing in");
    // Nothing is asked on Phone: it joins and says who let it in.
    await expect(phone.locator(".summary h2")).toHaveText("This device joined the account");
    await expect(phone.locator(".summary")).toContainText("Signed in from Desk");
    await phone.locator('[data-f="pair:done"]').click();
    await synced(phone);
    await desk.locator('[data-f="sync-add:done"]').click();
    await expect.poll(() => names(desk)).toEqual(["Desk", "Living room TV", "Phone"]);
  });

  test("a restored sync server shows Desk a different list, which Desk compares before continuing", async () => {
    // A kit Phone makes now is on both lists later: it goes only if Phone is ticked.
    await go(phone, "/sync/kit");
    await phone.locator('[data-f="sync-kit:make"]').click();
    await expect(phone.locator(".sync-kit-text")).toBeVisible();
    await phone.locator('[data-f="sync-kit:saved"]').click();
    await go(desk, "/sync");
    await syncNow(desk);
    const snapshot = join(scratch, "snapshot");
    const stop = async () => {
      relay.proc.kill();
      await once(relay.proc, "exit");
    };
    await stop();
    mkdirSync(snapshot, { recursive: true });
    for (const f of RELAY_FILES) if (existsSync(join(relay.data, f))) copyFileSync(join(relay.data, f), join(snapshot, f));
    relay = await startRelay(SYNC_PORTS.relay, relay.data);

    // Desk makes a kit; then the operator restores the backup from before it.
    await go(desk, "/sync/kit");
    await desk.locator('[data-f="sync-kit:make"]').click();
    await expect(desk.locator(".sync-kit-text")).toBeVisible();
    await desk.locator('[data-f="sync-kit:saved"]').click();
    await stop();
    for (const f of RELAY_FILES) rmSync(join(relay.data, f), { force: true });
    for (const f of RELAY_FILES) if (existsSync(join(snapshot, f))) copyFileSync(join(snapshot, f), join(relay.data, f));
    relay = await startRelay(SYNC_PORTS.relay, relay.data, ["--new-generation"]);

    // Phone, which never saw Desk's kit, makes one of its own at the same place.
    await go(phone, "/sync/kit");
    await phone.locator('[data-f="sync-kit:make"]').click();
    await expect(phone.locator(".sync-kit-text")).toBeVisible();
    await phone.locator('[data-f="sync-kit:saved"]').click();

    // Desk opens the comparison before a round found the different list: nothing to compare yet.
    await go(desk, "/sync/fork");
    await expect(desk.locator(".problem")).toContainText("There is nothing to compare.");
    // A round finds it while the page is open (the tab comes back): the lists show without leaving.
    await desk.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
    await expect(desk.locator(".sync-member", { hasText: "only on the server's list" })).toHaveCount(1);
    await go(desk, "/sync");
    await expect(state(desk)).toHaveText("The sync server lists different devices than this device knows. Compare the lists to keep syncing.");
    // Were this device off the server's list, the page says so and offers nothing to continue.
    editOnce.set(SYNC_PORTS.a, { op: "forkPreview", edit: (r) => (r.onServer = false) });
    await desk.locator('[data-f="sync:server-list"]').click();
    await expect(desk).toHaveURL(/\/sync\/fork$/);
    await expect(desk.locator(".problem .st.bad")).toHaveText("The sync server's list no longer includes this device: it was removed there. Pair this device again from one that still syncs.");
    await expect(desk.locator('[data-f="sync-fork:go"]')).toHaveCount(0);
    // Its way out: stop syncing on this device only.
    await expect(desk.locator('[data-f="sync-fork:leave-here"]')).toBeVisible();
    await desk.locator('[data-f="sync-fork:cancel"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await desk.locator('[data-f="sync:server-list"]').click();
    await expect(desk).toHaveURL(/\/sync\/fork$/);
    // Phone's kit is only on the server's list and goes; Desk's own kit only Desk knew.
    const serverOnly = desk.locator(".sync-member", { hasText: "only on the server's list" });
    await expect(serverOnly).toHaveCount(1);
    // The top of the page: its title and what it is for.
    await shot(desk, "sync-fork-top-desktop");
    await expect(serverOnly).toContainText("Removed anyway");
    await expect(serverOnly).toContainText("on Phone");
    const mineOnly = desk.locator(".set", { has: desk.locator("h2", { hasText: "Only this device knew these" }) });
    await expect(mineOnly.locator(".sync-member")).toHaveCount(1);
    await expect(mineOnly.locator(".sync-member")).toContainText("on Desk");
    const tvTick = desk.locator(".sync-check", { hasText: "Living room TV" });
    await expect(tvTick).toHaveAttribute("aria-checked", "false");
    await tvTick.click();
    await expect(desk.locator('[data-f="sync-fork:go"]')).toHaveText("Continue and remove 1 device");
    await tvTick.click();
    await expect(desk.locator('[data-f="sync-fork:go"]')).toHaveText("Continue with the server's list");
    // Ticking Phone takes the kit it made along, and the page says so.
    const phoneTick = desk.locator(".sync-check", { has: desk.locator("b", { hasText: /^Phone$/ }) });
    await phoneTick.click();
    await expect(phoneTick).toContainText("Its recovery kit goes with it.");
    await expect(desk.locator('[data-f="sync-fork:go"]')).toHaveText("Continue and remove 1 device and 1 recovery kit");
    // The shot shows a ticked device, its kit line and the Continue that names both.
    await desk.locator('[data-f="sync-fork:go"]').scrollIntoViewIfNeeded();
    await shot(desk, "sync-fork-desktop");
    await phoneTick.click();
    // Phone adds a kit while Desk looks: continuing is refused and the lists are shown again.
    await go(phone, "/sync/kit");
    await phone.locator('[data-f="sync-kit:make"]').click();
    await expect(phone.locator(".sync-kit-text")).toBeVisible();
    await phone.locator('[data-f="sync-kit:saved"]').click();
    await desk.locator('[data-f="sync-fork:go"]').click();
    await expect(desk.getByText("The sync server's list changed while you looked. Here it is again; check it before you continue.")).toBeVisible();
    await expect(serverOnly).toHaveCount(2);
    await expect(desk).toHaveURL(/\/sync\/fork$/);
    await desk.locator('[data-f="sync-fork:go"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await expect(desk.locator(".sync-card")).not.toHaveAttribute("data-syncing", "", { timeout: 30_000 });
    await expect(state(desk)).toContainText("Synced at");
    await expect.poll(() => names(desk)).toEqual(["Desk", "Living room TV", "Phone", "Recovery kit"]);
    await syncNow(phone);
    await expect(state(phone)).toContainText("Synced at");
    await expect.poll(async () => (await phone.locator(".sync-fp").textContent()) === (await desk.locator(".sync-fp").textContent())).toBe(true);
    await shot(desk, "sync-desktop");
  });

  test("Desk removes the TV and a kit while a Forget hangs, then stops syncing, and forgets this browser", async () => {
    const row = member(desk, "Living room TV");
    await row.locator('[data-f$=":remove"]').click();
    await expect(row.locator(".sync-ask")).toContainText("Remove Living room TV? It stops syncing");
    // The removal lands; reading the list after it fails: the removal still counts.
    failOnce.set(SYNC_PORTS.a, { op: "syncDevices" });
    await row.locator('[data-f$=":confirm"]').click();
    await expect(desk.getByText("Removed: Living room TV. Done, but the device list did not refresh. Try again to see it.")).toBeVisible();
    await syncNow(desk);
    await expect.poll(() => names(desk)).toEqual(["Desk", "Phone", "Recovery kit"]);
    await tv.locator('[data-f="sync:now"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv.locator("main > .st.bad")).toContainText("This device was removed from your sync account.");

    const credentials = async () => ((await passkey.cdp.send("WebAuthn.getCredentials", { authenticatorId: passkey.id })) as { credentials: unknown[] }).credentials.length;
    const held = await credentials();
    // Forget in this browser, whose delete the storage never answers: while it may still hold the
    // sealed copy, removing a kit asks for the passkey.
    await hold(desk, "delete");
    await desk.locator('[data-f="sync:forget-browser"]').click();
    const kitRow = member(desk, "Recovery kit");
    await expect(kitRow).toContainText("on Phone");
    const signed = await signCount();
    await kitRow.locator('[data-f$=":remove"]').click();
    await expect(kitRow.locator(".sync-ask")).toContainText("Remove this recovery kit? It stops working.");
    await kitRow.locator('[data-f$=":confirm"]').click();
    // A removed kit stops being listed.
    await expect.poll(() => names(desk)).toEqual(["Desk", "Phone"]);
    expect(await signCount()).toBeGreaterThan(signed);
    // A storage that never answers fails the Forget after 10 s: the page says so and stays remembered.
    await expect(desk.locator(".sync-storage")).toContainText("This browser could not forget the sync account", { timeout: 15_000 });
    await expect(desk.getByText("Remembered in this browser, locked with a passkey.")).toBeVisible();
    await release(desk);
    await desk.locator('[data-f="sync:forget-again"]').click();
    await expect(desk.locator(".sync-storage")).toHaveCount(0);
    await expect(desk.getByText("Not remembered: the sync key lasts until this tab closes or reloads.")).toBeVisible();
    await expect.poll(credentials).toBe(held - 1);
    await desk.locator('[data-f="sync:remember"]').click();
    await expect(desk.getByText("Remembered in this browser, locked with a passkey.")).toBeVisible();

    // Count the passkey prompts: stopping sync asks it first.
    await desk.evaluate(() => {
      const w = window as unknown as { asked: number };
      w.asked = 0;
      const get = navigator.credentials.get.bind(navigator.credentials);
      navigator.credentials.get = (o) => {
        w.asked++;
        return get(o);
      };
    });
    // The browser aborts the delete of the sealed copy after 1.5 s.
    await hold(desk, "delete", 1500);
    const left = desk.getByText("This device no longer syncs. Your library stays here.");
    await desk.locator('[data-f="sync:remove-library"]').click();
    await desk.locator('[data-f="sync:leave"]').click();
    await expect(desk.locator('[data-f="sync:start"]')).toBeVisible();
    // Nothing says it left before the delete was tried.
    expect(await left.isVisible()).toBe(false);
    // Then one story: it no longer syncs, this browser still holds the account, a reload would
    // offer it again, and Forget finishes. No green line beside it.
    await expect(desk.locator(".sync-storage")).toContainText(
      "This device no longer syncs, but this browser could not forget the account: it still holds it, locked with your passkey, and reloading this page would offer to open it again.",
    );
    await expect(left).toHaveCount(0);
    expect(await desk.evaluate(() => (window as unknown as { asked: number }).asked)).toBeGreaterThan(0);
    expect((await kept(desk)).text).toContain("credential");
    await shot(desk, "sync-left-forget-failed-desktop");
    // Forget fails once more: the same one explanation stays, still without the green line.
    await desk.locator('[data-f="sync:forget-again"]').click();
    await expect.poll(() => desk.evaluate(() => (window as unknown as { held: number }).held)).toBe(2);
    await desk.evaluate(() => new Promise((done) => setTimeout(done, 100)));
    await expect(desk.locator(".sync-storage")).toContainText("This device no longer syncs, but this browser could not forget the account");
    await expect(left).toHaveCount(0);
    await release(desk);
    await desk.locator('[data-f="sync:forget-again"]').click();
    await expect(desk.locator(".sync-storage")).toHaveCount(0);
    await expect(left).toBeVisible();
    // Nothing is sealed here any more, and the passkey that guarded it was dropped by its manager.
    await expect.poll(credentials).toBe(held - 1);
    expect((await kept(desk)).text).not.toContain("credential");
    await go(desk, "/settings/account");
    await expect(desk.locator('[data-f="settings:sync"]')).toContainText("Keep your library, progress and addons the same on every device.");
  });

  test("Phone reloads without remembering: the page no longer holds the account", async () => {
    const from = asked.length;
    await go(phone, "/sync");
    await expect(phone.getByText("This browser cannot lock the sync key with a passkey")).toBeVisible();
    // The sync server cannot take the leave (here: it answers 502): this device stops syncing here only.
    failOnce.set(SYNC_PORTS.b, { op: "signOut" });
    await phone.locator('[data-f="sync:leave"]').click();
    await expect(phone.locator(".set .st.bad").first()).toContainText("This device still syncs.");
    // The core does not hold this account (here: the answer says so): plain words, and no button
    // that would only be refused again.
    failOnce.set(SYNC_PORTS.b, { op: "signOut", code: "wrongRecord" });
    await phone.locator('[data-f="sync:leave-here"]').click();
    await expect(phone.locator(".sync-leave-here .st.bad")).toHaveText(
      "This device does not hold this sync account any more, so there is nothing to stop syncing here. Reload the page; if it then offers to unlock the account, forget it in this browser.",
    );
    await expect(phone.locator('[data-f="sync:leave-here"]')).toHaveCount(0);
    // Back on the page, the same way out works.
    await go(phone, "/settings");
    await go(phone, "/sync");
    failOnce.set(SYNC_PORTS.b, { op: "signOut" });
    await phone.locator('[data-f="sync:leave"]').click();
    await phone.locator('[data-f="sync:leave-here"]').click();
    await expect(phone.getByText("This device no longer syncs. The sync server still lists it until another device removes it; you can pair it again.")).toBeVisible();
    expect(asked.slice(from).filter((a) => a.port === SYNC_PORTS.b && a.op === "signOut")).toHaveLength(4);
    await phone.reload();
    await expect(phone.locator('[data-f="sync:pair"]')).toBeVisible();
    await expect(phone.locator(".sync-card")).toHaveCount(0);
    await expect(phone.getByText("A browser has no safe place for this device's sync key")).toBeVisible();
    // Nothing remembered here: once the (slow) check settles, pairing starts with the known server.
    await phone.evaluate(() => sessionStorage.setItem("mq.e2e.slowIdb", "1"));
    await phone.goto("/pair");
    await expect(phone.locator("main h1")).toBeVisible();
    expect(await phone.locator(".qrbox").count()).toBe(0);
    await expect(phone.locator(".qrbox svg")).toBeVisible({ timeout: 15_000 });
  });

  test("a TV whose storage cannot be read at all goes on without remembering, and leaves what it never held", async () => {
    // A copy an earlier account sealed in this browser (put there directly: a TV on an IP has no passkey).
    await tv.evaluate(async () => {
      const db = await new Promise<IDBDatabase>((ok, fail) => {
        const r = indexedDB.open("marquee-vault", 1);
        r.onupgradeneeded = () => r.result.createObjectStore("sealed");
        r.onsuccess = () => ok(r.result);
        r.onerror = () => fail(r.error);
      });
      await new Promise((ok, fail) => {
        const t = db.transaction("sealed", "readwrite");
        t.objectStore("sealed").put({ credential: new Uint8Array(16).buffer, salt: new Uint8Array(32), iv: new Uint8Array(12), data: new Uint8Array(48).buffer }, "sync");
        t.oncomplete = ok;
        t.onerror = () => fail(t.error);
      });
      db.close();
    });
    await tv.evaluate(() => sessionStorage.setItem("mq.e2e.noIdb", "1"));
    await tv.goto("/settings/account");
    // Settings names the storage, not a passkey lock.
    await expect(tv.locator('[data-f="settings:sync"]')).toContainText("This browser could not check whether it remembers a sync account");
    await go(tv, "/sync");
    await expect(tv.locator('[data-f="sync:recheck"]')).toBeVisible();
    await expect(tv.locator('[data-f="sync:continue"]')).toHaveCount(0);
    await expect(tv.locator('[data-f="sync:forget-browser"]')).toHaveCount(0);
    await tv.locator('[data-f="sync:recheck"]').focus();
    await tv.keyboard.press("Enter");
    // The check waits for the storage, and says so meanwhile.
    await expect(tv.locator('[data-f="sync:recheck"]')).toHaveText("Checking");
    // The second failed check offers to go on in this tab only.
    await expect(tv.locator('[data-f="sync:continue"]')).toBeVisible();
    await shot(tv, "sync-unreadable-tv");
    await tv.locator('[data-f="sync:continue"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv.locator('[data-f="sync:pair"]')).toBeVisible();
    await go(tv, "/pair");
    await expect(tv.locator(".qrbox svg")).toBeVisible();
    // A new account, in this tab only.
    await go(tv, "/sync/start");
    await tv.locator('[data-f="sync-form:server"]').fill(SERVER);
    await tv.locator('[data-f="sync-form:name"]').fill("Kitchen TV");
    await tv.locator('[data-f="sync-form:go"]').click();
    await expect(tv).toHaveURL(/\/sync$/);
    await synced(tv);
    await expect(tv.getByText("This browser's storage could not be read, so nothing is remembered here")).toBeVisible();
    // The storage works again; leaving this account touches no copy it never held.
    await tv.evaluate(() => sessionStorage.removeItem("mq.e2e.noIdb"));
    // A TV's Sign out always removes the library too (D13): it starts again at its first run,
    // whose doors stand in for the sign-in while this browser still remembers an account.
    await expect(tv.locator('[data-f="sync:remove-library"]')).toHaveAttribute("aria-disabled", "true");
    await tv.locator('[data-f="sync:leave"]').click();
    await expect(tv).toHaveURL(/\/welcome$/, { timeout: 30_000 });
    await expect(tv.locator("h1")).toHaveText("How would you like to begin?");
    expect((await kept(tv)).text).toContain("credential");
    // Reloaded, the browser offers that copy; forgetting it leaves the TV as it was.
    await tv.goto("/sync");
    await tv.locator('[data-f="sync:forget-browser"]').click();
    await expect(tv.locator('[data-f="sync:pair"]')).toBeVisible();
    expect((await kept(tv)).text).not.toContain("credential");
  });

  test("the TV hears its account was deleted and says so once, then deletes one of its own with the remote", async () => {
    const gone = tv.getByText("Your Momotus account was deleted. Your library stays on this device.");
    const start = async () => {
      await go(tv, "/sync/start");
      await tv.locator('[data-f="sync-form:server"]').fill(SERVER);
      await tv.locator('[data-f="sync-form:name"]').fill("Kitchen TV");
      await tv.locator('[data-f="sync-form:go"]').click();
      await expect(tv).toHaveURL(/\/sync$/);
      await synced(tv);
    };
    const press = async (key: string) => {
      await tv.locator(`[data-f="${key}"]`).focus();
      await tv.keyboard.press("Enter");
    };
    await start();
    // Another device deleted the account (here: the core's answer says so): this one forgets it,
    // says so, and OK puts the line away.
    failOnce.set(SYNC_PORTS.c, { op: "syncNow", code: "accountGone" });
    await press("sync:now");
    await expect(gone).toBeVisible();
    await expect(tv.locator('[data-f="sync:pair"]')).toBeVisible();
    await expect.poll(() => focused(tv)).toBe("sync:gone-ok");
    await shot(tv, "sync-account-gone-tv");
    // Not seen yet when the page reloads: it is said again, until OK.
    await tv.reload();
    await expect(gone).toBeVisible();
    await press("sync:gone-ok");
    await expect(gone).toHaveCount(0);
    await tv.reload();
    await expect(tv.locator('[data-f="sync:pair"]')).toBeVisible();
    await expect(gone).toHaveCount(0);

    // Deleting here, for real: one confirmation that names what goes, then the sync server deletes it.
    await start();
    const from = asked.length;
    await press("sync:delete");
    await expect(tv.locator(".sync-ask")).toContainText("Your profiles, library, progress and settings are deleted from the sync service. This device keeps its own copy.");
    await expect.poll(() => focused(tv)).toBe("sync:delete-cancel");
    await shot(tv, "sync-delete-ask-tv");
    // No answer: the page claims nothing, keeps the account, and the confirm works again.
    failOnce.set(SYNC_PORTS.c, { op: "deleteAccount" });
    await press("sync:delete-confirm");
    await expect(tv.getByText("The deletion was not confirmed. The sync server did not answer. Try again in a moment.")).toBeVisible();
    await expect(card(tv)).toBeVisible();
    await press("sync:delete-confirm");
    await expect(gone).toBeVisible();
    await expect(tv.locator('[data-f="sync:pair"]')).toBeVisible();
    expect(asked.slice(from).filter((a) => a.port === SYNC_PORTS.c && a.op === "deleteAccount")).toHaveLength(2);
  });

  test("nothing secret reaches storage, history, the URL or the console", async () => {
    for (const page of [desk, phone, tv]) await noSecretKept(page);
    const logged = consoleLines.join("\n");
    for (const secret of secrets) expect(logged.includes(secret), "a secret reached the console").toBe(false);
    expect(errors).toEqual([]);
  });
});
