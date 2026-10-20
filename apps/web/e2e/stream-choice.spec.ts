/** Smart stream choice on the web (plans/m13-stream-choice.md 3.7, 3.9,
 * 3.11, 4.3, 4.4). Settings > Stream choice writes to a fresh core of its
 * own (DEFAULTS_PORT; the suite runs one test at a time). Versions reads the
 * shared core's real lines, conditions and blocked state (this build has no
 * engine and no account, so a title only Torrentio lists is blocked). What
 * the core cannot reach here (nothing plays on this device, a learning note,
 * a measured speed) is put into its answers at the socket, and the writes
 * they lead to are checked as sent; playback is never opened. Shots go to SHOTS (e2e/core.ts) at TV, desktop
 * and phone sizes. */

import { type Browser, type BrowserContext, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { readFileSync } from "node:fs";
import type { Prefs } from "../src/core/generated/Prefs";
import type { StreamConditions } from "../src/core/generated/StreamConditions";
import type { VersionsSnapshot } from "../src/core/generated/VersionsSnapshot";
import { Client, cleanup, DEFAULTS_PORT, GOT, PORTS, SHOTS, startBridge } from "./core";

type Form = "tv" | "desktop" | "phone";
const FORMS = {
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};
const OUT = SHOTS;
const GiB = 1024 ** 3;
/** The Shawshank Redemption: only Torrentio lists it here. */
const SHAWSHANK = "tt0111161";
const errors: string[] = [];

type Msg = Record<string, unknown> & { id?: number; op?: string; view?: string; sub?: number };
/** What the page sent, in order. */
const sent: Msg[] = [];

const NOTE_PREFS: Prefs["streams"] = { quality: "balanced", maxHeight: null, maxSizeBytes: null, languages: [], avoid: [], sizeLimitBytes: 5 * GiB, minSeeders: 50, hdr: true, preferReady: true, learn: true };

// The words the core's states are said in (shared with Android and Apple).
const choice = JSON.parse(readFileSync(new URL("../../shared/stream-choice-words.json", import.meta.url), "utf8")) as { nobodySharing: { text: string; details: string } };
const shot = (page: Page, name: string, form: Form, fullPage = false) =>
  page.waitForTimeout(500).then(() => page.screenshot({ path: `${OUT}/${name}-${form}.jpg`, type: "jpeg", quality: 80, fullPage }));
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

test.beforeEach(() => {
  sent.length = 0;
  errors.length = 0;
});
test.afterEach(() => expect(errors).toEqual([]));

/** A page on `port`'s core. `answer` replies to a request itself (return
 * the result); `versions` rewrites the versions view's snapshots. A
 * playback view is never opened. */
async function device(
  browser: Browser,
  form: Form,
  port: number,
  opts: { answer?: (m: Msg) => unknown; versions?: (s: VersionsSnapshot) => VersionsSnapshot } = {},
): Promise<Page> {
  const context: BrowserContext = await browser.newContext({ ...FORMS[form], colorScheme: "dark", reducedMotion: "reduce", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    const asked = new Set<number>();
    const subs = new Set<number>();
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => {
      const msg = JSON.parse(String(e.data)) as Msg & { ok?: boolean; result?: { sub: number; snapshot: VersionsSnapshot }; updates?: VersionsSnapshot[] };
      const rewrite = opts.versions;
      if (rewrite && msg.id !== undefined && asked.has(msg.id) && msg.ok && msg.result) {
        subs.add(msg.result.sub);
        return ws.send(JSON.stringify({ ...msg, result: { ...msg.result, snapshot: rewrite(msg.result.snapshot) } }));
      }
      if (rewrite && msg.sub !== undefined && subs.has(msg.sub) && msg.updates) return ws.send(JSON.stringify({ ...msg, updates: msg.updates.map(rewrite) }));
      ws.send(String(e.data));
    });
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as Msg;
      sent.push(msg);
      if (msg.op === "subscribe" && msg.view === "playback") return;
      if (msg.op === "subscribe" && msg.view === "versions" && msg.id !== undefined) asked.add(msg.id);
      const own = opts.answer?.(msg);
      if (own !== undefined) return ws.send(JSON.stringify({ id: msg.id, ok: true, result: own }));
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => upstream.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  return page;
}

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
  const prefs = () => client.req<Prefs>("setPrefs", { patch: {} });
  const streams = async () => (await prefs()).streams;

  test("each row writes its preference to the core, and Settings sums it up", async ({ browser }) => {
    const page = await device(browser, "desktop", DEFAULTS_PORT);
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
    // The core's conditions: priors only, so nothing measured and no Measure again.
    await expect(page.locator(".device-line .cond")).toHaveText(/^(4K|1080p|720p)( HDR)?, (Wi-Fi|Ethernet|mobile data|this network), speed not measured yet$/);
    await expect(page.locator('[data-f="choice:measure"]')).toHaveCount(0);
    expect(sent.some((m) => m.op === "streamConditions")).toBe(true);

    const picks: [string, Partial<Prefs["streams"]>][] = [
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

  test("what was learned shows with Undo each, and Forget clears it everywhere", async ({ browser }) => {
    // Counters as two devices' picks would leave them: 4K bigger files, 1080p fewer lines.
    await client.req("setPrefs", { patch: { learned: { uhd: { size: 3, picks: 4 }, fhd: { height: -4, picks: 5 } } } });
    const page = await device(browser, "desktop", DEFAULTS_PORT);
    await page.goto("/stream-choice?form=desktop");
    const rows = page.locator(".learned .lrow");
    await expect(rows).toHaveText(["On 4K screens you often picked bigger files.Undo", "On 1080p screens you often picked lower resolutions.Undo"]);
    await page.locator(".learned").scrollIntoViewIfNeeded();
    await shot(page, "stream-choice-learned", "desktop");
    await page.locator('[data-f="learned:2160:size"]').focus();
    await page.keyboard.press("Enter");
    // The core's undoLearned: that counter back to zero, and it learns no more.
    expect(sent.find((m) => m.op === "undoLearned")).toMatchObject({ screenClass: 2160, choice: "size" });
    await expect.poll(async () => (await prefs()).learned.uhd).toMatchObject({ size: 0, muted: ["size"] });
    await expect(rows).toHaveCount(1);
    // By keys, focus stays near (the Learn choice), never on Forget.
    await expect.poll(() => focused(page)).toBe("choice-learn:true");
    await page.locator('[data-f="learned:forget"]').click();
    // forgetLearned: an epoch above every stamp, so every counter reads as zero.
    expect(sent.filter((m) => m.op === "forgetLearned")).toHaveLength(1);
    await expect.poll(async () => (await prefs()).learnedEpoch).toBeGreaterThan(0);
    await expect(page.locator(".learned")).toHaveCount(0);
  });

  test("Measure again resets this network's speed through the core", async ({ browser }) => {
    // The core's speeds are priors until playbacks measure them: the first
    // answer is made measured here; resetSpeed goes to the real core.
    let first = true;
    const page = await device(browser, "tv", DEFAULTS_PORT, {
      answer: (m) => {
        if (m.op !== "streamConditions" || !first) return undefined;
        first = false;
        const measured: StreamConditions = { network: "wifi", http: { mbps: 42.4, samples: 3 }, p2p: { mbps: 18, samples: 2 }, freeBytes: 61 * GiB, engine: true };
        return measured;
      },
    });
    await page.goto("/stream-choice?form=tv");
    const cond = page.locator(".device-line .cond");
    // No account and the engine: the torrent speed is the one that decides.
    await expect(cond).toHaveText(/^(4K|1080p|720p)( HDR)?, about 18 Mbit\/s on Wi-Fi, 61 GB free$/);
    const again = page.locator('[data-f="choice:measure"]');
    await again.scrollIntoViewIfNeeded();
    await again.focus();
    await shot(page, "stream-choice-device", "tv");
    await page.keyboard.press("Enter");
    await expect(page.locator(".device-line .st")).toHaveText("Measuring at the next playback");
    await expect(cond).toContainText("speed not measured yet");
    // Spent, not gone: focus stays on it, and a second press asks nothing.
    await expect.poll(() => focused(page)).toBe("choice:measure");
    await expect(again).toHaveAttribute("aria-disabled", "true");
    await page.keyboard.press("Enter");
    expect(sent.filter((m) => m.op === "resetSpeed")).toHaveLength(1);
  });

  test("the page at each size; a TV starts on the first choice and picks with select", async ({ browser }) => {
    for (const form of ["tv", "desktop", "phone"] as const) {
      const page = await device(browser, form, DEFAULTS_PORT);
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

test.describe("Versions", () => {
  const path = (form: Form) => `/title/series/${GOT.id}/versions/${GOT.resume}?form=${form}`;
  /** The first row, once every addon has answered. */
  async function firstRow(page: Page) {
    await expect(page.locator(".vrow .line").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".vstate .live")).toHaveCount(0, { timeout: 60_000 });
    return page.locator(".vrow").first();
  }

  test("the view is asked with the title's facts and the network, and says what it read", async ({ browser }) => {
    const page = await device(browser, "tv", PORTS.bridge);
    await page.goto(path("tv"));
    await firstRow(page);
    const ask = sent.find((m) => m.op === "subscribe" && m.view === "versions")?.params as Record<string, unknown>;
    expect(ask).toMatchObject({ kind: "series", videoId: GOT.resume, network: "other" });
    expect(ask.runtime).toMatch(/\d+ min/);
    expect(ask.releaseInfo).toMatch(/^\d{4}/);
    // The header is the core's conditions, in words. An HTTP playback that
    // ran 30 s on this shared core (shots.spec) measures the line.
    await expect(page.locator(".vstate .device")).toHaveText(
      /^This TV: (4K|1080p|720p|SD)( HDR)?, (this network, speed not measured yet|about \d+ Mbit\/s on this network)$/,
    );
  });

  test("TV: a focused row shows its line's details under it, with the tone icon", async ({ browser }) => {
    const page = await device(browser, "tv", PORTS.bridge);
    await page.goto(path("tv"));
    const row = await firstRow(page);
    await expect(row.locator(".line svg.tone")).toHaveCount(1);
    await row.focus();
    await expect(row.locator(".more .detail")).toBeVisible();
    await expect(row.locator(".line .tip")).toBeHidden();
    await shot(page, "stream-choice-versions", "tv");
  });

  test("desktop: hovering a row shows its details in a tip", async ({ browser }) => {
    const page = await device(browser, "desktop", PORTS.bridge);
    await page.goto(path("desktop"));
    const row = await firstRow(page);
    const tip = row.locator(".line .tip");
    await expect(tip).toBeHidden();
    await row.hover();
    await expect(tip).toBeVisible();
    await expect.poll(() => tip.evaluate((t) => getComputedStyle(t).opacity)).toBe("1");
    // The tip says what the TV's focus line says.
    await expect(tip).toHaveText((await row.locator(".more .detail").textContent()) as string);
    // It opens below its line, clear of the group's pinned heading above.
    const at = await row.evaluate((r) => ({
      tip: (r.querySelector(".line .tip") as HTMLElement).getBoundingClientRect().top,
      line: (r.querySelector(".line") as HTMLElement).getBoundingClientRect().bottom,
    }));
    expect(at.tip).toBeGreaterThan(at.line);
    // Via shows the source's whole name.
    const via = row.locator(".via");
    expect(await via.evaluate((v) => v.scrollWidth <= v.clientWidth)).toBe(true);
    await shot(page, "stream-choice-versions", "desktop");
  });

  test("phone: a tap opens the row with its details and Play, a second tap closes it", async ({ browser }) => {
    const page = await device(browser, "phone", PORTS.bridge);
    await page.goto(path("phone"));
    const row = await firstRow(page);
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
  });

  /** Every version as blocked for `reason` (3.11), for a reason this build's
   * core cannot reach on its own. */
  const blockedAs = (reason: "nothingPlaysHere") => (s: VersionsSnapshot): VersionsSnapshot => {
    const rows = [...(s.recommended ? [s.recommended] : []), ...s.rows];
    return rows.length && s.done ? { ...s, recommended: null, rows: [], blocked: { reason, count: rows.length, rows } } : s;
  };

  test("blocked, nothing plays here: the card says why, and Show them lists the versions", async ({ browser }) => {
    for (const form of ["tv", "phone"] as const) {
      const page = await device(browser, form, PORTS.bridge, { versions: blockedAs("nothingPlaysHere") });
      await page.goto(path(form));
      const card = page.locator(".pick .blocked");
      await expect(card.locator("p")).toHaveText(/^Found \d+ versions?, but none plays on this device\.$/, { timeout: 60_000 });
      await expect(page.locator(".vrow")).toHaveCount(0);
      // The header counts what was found, not the empty list.
      await expect(page.locator(".vstate .st").first()).not.toHaveText("0 versions");
      const show = page.locator('[data-f="versions:show"]');
      if (form === "tv") await expect.poll(() => focused(page)).toBe("versions:show");
      await shot(page, "stream-choice-blocked", form);
      await show.click();
      await expect(show).toHaveAttribute("aria-expanded", "true");
      await expect(page.locator(".vrow").first()).toBeVisible();
      await page.context().close();
    }
  });

  test("blocked, needs an account (the core's own answer): words, Add an account and Show them, on Versions, Title and the player", async ({ browser }) => {
    // Only Torrentio lists S1:E1, and this build has no engine and no
    // account: the core blocks every row (no answer is changed here).
    const page = await device(browser, "desktop", PORTS.bridge);
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.id}:1:1?form=desktop`);
    const card = page.locator(".pick .blocked");
    await expect(card.locator("p")).toHaveText(
      /^Marquee found \d+ versions?\. This version of the app plays them only through a streaming service account \(Real-Debrid, TorBox, AllDebrid or Premiumize\)\. Add one in Settings\.$/,
      { timeout: 60_000 },
    );
    await expect(page.locator(".vstate .st").first()).toHaveText(/^\d+ versions?$/);
    await expect(page.locator(".vstate .st").first()).not.toHaveText("0 versions");
    const account = page.locator('[data-f="versions:account"]');
    await expect(account).toHaveText("Add an account");
    await expect(account).toHaveAttribute("href", "/debrid");
    await expect(page.locator(".vrow")).toHaveCount(0);
    await shot(page, "stream-choice-account", "desktop");
    // Show them: the versions as they would rank with an account, each with its line.
    await page.locator('[data-f="versions:show"]').click();
    await expect(page.locator(".vrow .line").first()).toBeVisible();
    await shot(page, "stream-choice-account-shown", "desktop");
    // A film with only Torrentio versions: Title's primary leads to the account.
    await page.goto(`/title/movie/${SHAWSHANK}?form=desktop`);
    await expect(page.locator('[data-f="title:addaccount"]')).toHaveText("Add an account", { timeout: 60_000 });
    await expect(page.locator(".vline")).toContainText("only through a streaming service account");
    // The player, opened straight (a deep link), says the same (no playback is opened).
    await page.goto(`/play/movie/${SHAWSHANK}/${SHAWSHANK}?form=desktop`);
    await expect(page.locator(".player-scr")).toContainText("only through a streaming service account", { timeout: 60_000 });
  });

  test("almost nobody sharing is said once: on the card before Play, over the list only with no card", async ({ browser }) => {
    const words = `${choice.nobodySharing.text}${choice.nobodySharing.details}`;
    for (const form of ["desktop", "phone"] as const) {
      const page = await device(browser, form, PORTS.bridge, { versions: (s) => ({ ...s, nobodySharing: true }) });
      await page.goto(path(form));
      await firstRow(page);
      const card = page.locator(".pick .nobody");
      await expect(card).toHaveText(words);
      await expect(card).toHaveAttribute("role", "status");
      // Said before Play: the line comes right before the button.
      await expect(page.locator('.pick .nobody + [data-f="versions:play"]')).toHaveCount(1);
      // Once per screen: not over the list as well.
      await expect(page.locator(".nobody")).toHaveCount(1);
      await shot(page, "stream-choice-nobody-sharing", form);
      await page.context().close();
    }
    // No recommended card: the list says it, once.
    const listOnly = await device(browser, "desktop", PORTS.bridge, { versions: (s) => ({ ...s, nobodySharing: true, recommended: null }) });
    await listOnly.goto(path("desktop"));
    await firstRow(listOnly);
    await expect(listOnly.locator(".alts .lnote.nobody")).toHaveText(words);
    await expect(listOnly.locator(".alts .lnote.nobody")).toHaveAttribute("role", "status");
    await expect(listOnly.locator(".nobody")).toHaveCount(1);
    await listOnly.context().close();
    // This core has no engine, so no live counts: it never says so.
    const plain = await device(browser, "desktop", PORTS.bridge);
    await plain.goto(path("desktop"));
    await firstRow(plain);
    await expect(plain.locator(".pick h2")).toBeVisible();
    await expect(plain.locator(".nobody")).toHaveCount(0);
    await plain.context().close();
  });

  test("the learning note shows once with Undo, which mutes that choice", async ({ browser }) => {
    // The note as the core sends it when a counter crosses 3; Undo
    // (undoLearned) is answered here, so the shared core's profile is untouched.
    const page = await device(browser, "desktop", PORTS.bridge, {
      versions: (s) => ({ ...s, note: { screenClass: 1080, choice: "size", up: true, prefs: { ...NOTE_PREFS, sizeLimitBytes: 10 * GiB } } }),
      answer: (m) => (m.op === "undoLearned" ? {} : undefined),
    });
    await page.goto(path("desktop"));
    const note = page.locator(".lnote");
    await expect(note).toContainText("On this computer you often picked bigger files, so Marquee now picks files up to 10 GB.", { timeout: 60_000 });
    await shot(page, "stream-choice-note", "desktop");
    await page.locator('[data-f="versions:undo"]').click();
    await expect(note).toHaveCount(0);
    expect(sent.find((m) => m.op === "undoLearned")).toMatchObject({ screenClass: 1080, choice: "size" });
  });
});
