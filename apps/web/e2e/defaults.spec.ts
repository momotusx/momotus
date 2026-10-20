/** The recommended addons through the web screens, on a fresh core of its own
 * per walk (the shared core never gets them): nothing is installed before
 * Continue and switches turned off stay off; Browse first passes the
 * switches; a TV does it with keys only; a finished Stremio import leads to
 * the step, and a restart before Continue gets the Home card instead; a
 * pasted link that carries a sign-in is confirmed first. Stremio's sign-in
 * cannot run here, so the import view's answer is a finished import made up
 * at the socket, and the addon it brought is installed through the protocol;
 * everything else is the real core. Shots go to SHOTS (e2e/core.ts) at
 * desktop, phone and TV sizes. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import type { DefaultAddons } from "../src/core/generated/DefaultAddons";
import type { Prefs } from "../src/core/generated/Prefs";
import { Client, cleanup, DEFAULTS_PORT, PORTS, SHOTS, startBridge } from "./core";

type Form = "tv" | "desktop" | "phone";
const FORMS = {
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};
const OUT = SHOTS;
const CINEMETA = "https://v3-cinemeta.strem.io/manifest.json";
const IMPORT_SUB = 990_001;

let core: { proc: ChildProcess; data: string } | null = null;
let client: Client;
/** Every request the page sent its core, in order. */
const sent: Record<string, unknown>[] = [];
const errors: string[] = [];

test.beforeEach(async () => {
  sent.length = 0;
  errors.length = 0;
  core = await startBridge(DEFAULTS_PORT);
  client = new Client(`ws://127.0.0.1:${DEFAULTS_PORT}/ws`);
});

test.afterEach(async () => {
  client.close();
  if (core) cleanup(core.proc, core.data);
  core = null;
  expect(errors).toEqual([]);
});

/** A page whose socket reaches this walk's core. With `importDone` the import
 * view answers as a finished import. */
async function device(browser: Browser, form: Form, opts: { importDone?: boolean } = {}): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${DEFAULTS_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const msg = JSON.parse(String(m)) as Record<string, unknown>;
      sent.push(msg);
      if (opts.importDone && msg.op === "subscribe" && msg.view === "import") {
        const summary = { addonsImported: 1, addonsSkipped: [], titles: 0, inProgress: 0, watched: 0, seriesUnmatched: 0 };
        ws.send(JSON.stringify({ id: msg.id, ok: true, result: { sub: IMPORT_SUB, snapshot: { phase: "done", summary } } }));
        return;
      }
      if (msg.op === "unsubscribe" && msg.sub === IMPORT_SUB) {
        ws.send(JSON.stringify({ id: msg.id, ok: true, result: {} }));
        return;
      }
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => upstream.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  return page;
}

const shot = (page: Page, name: string, form: Form) =>
  page.waitForTimeout(500).then(() => page.screenshot({ path: `${OUT}/${name}-${form}.jpg`, type: "jpeg", quality: 80 }));
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const states = async () => Object.fromEntries((await client.req<DefaultAddons>("defaultAddons")).addons.map((a) => [a.key, a.installed]));
const allAbsent = async () => Object.values(await states()).every((s) => s === "absent");
/** Public addons sometimes miss the core's 10 s fetch limit; try thrice. */
async function install(url: string) {
  for (let attempt = 1; ; attempt++) {
    try {
      return await client.req("installAddon", { url });
    } catch (e) {
      if (attempt === 3) throw e;
    }
  }
}

test("Start fresh: nothing installed before Continue, two switches turned off stay off", async ({ browser }) => {
  const page = await device(browser, "desktop");
  // A profile that never had a list opens at the welcome steps.
  await page.goto("/?form=desktop");
  await expect(page).toHaveURL(/\/welcome/);
  await page.locator('[data-f="welcome:fresh"]').click();
  await expect(page.locator("h1")).toHaveText("Recommended addons");
  await expect(page.locator("button.drow")).toHaveCount(9);
  await expect(page.locator('button.drow[aria-checked="true"]')).toHaveCount(9);
  await shot(page, "defaults-step", "desktop");
  for (const key of ["torrentio", "trakt"]) await page.locator(`[data-f="welcome:default:${key}"]`).click();
  await expect(page.locator('[data-f="welcome:default:torrentio"]')).toHaveAttribute("aria-checked", "false");
  await expect(page.locator('[data-f="welcome:default:torrentio"] .dword')).toHaveText("Off");
  expect(await allAbsent()).toBe(true);
  expect(sent.some((m) => m.op === "installDefaults")).toBe(false);

  await page.locator('[data-f="welcome:next"]').click();
  // The one stream choice question, then the setup check.
  await expect(page).toHaveURL(/step=choice/);
  await expect(page.locator("h1")).toHaveText("How should Marquee pick what to play?");
  await shot(page, "stream-choice-question", "desktop");
  await page.locator('[data-f="welcome:best"]').click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).streams).toMatchObject({ sizeLimitBytes: null, minSeeders: 20 });
  const choices = sent.find((m) => m.op === "installDefaults")?.choices as { key: string; on: boolean }[];
  expect(choices).toHaveLength(9);
  expect(choices.filter((c) => !c.on).map((c) => c.key)).toEqual(["torrentio", "trakt"]);
  expect(await states()).toMatchObject({ cinemeta: "on", torrentio: "off", trakt: "off", kitsu: "on", opensubtitles: "on" });
  // Torrentio is off, so the setup check offers it through the core's defaults.
  await expect(page.locator('[data-f="check:Sources"]')).toHaveText("Add Torrentio");

  // Settings shows the same switches, the two still off.
  await page.goto("/recommended-addons?form=desktop");
  await expect(page.locator('[data-f="defaults:torrentio"]')).toHaveAttribute("aria-checked", "false");
  await expect(page.locator('[data-f="defaults:trakt"]')).toHaveAttribute("aria-checked", "false");
  await expect(page.locator('[data-f="defaults:cinemeta"]')).toHaveAttribute("aria-checked", "true");
  await shot(page, "defaults-settings-fresh", "desktop");

  // Answered here: Home opens and offers no card.
  const asked = sent.filter((m) => m.op === "defaultAddons").length;
  await page.goto("/?form=desktop");
  await expect.poll(() => sent.filter((m) => m.op === "defaultAddons").length).toBeGreaterThan(asked);
  await page.waitForTimeout(500);
  await expect(page).not.toHaveURL(/welcome/);
  await expect(page.locator(".defaults-card")).toHaveCount(0);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).defaultsSeen).toBe(1);
});

test("Browse first passes the switches, then opens Home", async ({ browser }) => {
  const page = await device(browser, "phone");
  await page.goto("/?form=phone");
  await expect(page).toHaveURL(/\/welcome/);
  await page.locator('[data-f="welcome:skip"]').click();
  await expect(page).toHaveURL(/step=2&then=home/);
  await expect(page.locator("button.drow")).toHaveCount(9);
  expect(await allAbsent()).toBe(true);
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT}/defaults-step-phone.jpg`, type: "jpeg", quality: 80, fullPage: true });
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/step=choice&then=home/);
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT}/stream-choice-question-phone.jpg`, type: "jpeg", quality: 80, fullPage: true });
  // Skip writes nothing: the defaults are Start fast's.
  await page.locator('[data-f="welcome:skip"]').click();
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
  expect(Object.values(await states()).every((s) => s === "on")).toBe(true);
  expect(sent.some((m) => m.op === "setPrefs" && JSON.stringify(m).includes("streams"))).toBe(false);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).streams).toMatchObject({ sizeLimitBytes: 5 * 1024 ** 3, minSeeders: 50 });
});

test("a TV does it with keys only: Continue first, a row toggles with select", async ({ browser }) => {
  const page = await device(browser, "tv");
  await page.goto("/?form=tv");
  await expect(page).toHaveURL(/\/welcome/);
  await expect.poll(() => focused(page)).toBe("welcome:stremio");
  await page.keyboard.press("ArrowRight");
  await expect.poll(() => focused(page)).toBe("welcome:fresh");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/step=2/);
  // One press accepts all: focus starts on Continue. No text field on a TV.
  await expect.poll(() => focused(page)).toBe("welcome:next");
  await expect(page.locator('[data-f="welcome:url"]')).toHaveCount(0);
  await expect(page.locator(".step-body .aside")).toHaveText("Add more later from your phone or computer.");
  await shot(page, "defaults-step", "tv");
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => focused(page)).toMatch(/^welcome:default:/);
  const row = (await focused(page)) as string;
  await page.keyboard.press("Enter");
  await expect(page.locator(`[data-f="${row}"]`)).toHaveAttribute("aria-checked", "false");
  for (let i = 0; i < 8 && (await focused(page)) !== "welcome:next"; i++) await page.keyboard.press("ArrowDown");
  await expect.poll(() => focused(page)).toBe("welcome:next");
  await page.keyboard.press("Enter");
  // Start fast is focused: one press takes it.
  await expect(page).toHaveURL(/step=choice/);
  await expect.poll(() => focused(page)).toBe("welcome:fast");
  // On a TV the two answers sit in the middle of the screen, not its top half.
  const mid = await page.locator(".doors.two").evaluate((d) => {
    const r = d.getBoundingClientRect();
    return (r.top + r.bottom) / 2 / innerHeight;
  });
  expect(mid).toBeGreaterThan(0.45);
  expect(mid).toBeLessThan(0.65);
  await shot(page, "stream-choice-question", "tv");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/step=3/);
  expect(sent.find((m) => m.op === "setPrefs" && JSON.stringify(m).includes("streams"))).toMatchObject({ patch: { streams: { sizeLimitBytes: 5 * 1024 ** 3, minSeeders: 50 } } });
  const now = await states();
  const key = row.split(":")[2] as string;
  expect(now[key]).toBe("off");
  expect(Object.entries(now).filter(([, s]) => s !== "on")).toEqual([[key, "off"]]);
});

test("a finished import leads to the step, and a restart before Continue gets the Home card", async ({ browser }) => {
  const page = await device(browser, "desktop", { importDone: true });
  await page.goto("/welcome?form=desktop");
  await page.locator('[data-f="welcome:stremio"]').click();
  await expect(page).toHaveURL(/\/import\?from=welcome/);
  // What the import wrote: Stremio's Cinemeta.
  await install(CINEMETA);
  await page.locator('[data-f="import:next"]').click();
  await expect(page).toHaveURL(/step=2&from=import/);
  await expect(page.locator(".drow.fixed", { hasText: "Cinemeta" })).toContainText("From Stremio");
  await expect(page.locator("button.drow")).toHaveCount(8);
  await shot(page, "defaults-after-import", "desktop");

  // The app restarts before Continue: the profile has a list now, so Home
  // opens (not the welcome steps) and offers the missing ones once.
  await page.goto("/?form=desktop");
  const card = page.locator(".defaults-card");
  await expect(card).toBeVisible();
  await expect(card.locator("h2")).toHaveText("New recommended addons");
  await expect(card.locator("p")).toContainText("Torrentio");
  await expect(card.locator("p")).not.toContainText("Cinemeta");
  await expect(page).not.toHaveURL(/welcome/);
  await shot(page, "defaults-card", "desktop");
  await page.locator('[data-f="home:defaults:review"]').click();
  await expect(page).toHaveURL(/step=2&review=1/);
  await expect(page.locator("button.drow")).toHaveCount(8);
  await expect(page.locator(".drow.fixed")).toHaveCount(0);
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
  await expect(card).toHaveCount(0);
  expect(Object.values(await states()).every((s) => s === "on")).toBe(true);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).defaultsSeen).toBe(1);
});

test("the Home card at TV and phone sizes; Not now closes it for good", async ({ browser }) => {
  await install(CINEMETA);
  for (const form of ["tv", "phone"] as const) {
    const page = await device(browser, form);
    await page.goto(`/?form=${form}`);
    await expect(page.locator(".defaults-card")).toBeVisible();
    await shot(page, "defaults-card", form);
    await page.context().close();
  }
  const page = await device(browser, "desktop");
  await page.goto("/?form=desktop");
  await page.locator('[data-f="home:defaults:later"]').click();
  await expect(page.locator(".defaults-card")).toHaveCount(0);
  await page.reload();
  await expect.poll(() => sent.filter((m) => m.op === "defaultAddons").length).toBeGreaterThan(1);
  await page.waitForTimeout(500);
  await expect(page.locator(".defaults-card")).toHaveCount(0);
  // Nothing was installed.
  expect(Object.entries(await states()).filter(([, s]) => s !== "absent")).toEqual([["cinemeta", "on"]]);
});

test("a pasted link that carries a sign-in is confirmed first, then sent again acknowledged", async ({ browser }) => {
  const page = await device(browser, "desktop");
  await page.goto("/addons?form=desktop");
  await page.locator('[data-f="install:url"]').fill("https://addon.example/manifest.json?api_key=K");
  await page.locator('[data-f="install:go"]').click();
  await expect(page.locator(".install-state")).toContainText("carries a sign-in");
  const ack = page.locator('[data-f="install:ack"]');
  await expect(ack).toHaveText("It is mine, install");
  // The risky choice is no white primary: glass with the warning's edge, and
  // nothing on the screen is primary while the warning stands.
  await expect(ack).not.toHaveClass(/\bprimary\b/);
  expect(await ack.evaluate((b) => getComputedStyle(b).boxShadow)).toContain("rgb(245, 197, 24)");
  await expect(page.locator(".btn.primary")).toHaveCount(0);
  await shot(page, "addons-token", "desktop");
  await ack.click();
  await expect
    .poll(() => sent.filter((m) => m.op === "installAddon").map((m) => m.acknowledgeToken === true))
    .toEqual([false, true]);
  // Acknowledged, the core tries the made-up host and says why it failed.
  await expect(page.locator(".install-state")).toContainText("Could not install");
});

test("Settings by keys: a switch keeps focus on its own row while the list reloads", async ({ browser }) => {
  const page = await device(browser, "tv");
  await page.goto("/recommended-addons?form=tv");
  for (const key of ["trakt", "cinemeta"]) {
    const row = page.locator(`[data-f="defaults:${key}"]`);
    await expect(row).toBeVisible({ timeout: 30_000 });
    await row.focus();
    // A row the reload rebuilt would come back without this mark.
    await row.evaluate((el) => (el.dataset.kept = "yes"));
    await page.keyboard.press("Enter");
    await expect.poll(async () => (await states())[key], { timeout: 30_000 }).toBe("on");
    await expect(row).toHaveAttribute("aria-checked", "true");
    const active = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      return { f: el?.dataset.f, kept: el?.dataset.kept };
    });
    expect(active, `focus stays on ${key}`).toEqual({ f: `defaults:${key}`, kept: "yes" });
  }
});

test("after an import, Back from the setup check does not call Marquee's own installs From Stremio", async ({ browser }) => {
  const page = await device(browser, "desktop", { importDone: true });
  await page.goto("/welcome?form=desktop");
  await page.locator('[data-f="welcome:stremio"]').click();
  await expect(page).toHaveURL(/\/import\?from=welcome/);
  await install(CINEMETA);
  await page.locator('[data-f="import:next"]').click();
  await expect(page).toHaveURL(/step=2&from=import/);
  await expect(page.locator(".drow.fixed", { hasText: "Cinemeta" })).toContainText("From Stremio");
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/step=choice/);
  await page.locator('[data-f="welcome:fast"]').click();
  await expect(page).toHaveURL(/step=3/);
  await page.goBack();
  await expect(page).toHaveURL(/step=choice/);
  await page.goBack();
  await expect(page).toHaveURL(/step=2/);
  await expect(page).not.toHaveURL(/from=import/);
  await expect(page.locator(".drow.fixed", { hasText: "Torrentio" })).toContainText("Installed");
  await expect(page.locator(".drow.fixed", { hasText: "From Stremio" })).toHaveCount(0);
});

test("an empty Home offers Set up Marquee alone, not the defaults card beside it", async ({ browser }) => {
  // A list with no catalog addon: Home is empty, and the profile is no longer fresh.
  await install("https://torrentio.strem.fun/manifest.json");
  const page = await device(browser, "desktop");
  await page.goto("/?form=desktop");
  await expect(page.locator('[data-f="home:setup"]')).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  await expect(page.locator(".defaults-card")).toHaveCount(0);
});

test("the built-in TurkceAltyazi.org row says where it runs instead of a host", async ({ browser }) => {
  // Bundled manifests: nothing is fetched to install them.
  await client.req("installDefaults", {
    choices: [
      { key: "cinemeta", on: true },
      { key: "turkcealtyazi", on: true },
    ],
  });
  await client.req("setPrefs", { patch: { defaultsSeen: 1 } });
  const page = await device(browser, "desktop");
  await page.goto("/addons?form=desktop");
  const builtIn = page.locator("li.addon", { hasText: "TurkceAltyazi" });
  await expect(builtIn.locator(".host")).toHaveText("Built into Marquee, runs on this device");
  // Never configurable: its old host is never opened.
  await expect(builtIn).not.toContainText("Settings live on its website");
  await expect(page.locator("li.addon", { hasText: "Cinemeta" }).locator(".host")).toHaveText("v3-cinemeta.strem.io");
  await shot(page, "addons-built-in", "desktop");
  // Settings credits the site once, under About.
  await page.goto("/settings?form=desktop");
  await expect(page.locator("section.set", { hasText: "About" })).toContainText(
    "Turkish subtitles from TurkceAltyazi.org, fetched by this device when you play.",
  );
});
