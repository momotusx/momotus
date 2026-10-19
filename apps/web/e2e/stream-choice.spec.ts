/** Smart stream choice on the web (plans/m13-stream-choice.md 3.7, 4.4).
 * Settings > Stream choice writes the v2 stream preferences to a fresh core
 * of its own (DEFAULTS_PORT; the suite runs one test at a time). The version
 * rows' lines are shown on the shared core with each row's line made up at
 * the socket from its own seeds and size, as rank.rs words them, until the
 * core sends lines (phase 2); playback is never opened. Shots go to
 * e2e/shots/ at TV, desktop and phone sizes. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import type { Prefs } from "../src/core/generated/Prefs";
import type { VersionRow } from "../src/core/generated/VersionRow";
import type { ReasonLine, StreamChoicePrefs } from "../src/lib/words";
import { Client, cleanup, DEFAULTS_PORT, GOT, PORTS, startBridge } from "./core";

type Form = "tv" | "desktop" | "phone";
const FORMS = {
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};
const OUT = process.env.SHOTS_DIR ?? "e2e/shots";
const GiB = 1024 ** 3;
const errors: string[] = [];

const shot = (page: Page, name: string, form: Form, fullPage = false) =>
  page.waitForTimeout(500).then(() => page.screenshot({ path: `${OUT}/${name}-${form}.jpg`, type: "jpeg", quality: 80, fullPage }));
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test.afterEach(() => {
  expect(errors).toEqual([]);
  errors.length = 0;
});

test.describe("Settings > Stream choice", () => {
  let core: { proc: ChildProcess; data: string } | null = null;
  let client: Client;
  test.beforeEach(async () => {
    core = await startBridge(DEFAULTS_PORT);
    client = new Client(`ws://127.0.0.1:${DEFAULTS_PORT}/ws`);
  });
  test.afterEach(() => {
    client.close();
    if (core) cleanup(core.proc, core.data);
    core = null;
  });

  /** A page whose socket reaches this test's core. */
  async function device(browser: Browser, form: Form): Promise<Page> {
    const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", reducedMotion: "reduce", baseURL: `http://127.0.0.1:${PORTS.vite}` });
    await context.routeWebSocket(/\/ws$/, (ws) => {
      const upstream = new WebSocket(`ws://127.0.0.1:${DEFAULTS_PORT}/ws`);
      const early: string[] = [];
      upstream.addEventListener("open", () => {
        for (const m of early.splice(0)) upstream.send(m);
      });
      upstream.addEventListener("message", (e) => ws.send(String(e.data)));
      upstream.addEventListener("close", () => ws.close());
      ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
      ws.onClose(() => upstream.close());
    });
    const page = await context.newPage();
    page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
    return page;
  }
  const streams = async () => (await client.req<Prefs>("setPrefs", { patch: {} })).streams as Prefs["streams"] & StreamChoicePrefs;

  test("each row writes its preference to the core, and Settings sums it up", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await page.goto("/settings?form=desktop");
    const row = page.locator('[data-f="settings:stream-choice"]');
    await expect(row).toContainText("Up to 5 GB, shared by at least 50 people");
    // The old Pick and Up to chips are gone; Audio moved with them.
    await expect(page.locator('[data-fgroup="pref-quality"], [data-fgroup="pref-height"], [data-fgroup="pref-audio"]')).toHaveCount(0);
    await row.click();
    await expect(page).toHaveURL(/\/stream-choice/);
    await expect(page.locator("h1")).toHaveText("Stream choice");
    // A Settings page: the navigation stays, with Settings lit.
    await expect(page.locator('[data-f="nav:/settings"]')).toBeVisible();
    // The defaults (3.8), read from the core.
    for (const f of ["choice-size:5368709120", "choice-seeders:50", "choice-height:null", "choice-hdr:true", "choice-learn:true"])
      await expect(page.locator(`[data-f="${f}"]`), f).toHaveAttribute("aria-checked", "true");
    // No streaming service account: its row is not shown.
    await expect(page.locator('[data-fgroup="choice-ready"]')).toHaveCount(0);
    await expect(page.locator(".device-line .cond")).toHaveText(/^(4K|1080p|720p)( HDR)?, (Wi-Fi|Ethernet|mobile data|this network), speed not measured yet$/);

    const picks: [string, Partial<StreamChoicePrefs & { maxHeight: number | null }>][] = [
      ["choice-size:null", { sizeLimitBytes: null }],
      ["choice-seeders:100", { minSeeders: 100 }],
      ["choice-height:720", { maxHeight: 720 }],
      ["choice-hdr:false", { hdr: false }],
      ["choice-learn:false", { learn: false }],
    ];
    for (const [f, want] of picks) {
      await page.locator(`[data-f="${f}"]`).click();
      await expect.poll(streams, { message: f }).toMatchObject(want);
      await expect(page.locator(`[data-f="${f}"]`)).toHaveAttribute("aria-checked", "true");
    }
    await page.goto("/settings?form=desktop");
    await expect(row).toContainText("Any size, shared by at least 100 people");
  });

  test("the page at each size; a TV starts on the first choice and picks with select", async ({ browser }) => {
    for (const form of ["tv", "desktop", "phone"] as const) {
      const page = await device(browser, form);
      await page.goto(`/stream-choice?form=${form}`);
      await expect(page.locator('[data-f="choice-size:5368709120"]')).toHaveAttribute("aria-checked", "true");
      if (form === "tv") {
        await expect.poll(() => focused(page)).toBe("choice-size:2147483648");
        await page.keyboard.press("Enter");
        await expect.poll(async () => (await streams()).sizeLimitBytes).toBe(2 * GiB);
        await page.keyboard.press("ArrowRight");
        await page.keyboard.press("Enter");
        await expect.poll(async () => (await streams()).sizeLimitBytes).toBe(5 * GiB);
      }
      await shot(page, "stream-choice-settings", form, form === "phone");
      await page.context().close();
    }
  });
});

test.describe("Versions: each row's line", () => {
  /** A line from the row's own facts, as rank.rs words them with no account. */
  function lineFor(r: VersionRow): ReasonLine {
    const seeds = r.status.kind === "torrent" ? r.status.seeds : null;
    if (r.status.kind === "ready" || r.status.kind === "direct") return { kind: "ready", service: r.status.kind === "ready" ? r.status.service : null, sizeBytes: r.sizeBytes };
    if (seeds !== null && seeds < 5) return { kind: "veryFew", seeders: seeds };
    if (r.sizeBytes !== null && r.sizeBytes > 1.1 * 5 * GiB) return { kind: "large", sizeBytes: r.sizeBytes, limitBytes: 5 * GiB };
    if (seeds !== null && seeds < 50) return { kind: "few", seeders: seeds };
    if (seeds === null || seeds >= 200) return { kind: "quick", seeders: seeds, sizeBytes: r.sizeBytes };
    return { kind: "should", seeders: seeds, sizeBytes: r.sizeBytes, service: null };
  }
  type Snap = { recommended: VersionRow | null; rows: (VersionRow & { line?: ReasonLine })[] };
  const withLines = (s: Snap) => ({ ...s, rows: s.rows.map((r) => ({ ...r, line: lineFor(r) })) });

  /** The shared core, with lines added to the versions view and playback never opened. */
  async function device(browser: Browser, form: Form): Promise<Page> {
    const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", reducedMotion: "reduce", baseURL: `http://127.0.0.1:${PORTS.vite}` });
    await context.routeWebSocket(/\/ws$/, (ws) => {
      const server = ws.connectToServer();
      const asked = new Set<number>();
      const subs = new Set<number>();
      ws.onMessage((m) => {
        const msg = JSON.parse(String(m)) as { id?: number; op?: string; view?: string };
        if (msg.op === "subscribe" && msg.view === "playback") return;
        if (msg.op === "subscribe" && msg.view === "versions" && msg.id !== undefined) asked.add(msg.id);
        server.send(m);
      });
      server.onMessage((m) => {
        const msg = JSON.parse(String(m)) as { id?: number; ok?: boolean; result?: { sub: number; snapshot: Snap }; sub?: number; updates?: Snap[] };
        if (msg.id !== undefined && asked.has(msg.id) && msg.ok && msg.result) {
          subs.add(msg.result.sub);
          return ws.send(JSON.stringify({ ...msg, result: { ...msg.result, snapshot: withLines(msg.result.snapshot) } }));
        }
        if (msg.sub !== undefined && subs.has(msg.sub) && msg.updates) return ws.send(JSON.stringify({ ...msg, updates: msg.updates.map(withLines) }));
        ws.send(m);
      });
    });
    const page = await context.newPage();
    page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
    return page;
  }
  const path = (form: Form) => `/title/series/${GOT.id}/versions/${GOT.resume}?form=${form}`;
  /** The first row with a line, once every addon has answered. */
  async function firstLine(page: Page) {
    await expect(page.locator(".vrow .line").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".vstate .live")).toHaveCount(0, { timeout: 60_000 });
    return { row: page.locator(".vrow").filter({ has: page.locator(".line") }).first() };
  }

  test("TV: a focused row shows its details under it, with the tone icon", async ({ browser }) => {
    const page = await device(browser, "tv");
    await page.goto(path("tv"));
    const { row } = await firstLine(page);
    await expect(row.locator(".line svg.tone")).toHaveCount(1);
    await row.focus();
    const detail = row.locator(".more .detail");
    await expect(detail).toBeVisible();
    expect((await detail.textContent())?.length).toBeGreaterThan(0);
    // One place for the details on a TV: no tip.
    await expect(row.locator(".line .tip")).toBeHidden();
    await shot(page, "stream-choice-versions", "tv");
    await page.context().close();
  });

  test("desktop: hovering a row shows its details in a tip", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await page.goto(path("desktop"));
    const { row } = await firstLine(page);
    const tip = row.locator(".line .tip");
    await expect(tip).toBeHidden();
    await row.hover();
    await expect(tip).toBeVisible();
    await expect.poll(() => tip.evaluate((t) => getComputedStyle(t).opacity)).toBe("1");
    // The tip says what the TV's focus line says.
    const key = (await row.getAttribute("data-f")) as string;
    const said = await page.evaluate((k) => document.querySelector(`[data-f="${k}"] .more .detail`)?.textContent, key);
    await expect(tip).toHaveText(said as string);
    await shot(page, "stream-choice-versions", "desktop");
    await page.context().close();
  });

  test("phone: a tap opens the row with its details and Play, a second tap closes it", async ({ browser }) => {
    const page = await device(browser, "phone");
    await page.goto(path("phone"));
    const { row } = await firstLine(page);
    const url = page.url();
    await row.tap();
    await expect(row).toHaveAttribute("aria-expanded", "true");
    const open = page.locator(".vrow-open");
    await expect(open).toHaveCount(1);
    await expect(open.locator(".detail")).toBeVisible();
    await expect(open.locator("button", { hasText: "Play this version" })).toBeVisible();
    expect(page.url()).toBe(url);
    await open.scrollIntoViewIfNeeded();
    await shot(page, "stream-choice-versions", "phone");
    await row.tap();
    await expect(open).toHaveCount(0);
    expect(page.url()).toBe(url);
    await page.context().close();
  });
});
