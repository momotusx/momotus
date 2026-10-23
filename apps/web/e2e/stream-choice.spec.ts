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
import type { EmptyAction } from "../src/core/generated/EmptyAction";
import type { EmptyKind } from "../src/core/generated/EmptyKind";
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
const choice = JSON.parse(readFileSync(new URL("../../shared/stream-choice-words.json", import.meta.url), "utf8")) as { nobodySharing: { text: string } };
const shot = (page: Page, name: string, form: Form, fullPage = false) =>
  page.waitForTimeout(500).then(() => page.screenshot({ path: `${OUT}/${name}-${form}.jpg`, type: "jpeg", quality: 80, fullPage }));
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
/** The lowest WCAG contrast of a dimmed row's words on the page: each word's
 * color with every opacity up to the row, laid on the page color. */
const dimmedWords = (page: Page) =>
  page.evaluate(() => {
    type RGBA = [number, number, number, number];
    const rgba = (c: string): RGBA => {
      const [r = 0, g = 0, b = 0, a = 1] = (c.match(/[\d.]+/g) ?? []).map(Number);
      return [r, g, b, a];
    };
    const lum = ([r, g, b]: RGBA) => {
      const lin = (v: number) => (v / 255 <= 0.04045 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
      return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    };
    const probe = document.createElement("i");
    probe.style.color = "var(--bg)";
    document.body.append(probe);
    const bg = rgba(getComputedStyle(probe).color);
    probe.remove();
    const row = [...document.querySelectorAll<HTMLElement>(".vrow.dim")].find((r) => r !== document.activeElement);
    const words = [...(row?.querySelectorAll<HTMLElement>(".q, .chips span, .size, .lt") ?? [])].filter((el) => el.textContent?.trim());
    if (!words.length) return 0;
    return Math.min(
      ...words.map((el) => {
        const [r, g, b] = rgba(getComputedStyle(el).color);
        let a = rgba(getComputedStyle(el).color)[3];
        for (let n: HTMLElement | null = el; n && n !== row?.parentElement; n = n.parentElement) a *= Number(getComputedStyle(n).opacity);
        const fg: RGBA = [r * a + bg[0] * (1 - a), g * a + bg[1] * (1 - a), b * a + bg[2] * (1 - a), 1];
        return (Math.max(lum(fg), lum(bg)) + 0.05) / (Math.min(lum(fg), lum(bg)) + 0.05);
      }),
    );
  });

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
    // No speed is said until a playback has measured it: the network alone.
    await expect(page.locator(".device-line .cond")).toHaveText(/^(4K|1080p|720p)( HDR)?, (Wi-Fi|Ethernet|mobile data|this network)(, \d+ GB free)?$/);
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
    // The old speed is gone and none is said until the next playback measures one.
    await expect(cond).not.toContainText("Mbit/s");
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
    // Every addon answered: the header's status line is empty (it never names one).
    await expect(page.locator(".vstatus")).toHaveText("", { timeout: 60_000 });
    // Said once the list is done: the count, never each row; the status line itself is not a live region.
    await expect(page.locator('.versions-scr p.sr[role="status"]')).toHaveText(/^\d+ versions?$/);
    await expect(page.locator(".vstatus")).not.toHaveAttribute("role", "status");
    return page.locator(".vrow").first();
  }

  test("the view is asked with the title's facts and the network, and says what it read", async ({ browser }) => {
    // An HTTP playback that ran 30 s on this shared core (shots.spec) has
    // measured the line: "Measure again" first, so the header reads the
    // estimate whatever ran before.
    const c = new Client();
    try {
      await c.req("resetSpeed");
    } finally {
      c.close();
    }
    const page = await device(browser, "tv", PORTS.bridge);
    await page.goto(path("tv"));
    await firstRow(page);
    const ask = sent.find((m) => m.op === "subscribe" && m.view === "versions")?.params as Record<string, unknown>;
    expect(ask).toMatchObject({ kind: "series", videoId: GOT.resume, network: "other" });
    expect(ask.runtime).toMatch(/\d+ min/);
    expect(ask.releaseInfo).toMatch(/^\d{4}/);
    // No device line on Versions (plans/versions-web-spec.md 3.1): the count, then the status line.
    await expect(page.locator(".vstate")).toHaveText(/^\d+ versions?$/);
    await expect(page.locator(".device")).toHaveCount(0);
    await expect(page.locator(".versions-scr")).not.toContainText(/This TV:|speed not measured/);
  });

  /** The rects of rows 2 and 3: a details line that grew row 1 would move them (row 1 itself lifts on TV focus). */
  const rects = (page: Page) => page.locator(".vrow").evaluateAll((rs) => rs.slice(1, 3).map((r) => JSON.stringify(r.getBoundingClientRect())));

  test("TV: a focused row shows its details in its reserved line, with the tone icon; nothing moves", async ({ browser }) => {
    const page = await device(browser, "tv", PORTS.bridge, { versions: longBest(10) });
    await page.goto(path("tv"));
    const row = await firstRow(page);
    await expect(row.locator(".line svg.tone")).toHaveCount(1);
    const more = row.locator(".more");
    await expect(more).toHaveCSS("opacity", "0");
    const before = await rects(page);
    expect(before).toHaveLength(2);
    await row.focus();
    await expect(more).toHaveCSS("opacity", "1");
    await expect(more).not.toHaveText("");
    expect(await rects(page)).toEqual(before);
    await expect(page.locator(".tip, .via")).toHaveCount(0);
    await shot(page, "stream-choice-versions", "tv");
  });

  test("desktop: hovering a row fades in its reserved details line, which is seen; nothing moves", async ({ browser }) => {
    const page = await device(browser, "desktop", PORTS.bridge, { versions: longBest(10) });
    await page.goto(path("desktop"));
    const row = await firstRow(page);
    const more = row.locator(".more");
    await expect(more).toHaveCSS("opacity", "0");
    const before = await rects(page);
    expect(before).toHaveLength(2);
    await row.hover();
    await expect.poll(() => more.evaluate((m) => getComputedStyle(m).opacity), { timeout: 2_000 }).toBe("1");
    expect(await rects(page)).toEqual(before);
    // Seen, not only laid out: the point at the line's middle is the line (not clipped by the
    // row slot's paint skip), inside the viewport.
    const seen = await more.evaluate((m) => {
      const r = m.getBoundingClientRect();
      const x = r.left + Math.min(r.width, 40) / 2;
      const y = r.top + r.height / 2;
      return { hit: document.elementFromPoint(x, y) === m, inside: r.top >= 0 && r.bottom <= innerHeight };
    });
    expect(seen).toEqual({ hit: true, inside: true });
    await expect(page.locator(".tip, .via")).toHaveCount(0);
    await shot(page, "stream-choice-versions", "desktop");
  });

  test("phone: one tap plays a row; its info button opens a sheet on an entry of its own, closed by Back, Done or a swipe", async ({ browser }) => {
    const page = await device(browser, "phone", PORTS.bridge);
    await page.goto(path("phone"));
    const row = await firstRow(page);
    const url = page.url();
    const key = (await row.getAttribute("data-f"))?.slice(2);
    // The info button: beside the row, 44 px, out of the reader's way (the row's description says the same).
    const info = page.locator(".vslot").first().locator(".vinfo");
    expect(await info.boundingBox()).toMatchObject({ width: 44, height: 44 });
    await expect(info).toHaveAttribute("aria-hidden", "true");
    const sheet = page.locator(".vsheet");
    // The sheet: the row's details and Play; Back closes it and leaves the page where it was.
    await info.tap();
    await expect(sheet).toBeVisible();
    await expect(sheet).toHaveAttribute("role", "dialog");
    await expect(sheet.locator(".rel")).toContainText(".webm");
    await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBe("vsheet:play");
    await expect(page).toHaveURL(/[?&]info/);
    // Nothing behind it can be reached: a row there takes no focus.
    await row.evaluate((r) => (r as HTMLElement).focus());
    await expect(row).not.toBeFocused();
    await expect(page.locator(".vsheet-scrim")).toHaveCSS("overscroll-behavior", "contain");
    await shot(page, "stream-choice-versions", "phone");
    await page.goBack();
    await expect(sheet).toHaveCount(0);
    expect(page.url()).toBe(url);
    await expect.poll(() => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBe(`v:${key}`);
    // Done, and a swipe down.
    await info.tap();
    await sheet.locator('[data-f="vsheet:done"]').tap();
    await expect(sheet).toHaveCount(0);
    expect(page.url()).toBe(url);
    await info.tap();
    const box = await sheet.boundingBox();
    if (!box) throw new Error("no sheet");
    await page.mouse.move(box.x + box.width / 2, box.y + 10);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2, box.y + 140, { steps: 5 });
    await page.mouse.up();
    await expect(sheet).toHaveCount(0);
    expect(page.url()).toBe(url);
    // The sheet's Play plays that version, and its entry is gone first: Back from the play is this page.
    await info.tap();
    await sheet.locator('[data-f="vsheet:play"]').tap();
    await expect(page).toHaveURL(/\/play\//);
    expect(await page.evaluate(() => (history.state as { key?: string } | null)?.key)).toBe(key);
    await page.goBack();
    expect(page.url()).toBe(url);
    // One tap on a row plays it.
    await (await firstRow(page)).tap();
    await expect(page).toHaveURL(/\/play\//);
    await page.context().close();
  });

  /** The core's zero-playable state `kind` (plans/play-every-version.md 2a), put on the
   * snapshot once the view is done, for a state this build's core does not reach here. The
   * states that list rows list every version found; `key` is the kept version. */
  const emptyAs = (kind: EmptyKind, action: EmptyAction, key = false) => (s: VersionsSnapshot): VersionsSnapshot => {
    const rows = [...(s.recommended ? [s.recommended] : []), ...s.rows];
    const listed = kind === "nothingPlaysHere" || kind === "needsAccount";
    if (!s.done || !rows.length) return s;
    return { ...s, recommended: null, rows: [], empty: { kind, action, count: listed ? rows.length : 0, rows: listed ? rows : [], key: key ? (rows[0]?.key ?? null) : null } };
  };
  const words = JSON.parse(readFileSync(new URL("../../shared/stream-choice-words.json", import.meta.url), "utf8")) as { empty: Record<EmptyKind, { text: string; row?: string }> };
  // Every state, its actions on Versions, then in the player (the shared Step 3 table).
  const STATES: [EmptyKind, EmptyAction, boolean, string[], string[]][] = [
    ["noSource", "settingsAddons", false, ["Settings", "Back"], ["Settings", "Back"]],
    ["unreachable", "playFromDevice", true, ["Play from this device", "Try again", "Back"], ["Play from this device", "Try again", "Back"]],
    ["unreachable", "tryAgain", false, ["Try again", "Back"], ["Try again", "Back"]],
    ["noRoom", "none", false, ["Back"], ["Back"]],
    ["needsAccount", "settingsAccounts", false, ["Settings", "Back"], ["Settings", "Back"]],
    ["kidsNothing", "none", false, ["Back"], ["Back"]],
    ["noResults", "none", false, ["Back"], ["Back"]],
    ["nothingPlaysHere", "chooseVersion", false, ["Back"], ["Choose a version", "Back"]],
  ];

  test("every zero-playable state: the core's line in the reserved status line, its actions in the table's order, the first focused", async ({ browser }) => {
    let state: (s: VersionsSnapshot) => VersionsSnapshot = (s) => s;
    const tv = await device(browser, "tv", PORTS.bridge, { versions: (s) => state(s) });
    const desk = await device(browser, "desktop", PORTS.bridge, { versions: (s) => state(s) });
    const rects = () =>
      desk.evaluate(() => [".vhead", ".vstatus"].map((q) => JSON.stringify(document.querySelector(q)?.getBoundingClientRect())));
    // The header and its status line as a page with versions draws them.
    await desk.goto(path("desktop"));
    await expect(desk.locator(".vrow").first()).toBeVisible({ timeout: 60_000 });
    await expect(desk.locator(".vstatus")).toHaveText("");
    const listed = await rects();
    for (const [kind, action, key, onVersions, inPlayer] of STATES) {
      state = emptyAs(kind, action, key);
      const text = words.empty[kind].text;
      await tv.goto(path("tv"));
      await expect(tv.locator(".vstatus")).toHaveText(text, { timeout: 60_000 });
      await expect(tv.locator(".pick .blocked .btn")).toHaveText(onVersions);
      // Back leads when it is the one action.
      if (onVersions.length === 1) await expect(tv.locator('[data-f="versions:back"]')).toHaveClass(/\bprimary\b/);
      await expect.poll(() => focused(tv)).toBe(await tv.locator(".pick .blocked .btn").first().getAttribute("data-f"));
      if (kind === "noSource") await expect(tv.locator('[data-f="versions:settings"]')).toHaveAttribute("href", "/addons");
      if (kind === "needsAccount") await expect(tv.locator('[data-f="versions:settings"]')).toHaveAttribute("href", "/debrid");
      // Rows only where the table lists them.
      if (kind === "nothingPlaysHere" || kind === "needsAccount") {
        await expect(tv.locator(".vrow.dim").first()).toBeVisible();
        // Each says why in place of its own line (a lock for an account, a ban for this device).
        await expect(tv.locator(".vrow.dim:not(:focus) .st.line.why .lt").first()).toHaveText(new RegExp(`^(\\S+ [KMG]B, )?${(words.empty[kind].row ?? "").replace(/^./, (x) => x.toLowerCase())}$`));
      }
      else await expect(tv.locator(".vrow")).toHaveCount(0);
      // Nothing moves: the line sits in the slot reserved from the first paint.
      await desk.goto(path("desktop"));
      await expect(desk.locator(".vstatus")).toHaveText(text, { timeout: 60_000 });
      expect(await rects()).toEqual(listed);
      // The player's card: the same line, its actions (Choose a version is the player's).
      await tv.goto(`/play/series/${GOT.id}/${GOT.resume}?form=tv`);
      const card = tv.locator(".endcard");
      await expect(card.locator(".st")).toHaveText(text, { timeout: 60_000 });
      await expect(card.locator(".acts .btn")).toHaveText(inPlayer);
      if (inPlayer.length === 1) await expect(tv.locator('[data-f="player:back"]')).toHaveClass(/\bprimary\b/);
      await expect.poll(() => focused(tv)).toBe(await card.locator(".acts .btn").first().getAttribute("data-f"));
    }
    // Play from this device starts the kept version.
    state = emptyAs("unreachable", "playFromDevice", true);
    await desk.goto(path("desktop"));
    await desk.locator('[data-f="versions:device"]').click({ timeout: 60_000 });
    await expect(desk).toHaveURL(new RegExp(`/play/series/${GOT.id}/`));
    await tv.context().close();
    await desk.context().close();
  });

  test("blocked, nothing plays here: the status line says why, the rows are listed dimmed, Back is the one action", async ({ browser }) => {
    for (const form of ["tv", "phone"] as const) {
      const page = await device(browser, form, PORTS.bridge, { versions: emptyAs("nothingPlaysHere", "chooseVersion") });
      await page.goto(path(form));
      await expect(page.locator(".vstatus")).toHaveText("None of these plays on this device", { timeout: 60_000 });
      // The header counts what was found.
      await expect(page.locator(".vstate")).toHaveText(/^\d+ versions?$/);
      await expect(page.locator(".vstate")).not.toHaveText("0 versions");
      // The rows, dimmed, each still an attempt; no Show them, no second action.
      await expect(page.locator(".vrow.dim").first()).toBeVisible();
      // Dimmed by their icon and muted words, never an alpha: every word keeps 4.5:1.
      expect(await dimmedWords(page)).toBeGreaterThanOrEqual(4.5);
      await expect(page.locator('.vrow[aria-disabled="true"]')).toHaveCount(0);
      await expect(page.locator('[data-f="versions:show"]')).toHaveCount(0);
      await expect(page.locator(".pick .blocked .btn")).toHaveCount(1);
      await expect(page.locator('.pick .blocked [data-f="versions:back"]')).toHaveText("Back");
      if (form === "tv") await expect.poll(() => focused(page)).toBe("versions:back");
      await shot(page, "stream-choice-blocked", form);
      // The player sends this reason to Versions: no account to add.
      await page.goto(`/play/series/${GOT.id}/${GOT.resume}?form=${form}`);
      await expect(page.locator(".player-scr")).toContainText("None of these plays on this device", { timeout: 60_000 });
      await expect(page.locator('[data-f="player:account"]')).toHaveCount(0);
      await expect(page.locator('[data-f="player:versions"]')).toHaveClass(/\bprimary\b/);
      await page.context().close();
    }
  });

  test("blocked, needs an account (the core's own answer): the line, Settings and Back, the rows dimmed, on Versions and the player", async ({ browser }) => {
    // Only Torrentio lists S1:E1, and this build has no engine and no
    // account: the core blocks every row (no answer is changed here).
    const page = await device(browser, "desktop", PORTS.bridge);
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.id}:1:1?form=desktop`);
    await expect(page.locator(".vstatus")).toHaveText("These play through a streaming account", { timeout: 60_000 });
    await expect(page.locator(".vstate")).toHaveText(/^\d+ versions?$/);
    await expect(page.locator(".vstate")).not.toHaveText("0 versions");
    // Settings (the accounts screen), then Back; never "Add an account" or "Show them".
    const settings = page.locator('[data-f="versions:settings"]');
    await expect(settings).toHaveText("Settings");
    await expect(settings).toHaveAttribute("href", "/debrid");
    await expect(page.locator(".pick .blocked .btn")).toHaveText(["Settings", "Back"]);
    await expect(page.locator(".versions-scr")).not.toContainText(/Add an account|Show them/);
    // The versions as they would rank with an account, listed dimmed; they cannot be started here.
    const row = page.locator(".vrow").first();
    await expect(row).toHaveAttribute("aria-disabled", "true");
    await expect(row).toHaveClass(/\bdim\b/);
    const url = page.url();
    await row.dispatchEvent("click");
    await page.waitForTimeout(300);
    expect(page.url()).toBe(url);
    await shot(page, "stream-choice-account", "desktop");
    // A film with only Torrentio versions: Title's primary leads to the account.
    await page.goto(`/title/movie/${SHAWSHANK}?form=desktop`);
    await expect(page.locator('[data-f="title:addaccount"]')).toHaveText("Add an account", { timeout: 60_000 });
    await expect(page.locator(".vline")).toContainText("only through a streaming service account");
    // The player, opened straight (a deep link), says the same (no playback is opened): Settings,
    // then Back, never "Add an account" (accounts live in Settings).
    await page.goto(`/play/movie/${SHAWSHANK}/${SHAWSHANK}?form=desktop`);
    await expect(page.locator(".player-scr")).toContainText("These play through a streaming account", { timeout: 60_000 });
    const open = page.locator('[data-f="player:settings"]');
    await expect(open).toHaveText("Settings");
    await expect(open).toHaveAttribute("href", "/debrid");
    await expect(page.locator(".endcard .acts .btn")).toHaveText(["Settings", "Back"]);
    await expect(page.locator('[data-f="player:account"], [data-f="player:versions"]')).toHaveCount(0);
    await open.click();
    await expect(page).toHaveURL(/\/debrid/);
    await page.context().close();
    // TV: the remote starts on it, and select opens the accounts screen.
    const tv = await device(browser, "tv", PORTS.bridge);
    await tv.goto(`/play/movie/${SHAWSHANK}/${SHAWSHANK}?form=tv`);
    await expect.poll(() => focused(tv), { timeout: 60_000 }).toBe("player:settings");
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/debrid/);
    await tv.context().close();
  });

  test("blocked for an account: an account added while the card shows, or from it and Back, starts the play", async ({ browser }) => {
    const KEY = "MQDUMMYE2EKEY1";
    const film = `/play/movie/${SHAWSHANK}/${SHAWSHANK}?form=desktop`;
    // Playback subscribes are held back here (no playback is opened): one for a version is the start.
    const started = (from: number) =>
      sent.slice(from).some((m) => m.op === "subscribe" && m.view === "playback" && typeof (m.params as { key?: unknown } | undefined)?.key === "string");
    const connect = async (p: Page) => {
      await p.locator('[data-f="debrid:premiumize:field"]').fill(KEY);
      await p.locator('[data-f="debrid:premiumize:save"]').click();
      await expect(p.locator('.dcard[aria-label="Premiumize"] .dcard-head .st')).toHaveText("Connected");
    };
    const disconnect = async (p: Page) => {
      await p.goto("/debrid?form=desktop");
      const button = p.locator('[data-f="debrid:premiumize:disconnect"]');
      await button.click();
      await expect(button).toHaveText("Press again to disconnect");
      await button.click();
      await expect(p.locator('.dcard[aria-label="Premiumize"]').getByText("Disconnected")).toBeVisible();
    };
    const page = await device(browser, "desktop", PORTS.bridge);
    const screen = page.locator(".player-scr");
    try {
      // While the card shows: an account added elsewhere (another device, another tab) moves it on.
      await page.goto(film);
      await expect(screen).toContainText("These play through a streaming account", { timeout: 60_000 });
      const before = sent.length;
      const other = await device(browser, "desktop", PORTS.bridge);
      await other.goto("/debrid?form=desktop");
      await connect(other);
      await expect(screen).not.toContainText("These play through a streaming account", { timeout: 30_000 });
      await expect.poll(() => started(before), { timeout: 30_000 }).toBe(true);
      await disconnect(other);
      await other.context().close();
      // From the card: Settings, a key, and Back to the play, which starts.
      await page.goto(film);
      await expect(screen).toContainText("These play through a streaming account", { timeout: 60_000 });
      const again = sent.length;
      await page.locator('[data-f="player:settings"]').click();
      await expect(page).toHaveURL(/\/debrid/);
      await connect(page);
      await page.goBack();
      await expect(page).toHaveURL(/\/play\/movie\//);
      await expect(screen).not.toContainText("These play through a streaming account", { timeout: 30_000 });
      await expect.poll(() => started(again), { timeout: 30_000 }).toBe(true);
    } finally {
      // The rest of this spec runs with no account.
      await disconnect(page);
      await page.context().close();
    }
  });

  test("almost nobody sharing draws no bar anywhere: the why line says it when it is the reason", async ({ browser }) => {
    const words = choice.nobodySharing.text;
    for (const form of ["desktop", "phone"] as const) {
      const page = await device(browser, form, PORTS.bridge, { versions: (s) => ({ ...s, nobodySharing: true }) });
      await page.goto(path(form));
      await firstRow(page);
      await expect(page.locator(".nobody")).toHaveCount(0);
      await expect(page.locator(".versions-scr")).not.toContainText(words);
      await page.context().close();
    }
    const listOnly = await device(browser, "desktop", PORTS.bridge, { versions: (s) => ({ ...s, nobodySharing: true, recommended: null }) });
    await listOnly.goto(path("desktop"));
    await firstRow(listOnly);
    await expect(listOnly.locator(".nobody")).toHaveCount(0);
    await listOnly.context().close();
  });

  /** The core's list with `n` rows in its 1080p section and 40 in 720p and below (copies of its first row, each its own key). */
  const longBest = (n: number) => (s: VersionsSnapshot): VersionsSnapshot => {
    const first = s.rows[0] ?? s.recommended;
    if (!first || !s.done) return s;
    const copy = (q: "fhd" | "hd", k: number) => Array.from({ length: k }, (_, i) => ({ ...first, key: `${first.key}#${q}${i}` }));
    const [fhd, hd] = [copy("fhd", n), copy("hd", 40)];
    return { ...s, rows: [...fhd, ...hd], sections: [{ quality: "fhd", keys: fhd.map((r) => r.key) }, { quality: "hd", keys: hd.map((r) => r.key) }] };
  };

  test("TV: a long open section is left by Left to its heading's Show fewer, then Left to Play; folding keeps the toggle in place", async ({ browser }) => {
    const page = await device(browser, "tv", PORTS.bridge, { versions: longBest(100) });
    // Stand in for Safari, which has no scroll anchoring: the page must place the toggle itself.
    await page.addInitScript(() => document.addEventListener("DOMContentLoaded", () => {
      const s = document.createElement("style");
      s.textContent = "* { overflow-anchor: none !important; }";
      document.head.append(s);
    }));
    await page.goto(path("tv"));
    await firstRow(page);
    // The section below open too, so the list stays long after a fold (no clamp hides a jump).
    await page.locator('[data-f="fold:hd"]').dispatchEvent("click");
    await expect(page.locator('[data-group="hd"] .vrow')).toHaveCount(40);
    // Show all opens the section with the focus on its first row that was hidden.
    await page.locator('[data-f="fold:fhd"]').focus();
    await page.keyboard.press("Enter");
    const fourth = await page.locator('[data-group="fhd"] .vrow').nth(3).getAttribute("data-f");
    await expect.poll(() => focused(page)).toBe(fourth);
    // Row 50: Left goes to the heading's Show fewer, Left again to Play.
    const row50 = page.locator('[data-group="fhd"] .vrow').nth(49);
    await row50.focus();
    await page.keyboard.press("ArrowLeft");
    await expect.poll(() => focused(page)).toBe("foldtop:fhd");
    await page.keyboard.press("ArrowLeft");
    await expect.poll(() => focused(page)).toBe("versions:play");
    // Row 50 again: Left, then OK folds it (two presses). The heading stays where it was (no jump,
    // whatever the browser's scroll anchoring) and the bottom toggle, focused, is in view under it.
    await row50.focus();
    await page.keyboard.press("ArrowLeft");
    const heading = page.locator('[data-group="fhd"] .vghead');
    const top = await heading.evaluate((e) => e.getBoundingClientRect().top);
    await page.keyboard.press("Enter");
    await expect.poll(() => focused(page)).toBe("fold:fhd");
    await expect(page.locator('[data-group="fhd"] .vrow')).toHaveCount(3);
    expect(Math.abs((await heading.evaluate((e) => e.getBoundingClientRect().top)) - top)).toBeLessThanOrEqual(1);
    await expect(page.locator('[data-f="fold:fhd"]')).toBeInViewport();
    await page.context().close();
  });

  test("desktop: Home from a row of an open section goes to its Show fewer; the heading keeps its name", async ({ browser }) => {
    const page = await device(browser, "desktop", PORTS.bridge, { versions: longBest(20) });
    await page.goto(path("desktop"));
    await firstRow(page);
    await page.locator('[data-f="fold:fhd"]').click();
    await expect(page.locator('[data-f="foldtop:fhd"]')).toHaveAccessibleName("Show fewer 1080p versions");
    await expect(page.locator("#vg-fhd")).toHaveText(/^1080p\s*20$/);
    await page.locator('[data-group="fhd"] .vrow').nth(12).focus();
    await page.keyboard.press("Home");
    await expect.poll(() => focused(page)).toBe("foldtop:fhd");
    await page.context().close();
  });

  test("the card: one why line, Play named and described by it; the core's shortcuts under it, each plays its version", async ({ browser }) => {
    // The core's shortcuts, as two of its rows under their own keys.
    const short = (s: VersionsSnapshot): VersionsSnapshot => {
      const a = s.rows[0];
      return a && s.done ? { ...s, smaller: { ...a, key: `${a.key}#smaller`, sizeBytes: 100 * 2 ** 20 }, sharper: { ...a, key: `${a.key}#sharper`, quality: "1080p" } } : s;
    };
    const page = await device(browser, "tv", PORTS.bridge, { versions: short });
    await page.goto(path("tv"));
    await firstRow(page);
    const why = page.locator(".pick .why1");
    await expect(why).not.toBeEmpty();
    // One line, clamped; the full sentence describes Play.
    expect(await why.evaluate((e) => getComputedStyle(e).whiteSpace)).toBe("nowrap");
    const play = page.locator('[data-f="versions:play"]');
    await expect(play).toHaveAccessibleDescription((await why.textContent()) ?? "");
    await expect(play).toHaveAccessibleName(/^Play \S+/);
    const smaller = page.locator('[data-f="vshort:smaller"]');
    const sharper = page.locator('[data-f="vshort:sharper"]');
    await expect(smaller).toHaveAccessibleName(/^Smaller file: .+, 100 MB$/);
    await expect(sharper).toHaveAccessibleName(/^Sharper: 1080p/);
    // Down from Play: Smaller file, then Sharper.
    await expect.poll(() => focused(page)).toBe("versions:play");
    await page.keyboard.press("ArrowDown");
    await expect.poll(() => focused(page)).toBe("vshort:smaller");
    await page.keyboard.press("ArrowDown");
    await expect.poll(() => focused(page)).toBe("vshort:sharper");
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/play\//);
    expect(await page.evaluate(() => (history.state as { key?: string } | null)?.key)).toMatch(/#sharper$/);
    await page.context().close();
  });

  test("the sections are the core's, in its order, with their names; a shortcut equal to the pick is not drawn", async ({ browser }) => {
    const sections = (s: VersionsSnapshot): VersionsSnapshot => {
      const a = s.rows[0];
      if (!s.done || !a) return s;
      const rows = [a, { ...a, key: `${a.key}#1` }, { ...a, key: `${a.key}#2` }];
      return { ...s, rows, smaller: s.recommended, sections: [{ quality: "uhd", keys: rows.slice(1).map((r) => r.key) }, { quality: "other", keys: [a.key] }] };
    };
    const page = await device(browser, "desktop", PORTS.bridge, { versions: sections });
    await page.goto(path("desktop"));
    await firstRow(page);
    await expect(page.locator(".vgroup h3")).toHaveText([/^4K\s*\d+$/, /^Other\s*1$/]);
    await expect(page.locator('[data-group="other"] .vrow')).toHaveCount(1);
    await expect(page.locator('[data-f^="vshort:"]')).toHaveCount(0);
    await page.context().close();
  });

  test("a version in another language than the viewer's shows a chip naming it", async ({ browser }) => {
    const other = (s: VersionsSnapshot): VersionsSnapshot =>
      s.rows.length ? { ...s, rows: s.rows.map((r, i) => (i === 0 ? { ...r, group: "otherLanguages" as const, languages: ["spa"] } : r)) } : s;
    const page = await device(browser, "desktop", PORTS.bridge, { versions: other });
    await page.goto(path("desktop"));
    await firstRow(page);
    // One chip, on that row only, whatever its section.
    await expect(page.locator(".vrow .chips .lang")).toHaveText(["Spanish"]);
    await page.context().close();
  });

  test("a row is named by its version and described by its details: the release name is never in its name", async ({ browser }) => {
    const page = await device(browser, "desktop", PORTS.bridge);
    await page.goto(path("desktop"));
    const row = await firstRow(page);
    const release = await row.locator(".more").textContent();
    expect(release).toMatch(/\.webm/);
    await expect(row).not.toHaveAccessibleName(/\.webm/);
    await expect(row).toHaveAccessibleDescription(release ?? "");
    // One node per row for the reader: the list item holds the row button and nothing else.
    await expect(page.locator(".vslot").first()).toMatchAriaSnapshot(`
      - listitem:
        - button /^SD/
    `);
    await page.context().close();
  });

  test("nothing between a row and its list clips: the ring's outer pixel and the TV lift show whole", async ({ browser }) => {
    for (const form of ["tv", "desktop", "phone"] as const) {
      const page = await device(browser, form, PORTS.bridge);
      await page.goto(path(form));
      const row = await firstRow(page);
      // Focus from the keyboard, so the ring (or on TV the lift) is drawn.
      await page.keyboard.press("Shift");
      await row.focus();
      const clip = await row.evaluate((r) => {
        const s = getComputedStyle(r);
        // The ring's outer edge: its width plus its offset (none on TV, where the row lifts instead).
        const ring = s.outlineStyle === "none" ? 0 : Number.parseFloat(s.outlineWidth) + Number.parseFloat(s.outlineOffset);
        const b = r.getBoundingClientRect();
        const outer = { left: b.left - ring, right: b.right + ring, top: b.top - ring, bottom: b.bottom + ring };
        const clippers: string[] = [];
        let inside = true;
        for (let p = r.parentElement; p && p !== document.body; p = p.parentElement) {
          const c = getComputedStyle(p);
          const clips = c.overflowX !== "visible" || c.overflowY !== "visible" || c.contentVisibility === "auto" || /paint|strict|content/.test(c.contain);
          if (!clips) continue;
          clippers.push(p.className);
          const q = p.getBoundingClientRect();
          if (outer.left < q.left || outer.right > q.right) inside = false;
          // The list scrolls: its first row's ring may only meet its top and bottom when scrolled there.
          if (!p.classList.contains("alist") && (outer.top < q.top || outer.bottom > q.bottom)) inside = false;
        }
        return { ring, inside, clippers };
      });
      if (form === "desktop") expect(clip.ring).toBe(8);
      expect(clip.clippers.join(" ")).not.toMatch(/vslot|vgroup/);
      expect(clip.inside, `${form}: ${clip.clippers.join(", ")}`).toBe(true);
      // No breadcrumb and no column heads (plans/versions-web-spec.md 3.1 item 6), on every form.
      await expect(page.locator(".crumbs, nav[aria-label=Breadcrumb]")).toHaveCount(0);
      await expect(page.locator(".thead")).toHaveCount(0);
      await expect(page.locator(".versions-scr")).not.toContainText(/\bQuality\b.*\bFeatures\b/i);
      await page.context().close();
    }
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
