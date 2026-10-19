/** Trakt through the web screens, for real: a core of its own whose Trakt is
 * a local fake (e2e/trakt-fake.ts, reached through the dev bridge's
 * MQ_DEV_TRAKT), and a second core built as a release without a Trakt app
 * would be (no MQ_DEV_TRAKT). Connect shows the code, the fake approves it,
 * the card says who is connected, Sync now runs a round, a playback shows the
 * Trakt mark and scrobbles, a refused stop lists the film as not on Trakt,
 * and signing out ends it. The fake's tokens are looked for in storage, the
 * URL, history, the console and the page at every shot. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { rmSync } from "node:fs";
import { Client, PORTS, startBridge, TRAKT_PORTS } from "./core";
import { BBB_ID } from "./source";
import { type FakeTrakt, startFakeTrakt, USER, USER_CODE } from "./trakt-fake";

let fake: FakeTrakt;
const cores: { proc: ChildProcess; data: string }[] = [];
let page: Page;
let plain: Page;
const consoleLines: string[] = [];
const errors: string[] = [];

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
};

/** A page whose socket reaches the core on `port`. */
async function device(browser: Browser, port: number, form: keyof typeof FORMS): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => ws.send(String(e.data)));
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (core.readyState === WebSocket.OPEN ? core.send(String(m)) : early.push(String(m))));
    ws.onClose(() => core.close());
  });
  const p = await context.newPage();
  p.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  p.on("console", (m) => consoleLines.push(m.text()));
  await p.goto(`/settings?form=${form}`);
  return p;
}

/** In-app navigation: a reload would drop the tab's Trakt record, as it does for a user. */
const go = async (p: Page, path: string) => {
  await p.evaluate((to) => {
    history.pushState(null, "", to);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
  await expect(p).toHaveURL(new RegExp(`${path.replace(/[/?]/g, "\\$&")}$`));
};

/** Everything a page keeps where other code could read it, as text. */
const kept = (p: Page) =>
  p.evaluate(async () => {
    const out: string[] = [location.href, JSON.stringify(history.state)];
    for (const s of [localStorage, sessionStorage]) for (let i = 0; i < s.length; i++) out.push(String(s.key(i)), String(s.getItem(String(s.key(i)))));
    for (const { name } of await indexedDB.databases()) {
      if (!name) continue;
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
        out.push(JSON.stringify(rows));
      }
      db.close();
    }
    return out.join("\n");
  });

/** No token in the page, then the shot. */
const shot = async (p: Page, name: string) => {
  await p.waitForTimeout(400);
  const html = await p.content();
  for (const secret of fake.secrets) expect(html.includes(secret), `a token is on the page in ${name}`).toBe(false);
  await p.screenshot({ path: `e2e/shots/${name}.jpg`, type: "jpeg", quality: 80 });
};

const card = (p: Page) => p.locator(".trakt-card");
const asked = (method: string, path: string) => fake.seen.filter((s) => s.method === method && s.path === path).length;

test.describe.serial("Trakt on the web", () => {
  test.beforeAll(async ({ browser }) => {
    fake = await startFakeTrakt(TRAKT_PORTS.fake);
    cores.push(await startBridge(TRAKT_PORTS.bridge, { MQ_DEV_TRAKT: fake.url }));
    cores.push(await startBridge(TRAKT_PORTS.plain));
    // What a playback needs: the film's details and the local source.
    const c = new Client(`ws://127.0.0.1:${TRAKT_PORTS.bridge}/ws`);
    for (const url of ["https://v3-cinemeta.strem.io/manifest.json", `http://127.0.0.1:${PORTS.source}/manifest.json`])
      for (let attempt = 1; ; attempt++) {
        try {
          await c.req("installAddon", { url });
          break;
        } catch (e) {
          if (attempt === 3) throw e;
        }
      }
    c.close();
    page = await device(browser, TRAKT_PORTS.bridge, "desktop");
    plain = await device(browser, TRAKT_PORTS.plain, "tv");
  });

  test.afterAll(async () => {
    for (const p of [page, plain]) await p?.context().close();
    for (const { proc, data } of cores) {
      proc.kill();
      rmSync(data, { recursive: true, force: true });
    }
    await fake?.close();
  });

  test("Connect shows the code to enter at Trakt; approved there, the card says who is connected", async () => {
    await go(page, "/trakt");
    await expect(card(page)).toHaveAttribute("data-state", "off");
    await expect(card(page)).toContainText("Not connected.");
    await page.locator('[data-f="trakt:connect"]').click();
    await expect(page.locator(".trakt-code .code")).toHaveText(USER_CODE);
    await expect(page.locator('[data-f="trakt:url"]')).toHaveAttribute("href", "https://trakt.tv/activate");
    await expect(page.locator(".trakt-code .qrbox svg")).toBeVisible();
    await expect(page.locator(".trakt-code .st")).toContainText(/Good for (9|10):\d\d\./);
    await shot(page, "trakt-code-desktop");
    fake.approve();
    await expect(page.locator(".trakt-connect .st.ok")).toHaveText(`Connected as ${USER}`, { timeout: 15_000 });
    await page.locator('[data-f="trakt:done"]').click();
    await expect(card(page)).toHaveAttribute("data-state", "connected");
    await expect(card(page).locator(".trakt-state .st")).toHaveText(`Connected as ${USER}`);
    // Kept in this tab only: this browser does not remember a sync account.
    await expect(card(page)).toContainText("The sign-in lasts until this tab closes or reloads.");
  });

  test("Sync now runs a round against Trakt", async () => {
    const before = asked("GET", "/sync/last_activities");
    await page.locator('[data-f="trakt:sync"]').click();
    await expect(card(page).locator(".st.ok").last()).toHaveText(/^Synced: \d+ sent, \d+ brought in$/, { timeout: 20_000 });
    expect(asked("GET", "/sync/last_activities")).toBeGreaterThan(before);
    await expect(card(page)).toContainText(/Last synced \d/);
    await shot(page, "trakt-connected-desktop");
  });

  test("a playback shows the Trakt mark and scrobbles; a stop Trakt refuses lists the film", async () => {
    await go(page, `/play/movie/${BBB_ID}/${BBB_ID}`);
    const mark = page.locator(".trakt-mark");
    await expect(mark).toHaveAttribute("data-trakt", "on", { timeout: 30_000 });
    await expect(mark.locator(".sr")).toHaveText("Trakt: this play is recorded");
    await expect.poll(() => asked("POST", "/scrobble/start"), { timeout: 30_000 }).toBeGreaterThan(0);
    // The controls up, so the mark shows under the title.
    await page.mouse.move(700, 450);
    await page.mouse.move(720, 470);
    await shot(page, "trakt-player-desktop");
    // Half way (said with the next tick), then the page is left: the core pauses the scrobble.
    await page.evaluate(() => {
      const v = document.querySelector("video");
      if (v) v.currentTime = v.duration / 2;
    });
    await page.waitForTimeout(6000);
    await page.evaluate(() => dispatchEvent(new Event("pagehide")));
    await expect.poll(() => asked("POST", "/scrobble/pause"), { timeout: 15_000 }).toBeGreaterThan(0);
    // Still playing, the next tick starts it again; paused by hand, the pause goes at once,
    // and Trakt does not know the film, so it is listed as not on Trakt.
    const started = asked("POST", "/scrobble/start");
    await expect.poll(() => asked("POST", "/scrobble/start"), { timeout: 15_000 }).toBeGreaterThan(started);
    const paused = asked("POST", "/scrobble/pause");
    fake.answer("POST /scrobble/pause", [404]);
    await page.evaluate(() => document.querySelector("video")?.pause());
    await expect.poll(() => asked("POST", "/scrobble/pause"), { timeout: 15_000 }).toBeGreaterThan(paused);
    await go(page, "/trakt");
    await expect(page.locator(".trakt-unmapped li").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".trakt-unmapped")).toContainText("Big Buck Bunny");
    await shot(page, "trakt-unmapped-desktop");
  });

  test("Sign out asks first, then ends the connection; nothing of it stays", async () => {
    await page.locator('[data-f="trakt:sign-out"]').click();
    await expect(page.locator(".trakt-ask")).toContainText("Watched marks and progress stay here.");
    await page.locator('[data-f="trakt:sign-out-confirm"]').click();
    await expect(card(page)).toHaveAttribute("data-state", "off", { timeout: 15_000 });
    await expect(card(page)).toContainText("Signed out of Trakt on this device.");
    await expect.poll(() => asked("POST", "/oauth/revoke"), { timeout: 15_000 }).toBeGreaterThan(0);
  });

  test("a build without a Trakt app says so and offers nothing to press", async () => {
    await go(plain, "/trakt");
    await expect(card(plain)).toHaveAttribute("data-state", "notConfigured");
    await expect(card(plain)).toContainText("Trakt is not set up in this build of Marquee");
    await expect(card(plain).locator("button")).toHaveCount(0);
    await shot(plain, "trakt-not-configured-tv");
  });

  test("no token reaches storage, history, the URL or the console", async () => {
    for (const p of [page, plain]) {
      const text = await kept(p);
      for (const secret of fake.secrets) expect(text.includes(secret), "a token is kept").toBe(false);
    }
    const logged = consoleLines.join("\n");
    for (const secret of fake.secrets) expect(logged.includes(secret), "a token reached the console").toBe(false);
    expect(errors).toEqual([]);
  });
});
