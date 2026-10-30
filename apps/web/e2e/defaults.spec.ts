/** The recommended addons through the web screens, on a fresh core of its own
 * per walk (the shared core never gets them): nothing is installed before
 * Continue and switches turned off stay off; Browse first passes the
 * switches; a TV does it with keys only; the subtitles step after the
 * question writes the profile's language; a finished Stremio import leads to
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
 * view answers as a finished import; `languages` holds the language list
 * back that long, or answers it with an error ("fail"). */
async function device(
  browser: Browser,
  form: Form,
  opts: { importDone?: boolean; locale?: string; slowSubtitles?: number; languages?: number | "fail"; slowQuery?: { query: string; ms: number } } = {},
): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], locale: opts.locale, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
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
      if (opts.languages === "fail" && msg.op === "languagePicker") {
        ws.send(JSON.stringify({ id: msg.id, ok: false, error: "the core is busy" }));
        return;
      }
      // One query's answer held back, so it arrives after a later one.
      if (opts.slowQuery && msg.op === "languagePicker" && msg.query === opts.slowQuery.query) {
        setTimeout(() => upstream.send(String(m)), opts.slowQuery.ms);
        return;
      }
      if (typeof opts.languages === "number" && msg.op === "languagePicker") {
        setTimeout(() => upstream.send(String(m)), opts.languages);
        return;
      }
      // A slow save of the subtitle language, for Skip while it runs.
      if (opts.slowSubtitles && msg.op === "setPrefs" && JSON.stringify(msg).includes("subtitleLanguages")) {
        setTimeout(() => upstream.send(String(m)), opts.slowSubtitles);
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

/** The subtitles step's selector opens the picker over the step (V1: the step shows the choice alone). */
async function openPicker(page: Page) {
  await page.locator('[data-f="welcome:lang:open"]:not([aria-disabled="true"])').click();
  await expect(page.locator(".lp-layer")).toHaveCount(1);
}
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

/** The app's Back on Home goes nowhere: no welcome step shows, even for a
 * moment (one that sends the profile on to Home again would hide it). */
async function backStaysHome(page: Page) {
  await page.evaluate(() => {
    const w = window as unknown as { sawWelcome: boolean };
    w.sawWelcome = false;
    new MutationObserver(() => {
      if (document.querySelector(".welcome")) w.sawWelcome = true;
    }).observe(document.body, { childList: true, subtree: true });
  });
  const at = page.url();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(800);
  expect(page.url()).toBe(at);
  expect(await page.evaluate(() => (window as unknown as { sawWelcome: boolean }).sawWelcome)).toBe(false);
}

test("Start fresh: nothing installed before Continue, two switches turned off stay off", async ({ browser }) => {
  const page = await device(browser, "desktop");
  // A profile that never had a list opens at the welcome steps.
  await page.goto("/?form=desktop");
  await expect(page).toHaveURL(/\/welcome/);
  await page.locator('[data-f="welcome:fresh"]').click();
  await expect(page.locator("h1")).toHaveText("Recommended addons");
  await expect(page.locator("button.drow")).toHaveCount(11);
  await expect(page.locator('button.drow[aria-checked="true"]')).toHaveCount(11);
  await shot(page, "defaults-step", "desktop");
  for (const key of ["torrentio", "trakt"]) await page.locator(`[data-f="welcome:default:${key}"]`).click();
  await expect(page.locator('[data-f="welcome:default:torrentio"]')).toHaveAttribute("aria-checked", "false");
  await expect(page.locator('[data-f="welcome:default:torrentio"] .dword')).toHaveText("Off");
  expect(await allAbsent()).toBe(true);
  expect(sent.some((m) => m.op === "installDefaults")).toBe(false);

  await page.locator('[data-f="welcome:next"]').click();
  // The one stream choice question, then the setup check.
  await expect(page).toHaveURL(/step=choice/);
  await expect(page.locator("h1")).toHaveText("How should Momotus pick what to play?");
  await shot(page, "stream-choice-question", "desktop");
  await page.locator('[data-f="welcome:best"]').click();
  // Then the subtitle language: on an English browser No subtitles leads and
  // is chosen (most titles are in English), English right after it.
  await expect(page).toHaveURL(/step=subs/);
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("No subtitles");
  await openPicker(page);
  const suggested = page.locator('[data-fgroup="welcome:lang-s"] [data-f]');
  await expect(suggested.first()).toHaveAttribute("data-f", "welcome:lang:s:none");
  await expect(suggested.first()).toHaveAttribute("aria-selected", "true");
  await expect(suggested.nth(1)).toHaveAttribute("data-f", "welcome:lang:s:en");
  await expect(suggested.nth(1)).toHaveAttribute("aria-selected", "false");
  await page.locator('[data-f="lp:done"]').click();
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).streams).toMatchObject({ sizeLimitBytes: null, minSeeders: 20 });
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual([]);
  const choices = sent.find((m) => m.op === "installDefaults")?.choices as { key: string; on: boolean }[];
  expect(choices).toHaveLength(11);
  expect(choices.filter((c) => !c.on).map((c) => c.key)).toEqual(["torrentio", "trakt"]);
  expect(await states()).toMatchObject({ cinemeta: "on", torrentio: "off", trakt: "off", kitsu: "on", opensubtitles: "on" });
  // Torrentio is off, so the setup check offers it through the core's defaults.
  await expect(page.locator('[data-f="check:Sources"]')).toHaveText("Add Torrentio");
  // Go to Home leaves the steps behind: Back on Home stays on Home, never
  // back into the setup check or the question.
  await page.locator('[data-f="welcome:home"]').click();
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
  await backStaysHome(page);

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

test("Browse first passes the switches, then opens Home as the root", async ({ browser }) => {
  const page = await device(browser, "phone");
  await page.goto("/?form=phone");
  await expect(page).toHaveURL(/\/welcome/);
  await page.locator('[data-f="welcome:skip"]').click();
  await expect(page).toHaveURL(/step=2&then=home/);
  await expect(page.locator("button.drow")).toHaveCount(11);
  expect(await allAbsent()).toBe(true);
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${OUT}/defaults-step-phone.jpg`, type: "jpeg", quality: 80, fullPage: true });
  // No question on the way: its defaults are Start fast's, and nothing is written.
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
  await backStaysHome(page);
  expect(Object.values(await states()).every((s) => s === "on")).toBe(true);
  expect(sent.some((m) => m.op === "setPrefs" && JSON.stringify(m).includes("streams"))).toBe(false);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).streams).toMatchObject({ sizeLimitBytes: 5 * 1024 ** 3, minSeeders: 50 });
});

test("a TV's first press on Welcome is never undone by the screen's late first focus", async ({ browser }) => {
  const page = await device(browser, "tv");
  // The viewer presses Down (the doors stack, V1) the moment the first door has the focus: before
  // the screen's own first focus, asked for when the route changed and run a
  // frame later, has run. Under load that frame is late (the flake this
  // replaces: focus jumped back to I use Stremio).
  await page.addInitScript(() => {
    let pressed = false;
    document.addEventListener(
      "focusin",
      (e) => {
        if (pressed || (e.target as HTMLElement).dataset?.f !== "welcome:stremio") return;
        pressed = true;
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
      },
      true,
    );
  });
  await page.goto("/?form=tv");
  await expect(page).toHaveURL(/\/welcome/);
  await expect.poll(() => focused(page)).toBe("welcome:fresh");
  // Frames later, the focus is still where the viewer put it.
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 500)))));
  expect(await focused(page)).toBe("welcome:fresh");
});

test("a TV does it with keys only: Continue first, a row toggles with select", async ({ browser }) => {
  const page = await device(browser, "tv");
  await page.goto("/?form=tv");
  await expect(page).toHaveURL(/\/welcome/);
  await expect.poll(() => focused(page)).toBe("welcome:stremio");
  // The doors stack in a column (V1): the next one is down.
  await page.keyboard.press("ArrowDown");
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
  // On a TV the question and its two answers sit in a column at the lower left, over the still (V1).
  const at = await page.locator(".framed").evaluate((s) => {
    const h1 = (s.querySelector("h1") as HTMLElement).getBoundingClientRect();
    const doors = (s.querySelector(".doors.two") as HTMLElement).getBoundingClientRect();
    return { top: h1.top / innerHeight, bottom: doors.bottom / innerHeight, right: doors.right / innerWidth };
  });
  expect(at.top).toBeGreaterThan(0.3);
  expect(at.bottom).toBeGreaterThan(0.8);
  expect(at.bottom).toBeLessThanOrEqual(1);
  expect(at.right).toBeLessThan(0.62);
  await shot(page, "stream-choice-question", "tv");
  await page.keyboard.press("Enter");
  // The subtitle language: one press on Continue keeps this TV's (No subtitles on an English TV).
  await expect(page).toHaveURL(/step=subs/);
  await expect.poll(() => focused(page)).toBe("welcome:next");
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("No subtitles");
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
  await expect(page.locator("button.drow")).toHaveCount(10);
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
  await expect(page.locator("button.drow")).toHaveCount(10);
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
  // The risky choice is no white primary: the plain secondary, its words
  // carrying the risk, and nothing on the screen is primary while the warning stands.
  await expect(ack).not.toHaveClass(/\bprimary\b/);
  expect(await ack.evaluate((b) => getComputedStyle(b).backgroundColor)).not.toBe("rgba(255, 255, 255, 0.92)");
  await expect(page.locator(".btn.primary")).toHaveCount(0);
  await shot(page, "addons-token", "desktop");
  await ack.click();
  await expect
    .poll(() => sent.filter((m) => m.op === "previewAddon").map((m) => m.acknowledgeToken === true))
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

test("Subtitles: the device's language is chosen; the pick is the profile's; a profile with one passes by", async ({ browser }) => {
  const languages = async () => (await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages;
  // A Turkish phone: the step shows the choice alone, Turkish; the list opens from it.
  const phone = await device(browser, "phone", { locale: "tr-TR" });
  await phone.goto("/welcome?step=subs&form=phone");
  await expect(phone.locator("h1")).toHaveText("Subtitles");
  const selector = phone.locator('[data-f="welcome:lang:open"]');
  await expect(selector).toContainText("Türkçe");
  await expect(selector).toContainText("Turkish");
  await expect(phone.locator('[data-fgroup="welcome:lang-r"]')).toHaveCount(0);
  await phone.waitForTimeout(500);
  await phone.screenshot({ path: `${OUT}/subtitles-step-phone.jpg`, type: "jpeg", quality: 80 });
  await selector.click();
  await expect(phone.locator('[data-f="welcome:lang:field"]')).toBeFocused();
  const chips = phone.locator('[data-fgroup="welcome:lang-s"] [data-f]');
  await expect(chips.first()).toHaveAttribute("data-f", "welcome:lang:s:tr");
  await expect(chips.first()).toHaveAttribute("aria-selected", "true");
  await expect(chips.last()).toHaveAttribute("data-f", "welcome:lang:s:none");
  await expect(phone.locator('[data-fgroup="welcome:lang-r"] [data-f]')).toHaveCount(113);
  await phone.waitForTimeout(500);
  await phone.screenshot({ path: `${OUT}/subtitles-picker-phone.jpg`, type: "jpeg", quality: 80 });
  await phone.locator('[data-f="welcome:lang:s:none"]').click();
  await expect(phone.locator(".lp-layer")).toHaveCount(0);
  await expect(selector).toContainText("No subtitles");
  await phone.locator('[data-f="welcome:next"]').click();
  await expect(phone).toHaveURL(/step=3/);
  expect(await languages()).toEqual([]);

  // Desktop: the picker over the step, the field focused; "farsi" finds Persian.
  const desktop = await device(browser, "desktop", { locale: "tr-TR" });
  await desktop.goto("/welcome?step=subs&form=desktop");
  await expect(desktop.locator('[data-f="welcome:lang:open"]')).toContainText("Türkçe");
  await shot(desktop, "subtitles-step", "desktop");
  await openPicker(desktop);
  await expect(desktop.locator('[data-f="welcome:lang:field"]')).toBeFocused();
  await expect(desktop.locator('[data-fgroup="welcome:lang-r"] [data-f]')).toHaveCount(113);
  await shot(desktop, "subtitles-picker", "desktop");
  // Letters typed with a row focused go to the field; Backspace there deletes, it is not Back.
  await desktop.locator('[data-f="welcome:lang:r:de"]').focus();
  await desktop.keyboard.press("q");
  await expect(desktop.locator('[data-f="welcome:lang:field"]')).toBeFocused();
  await expect(desktop.locator('[data-f="welcome:lang:field"]')).toHaveValue("q");
  // "q" finds Kazakh (its own name, Qazaq tili): a row of it takes the focus, then Backspace.
  await desktop.locator('[data-f="welcome:lang:r:kk"]').focus();
  await desktop.keyboard.press("Backspace");
  await expect(desktop.locator('[data-f="welcome:lang:field"]')).toHaveValue("");
  await expect(desktop).toHaveURL(/step=subs/);
  await desktop.locator('[data-f="welcome:lang:field"]').focus();
  await desktop.keyboard.type("farsi");
  await expect(desktop.locator('[data-fgroup="welcome:lang-r"] [data-f]').first()).toHaveAttribute("data-f", "welcome:lang:r:fa");
  await desktop.locator('[data-f="welcome:lang:r:fa"]').click();
  await expect(desktop.locator(".lp-layer")).toHaveCount(0);
  await expect(desktop.locator('[data-f="welcome:lang:open"]')).toContainText("Persian");
  await desktop.locator('[data-f="welcome:next"]').click();
  await expect(desktop).toHaveURL(/step=3/);
  expect(await languages()).toEqual(["fa"]);

  // A profile with a language passes by, to the next step.
  await desktop.goto("/welcome?step=subs&form=desktop");
  await expect(desktop).toHaveURL(/step=3/);
  await expect(desktop.locator("h1")).toHaveText("Setup check");
  expect(await languages()).toEqual(["fa"]);

  // A TV: Continue has the focus, so one press keeps the pre-chosen language. Up is the selector; in its picker
  // the pre-chosen language has the focus, Up reaches the drawn keyboard, keys type, Back clears the query first.
  await client.req("setPrefs", { patch: { subtitleLanguages: [] } });
  const tv = await device(browser, "tv", { locale: "tr-TR" });
  await tv.goto("/welcome?step=subs&form=tv");
  await expect.poll(() => focused(tv)).toBe("welcome:next");
  await shot(tv, "subtitles-step", "tv");
  await tv.keyboard.press("ArrowUp");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:open");
  await tv.keyboard.press("Enter");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await tv.keyboard.press("ArrowUp");
  await expect.poll(() => focused(tv)).toMatch(/^welcome:lang:kb:/);
  // Left and Right stop at the keyboard's ends.
  for (let i = 0; i < 30; i++) await tv.keyboard.press("ArrowLeft");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:space");
  await tv.keyboard.type("russk");
  await expect(tv.locator('[data-fgroup="welcome:lang-r"] [data-f]').first()).toHaveAttribute("data-f", "welcome:lang:r:ru");
  await tv.keyboard.press("Backspace");
  await expect(tv.locator(".lp-field input")).toHaveValue("russ");
  await tv.keyboard.press("Escape");
  await expect(tv.locator(".lp-field input")).toHaveValue("");
  await expect(tv).toHaveURL(/step=subs/);
  await tv.keyboard.press("ArrowDown");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await tv.keyboard.press("Enter");
  await expect(tv).toHaveURL(/step=3/);
  expect(await languages()).toEqual(["tr"]);
});

test("Subtitles: a device language reads as the one the core lists", async ({ browser }) => {
  // Norway says Bokmål ("nb"); the core lists Norwegian as "no".
  const page = await device(browser, "desktop", { locale: "nb-NO" });
  await page.goto("/welcome?step=subs&form=desktop");
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("Norsk");
  await openPicker(page);
  const first = page.locator('[data-fgroup="welcome:lang-s"] [data-f]').first();
  await expect(first).toHaveAttribute("data-f", "welcome:lang:s:no");
  await expect(first).toHaveAttribute("aria-selected", "true");
  await expect(first).toContainText("Norsk");
  await expect(page.locator('[data-f="welcome:lang:s:nb"]')).toHaveCount(0);
  await page.locator('[data-f="lp:done"]').click();
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual(["no"]);
});

test("Subtitles: Continue waits for the language list, then keeps the device's language", async ({ browser }) => {
  const page = await device(browser, "desktop", { locale: "de-DE", languages: 2000 });
  await page.goto("/welcome?step=subs&form=desktop");
  const next = page.locator('[data-f="welcome:next"]');
  await expect(next).toHaveAttribute("aria-disabled", "true");
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("Loading languages");
  // A press before the list goes nowhere.
  await next.dispatchEvent("click");
  await expect(page).toHaveURL(/step=subs/);
  await expect(next).toHaveAttribute("aria-disabled", "false", { timeout: 10_000 });
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("Deutsch");
  await next.click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual(["de"]);
});

test("Subtitles: an older query's answer that arrives last changes nothing", async ({ browser }) => {
  // "a" lists Abkhazian first; its answer is held back until after "ar"'s, which lists Arabic first.
  const page = await device(browser, "desktop", { locale: "de-DE", slowQuery: { query: "a", ms: 1500 } });
  await page.goto("/welcome?step=subs&form=desktop");
  await openPicker(page);
  await expect(page.locator('[data-f="welcome:lang:field"]')).toBeFocused();
  await page.keyboard.type("ar");
  const first = page.locator('[data-fgroup="welcome:lang-r"] [data-f]').first();
  await expect(first).toHaveAttribute("data-f", "welcome:lang:r:ar");
  await page.waitForTimeout(2000);
  await expect(first).toHaveAttribute("data-f", "welcome:lang:r:ar");
  await expect(page.locator('[data-f="welcome:lang:field"]')).toHaveValue("ar");
});

test("Subtitles picker on TV: Space never picks, Up to the remembered key, Back and Clear hand the focus on", async ({ browser }) => {
  const tv = await device(browser, "tv", { locale: "tr-TR" });
  await tv.goto("/welcome?step=subs&form=tv");
  await expect.poll(() => focused(tv)).toBe("welcome:next");
  await openPicker(tv);
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  const query = tv.locator(".lp-field input");
  // A Space on a language types (nothing, with no query) and never presses it.
  await tv.keyboard.press(" ");
  await tv.waitForTimeout(300);
  await expect(tv).toHaveURL(/step=subs/);
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  // Up reaches the keyboard on its first remembered key, "a"; Right moves to "d".
  await tv.keyboard.press("ArrowUp");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:a");
  for (let i = 0; i < 3; i++) await tv.keyboard.press("ArrowRight");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:d");
  // Typed with the keys of a keyboard, the focus stays on the drawn key.
  await tv.keyboard.type("a");
  await expect(query).toHaveValue("a");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:d");
  // "a"'s answer is drawn (the count follows it), not the whole list still on screen.
  await expect(tv.locator(".lp-count")).toContainText("found");
  const first = tv.locator('[data-fgroup="welcome:lang-r"] [data-f]').first();
  const firstKey = await first.getAttribute("data-f");
  await tv.keyboard.press("ArrowDown");
  await expect.poll(() => focused(tv)).toBe(firstKey);
  // Up from any language of the first row is the remembered key, not the nearest one.
  await tv.keyboard.press("ArrowRight");
  await expect.poll(() => focused(tv)).not.toBe(firstKey);
  await tv.keyboard.press("ArrowUp");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:d");
  // Backspace on a language edits the query, never Back: the list changes, the focus goes to the first suggestion.
  await tv.keyboard.press("ArrowDown");
  await tv.keyboard.press("Backspace");
  await expect(query).toHaveValue("");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await expect(tv).toHaveURL(/step=subs/);
  // Typed from a suggestion: the focus goes to the new list's first language.
  await tv.keyboard.type("a");
  await expect.poll(() => focused(tv)).toBe(firstKey);
  // Back with the focus on a language: the query goes, the focus goes to the first suggestion, the step stays.
  await tv.keyboard.press("Escape");
  await expect(query).toHaveValue("");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await expect(tv).toHaveURL(/step=subs/);
  // Typing that finds nothing, from a language: the focus goes to Clear, and Clear hands it to the key.
  await tv.keyboard.type("zzzz");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:clear");
  await tv.keyboard.press("Enter");
  await expect(query).toHaveValue("");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:kb:d");
});

test("Subtitles picker on desktop: one Tab stop per list, the directory's columns, the spoken words", async ({ browser }) => {
  const page = await device(browser, "desktop", { locale: "tr-TR" });
  await page.goto("/welcome?step=subs&form=desktop");
  await openPicker(page);
  const field = page.locator('[data-f="welcome:lang:field"]');
  await expect(field).toBeFocused();
  const dir = page.locator('[data-fgroup="welcome:lang-r"]');
  await expect(dir.locator("[data-f]")).toHaveCount(113);
  await expect(dir).toHaveAttribute("role", "listbox");
  await expect(dir.locator('[role="group"]').first()).toHaveAttribute("aria-label", "A");
  await expect(page.locator('[data-fgroup="welcome:lang-s"]')).toHaveAttribute("role", "listbox");
  // The English name is spoken first, the own name in its own language; flags load as they scroll in.
  const turkish = page.locator('[data-f="welcome:lang:r:tr"]');
  await expect(turkish).toHaveAccessibleName(/^Turkish,\s*Türkçe$/);
  await expect(turkish.locator('[lang="tr"]')).toHaveText("Türkçe");
  await expect(turkish.locator("img")).toHaveAttribute("loading", "lazy");
  // Tab: the field, Suggested, the directory; the page behind the picker takes none.
  await page.keyboard.press("Tab");
  expect(await focused(page)).toBe("welcome:lang:s:tr");
  await page.keyboard.press("Tab");
  expect(await focused(page)).toMatch(/^welcome:lang:r:/);
  await page.keyboard.press("Tab");
  expect(await focused(page)).not.toBe("welcome:next");
  // Down at a column's foot goes to the next column's top, Up comes back.
  const [end, top] = await page.evaluate(() => {
    const rows = [...document.querySelectorAll<HTMLElement>(".lp-dir [data-f]")];
    const i = rows.findIndex((r, n) => n + 1 < rows.length && (rows[n + 1] as HTMLElement).getBoundingClientRect().left > r.getBoundingClientRect().left + 50);
    return [rows[i]?.dataset.f, rows[i + 1]?.dataset.f];
  });
  expect(end).toBeTruthy();
  await page.locator(`[data-f="${end}"]`).focus();
  await page.keyboard.press("ArrowDown");
  expect(await focused(page)).toBe(top);
  await page.keyboard.press("ArrowUp");
  expect(await focused(page)).toBe(end);
  // The count is spoken once the typing stops.
  await field.focus();
  const spoken = page.locator(".lp-count .sr");
  await page.keyboard.type("farsi");
  expect(await spoken.textContent()).toBe("");
  await expect(spoken).toHaveText("1 found");
});

test("Subtitles picker on the phone: the selector speaks its label, Back and Cancel close the list, not the step", async ({ browser }) => {
  const phone = await device(browser, "phone", { locale: "tr-TR" });
  await phone.goto("/welcome?step=subs&form=phone");
  const selector = phone.locator('[data-f="welcome:lang:open"]');
  await expect(selector).toHaveAccessibleName(/^Subtitle language\s*Türkçe\s*Turkish$/);
  await expect(selector).toHaveAttribute("aria-haspopup", "dialog");
  await expect(phone.locator(".lp-hint")).toHaveText("Tap to search 113 languages");
  // The browser's Back with a query typed closes the list.
  await selector.click();
  await phone.locator('[data-f="welcome:lang:field"]').fill("a");
  await phone.goBack();
  await expect(phone.locator(".lp-layer")).toHaveCount(0);
  await expect(phone).toHaveURL(/step=subs/);
  // Cancel closes it with the choice unchanged.
  await selector.click();
  await phone.locator('[data-f="lp:done"]').click();
  await expect(phone.locator(".lp-layer")).toHaveCount(0);
  await expect(selector).toContainText("Türkçe");
  await expect(phone).toHaveURL(/step=subs/);
});

test("Settings on TV: OK on a language applies the rule and the focus stays on it", async ({ browser }) => {
  await client.req("setPrefs", { patch: { subtitleLanguages: ["en"] } });
  const tv = await device(browser, "tv", { locale: "tr-TR" });
  await tv.goto("/settings/playback?form=tv");
  const open = tv.locator('[data-f="pref-subs:open"]');
  await expect(open).toBeVisible({ timeout: 30_000 });
  await open.focus();
  await tv.keyboard.press("Enter");
  await expect(tv.locator('[data-fgroup="pref-subs-s"]')).toHaveAttribute("aria-multiselectable", "true");
  await expect.poll(() => focused(tv)).toMatch(/^pref-subs:s:/);
  // Turkish, this TV's language, is added first: the most recent pick is the default.
  const key = "pref-subs:s:tr";
  await tv.locator(`[data-f="${key}"]`).focus();
  await tv.keyboard.press("Enter");
  await expect.poll(async () => (await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual(["tr", "en"]);
  await expect.poll(() => focused(tv)).toBe(key);
  await expect(tv.locator(`[data-f="${key}"] .lp-rank`)).toHaveText("Default");
  await expect(tv.locator(`[data-f="${key}"]`)).toHaveAccessibleName(/default, Remove$/);
});

test("Subtitles: a language list that fails says so, offers the common ones, and Continue still goes on", async ({ browser }) => {
  const page = await device(browser, "desktop", { locale: "de-DE", languages: "fail" });
  await page.goto("/welcome?step=subs&form=desktop");
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("No subtitles");
  await openPicker(page);
  await expect(page.locator(".lp-note")).toContainText("The language list could not be loaded: the core is busy");
  await expect(page.locator('[data-f="welcome:lang:s:none"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-f="welcome:lang:s:en"]')).toBeVisible();
  await page.locator('[data-f="lp:done"]').click();
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual([]);
});

test("Subtitles: Skip waits while Continue saves, so the step leaves once", async ({ browser }) => {
  const page = await device(browser, "desktop", { locale: "de-DE", slowSubtitles: 1500 });
  await page.goto("/welcome?step=subs&form=desktop");
  await expect(page.locator('[data-f="welcome:lang:open"]')).toContainText("Deutsch");
  await page.locator('[data-f="welcome:next"]').click();
  const skip = page.locator('[data-f="welcome:skip"]');
  await expect(skip).toHaveAttribute("aria-disabled", "true");
  // A press anyway (a remote's select reaches it): it goes nowhere.
  await skip.dispatchEvent("click");
  await page.waitForTimeout(300);
  await expect(page).toHaveURL(/step=subs/);
  // The save lands, and the step leaves once, to the setup check.
  await expect(page).toHaveURL(/step=3/, { timeout: 10_000 });
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual(["de"]);
});

test("after an import, Back from the setup check does not call Momotus's own installs From Stremio", async ({ browser }) => {
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
  await expect(page).toHaveURL(/step=subs/);
  // Skip writes nothing.
  await page.locator('[data-f="welcome:skip"]').click();
  await expect(page).toHaveURL(/step=3/);
  expect((await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages).toEqual([]);
  // Neither the question nor the subtitles step is kept under the setup check: the question's doors would rewrite the choice.
  await page.goBack();
  await expect(page).toHaveURL(/step=2/);
  await expect(page).not.toHaveURL(/from=import/);
  await expect(page.locator(".drow.fixed", { hasText: "Torrentio" })).toContainText("Installed");
  await expect(page.locator(".drow.fixed", { hasText: "From Stremio" })).toHaveCount(0);
});

test("an empty Home offers Set up Momotus alone, not the defaults card beside it", async ({ browser }) => {
  // A list with no catalog addon: Home is empty, and the profile is no longer fresh.
  await install("https://torrentio.strem.fun/manifest.json");
  const page = await device(browser, "desktop");
  await page.goto("/?form=desktop");
  await expect(page.locator('[data-f="home:setup"]')).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(1500);
  await expect(page.locator(".defaults-card")).toHaveCount(0);
  // Set up, then Continue: Home opens as the root, Back never returns to the step.
  await page.locator('[data-f="home:setup"]').click();
  await expect(page).toHaveURL(/step=2&review=1/);
  await page.locator('[data-f="welcome:next"]').click();
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/(\?.*)?$/);
  await backStaysHome(page);
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
  await expect(builtIn.locator(".host")).toHaveText("Built into Momotus, runs on this device");
  // Never configurable: its old host is never opened.
  await expect(builtIn).not.toContainText("Settings live on its website");
  await expect(page.locator("li.addon", { hasText: "Cinemeta" }).locator(".host")).toHaveText("v3-cinemeta.strem.io");
  await shot(page, "addons-built-in", "desktop");
  // The site's credit lives with its own row; About names no addon (the owner, 2026-10-30).
  await expect(builtIn).toContainText("Turkish subtitles from TurkceAltyazi.org.");
  await page.goto("/settings/about?form=desktop");
  await expect(page.locator("#set-about")).toBeVisible();
  await expect(page.locator("#set-about")).not.toContainText("TurkceAltyazi");
  await expect(page.locator("#set-about")).not.toContainText("Turkish");
});

test("the addon directory is a default: Find addons lists it out of the box", async ({ browser }) => {
  // The owner's decision of 2026-10-28 (plans/plugins-and-marketplace.md 7.1): installed like any
  // default, from its bundled manifest; its list comes from stremio-addons.net as recorded.
  await client.req("installDefaults", { choices: [{ key: "cinemeta", on: true }, { key: "stremioaddons", on: true }] });
  await client.req("setPrefs", { patch: { defaultsSeen: 1 } });
  const page = await device(browser, "desktop");
  await page.goto("/find-addons?form=desktop");
  const chip = page.getByRole("radio", { name: /^stremio-addons\.net/ });
  await expect(chip).toBeVisible({ timeout: 20_000 });
  await chip.click();
  await expect(page.locator(".fa-from a")).toHaveText("From stremio-addons.net");
  await expect(page.locator(".fa-card:not(.skel)").first()).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole("button", { name: /^Install / }).first()).toBeVisible();
  await shot(page, "find-addons-directory-default", "desktop");
});
