/** The recommended addons through the web screens, on a fresh core of its own
 * per walk (the shared core never gets them): first run installs nothing
 * before Start watching, then every default; a TV does it with keys only; the
 * setup sheet writes the profile's subtitle language, its More chip opening
 * the language picker; an import's Done adds the defaults it did not bring; a
 * pasted link that carries a sign-in is confirmed first. Stremio's sign-in
 * cannot run here, so the import view's answer is a finished import made up
 * at the socket, and the addon it brought is installed through the protocol;
 * everything else is the real core. Shots go to SHOTS (e2e/core.ts) at
 * desktop, phone and TV sizes. */

import { type Browser, expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import type { DefaultAddons } from "../src/core/generated/DefaultAddons";
import type { Prefs } from "../src/core/generated/Prefs";
import { Client, cleanup, DEFAULTS_PORT, PORTS, SHOTS, startBridge } from "./core";
import { WHITE_PNG } from "./contrast";

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
        ws.send(JSON.stringify({ id: msg.id, ok: false, error: "the core is busy", kind: "failed" }));
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

/** First run's setup sheet (plans/first-screens.md 3.5), its More chip opening the language picker over it. */
async function openPicker(page: Page) {
  await page.locator('[data-f="setup:sub:more"]').click();
  await expect(page.locator(".lp-layer")).toHaveCount(1);
}
const languages = async () => (await client.req<Prefs>("setPrefs", { patch: {} })).subtitleLanguages;

test("first run on a fresh core: nothing installed before Start watching, then every default on, the language written, Home as the root", async ({ browser }) => {
  const page = await device(browser, "desktop");
  // A profile that never had a list opens at first run; with no sync server, Get started is the profile form.
  await page.goto("/?form=desktop");
  await expect(page).toHaveURL(/\/welcome/);
  await page.locator('[data-f="welcome:start"]').click();
  await expect(page).toHaveURL(/step=profile/);
  await page.locator('[data-f="profile:continue"]').click();
  await expect(page).toHaveURL(/step=setup/);
  await expect(page.locator('[data-f="setup:sub:en"]')).toHaveAttribute("aria-checked", "true");
  await shot(page, "first-run-setup", "desktop");
  expect(await allAbsent()).toBe(true);
  await page.locator('[data-f="setup:start"]').click();
  await expect(page).toHaveURL(/\/(\?form=desktop)?$/, { timeout: 60_000 });
  const on = await states();
  expect(Object.values(on).length).toBeGreaterThan(3);
  expect(Object.values(on).every((s) => s === "on")).toBe(true);
  expect(await languages()).toEqual(["en"]);
  expect(sent.filter((m) => m.op === "createProfile")).toEqual([]);
  // Home is the root: Back walks into no first run step.
  await backStaysHome(page);
});

test("a TV does it with keys only: OK on Get started, Continue and Start watching", async ({ browser }) => {
  const tv = await device(browser, "tv");
  await tv.goto("/?form=tv");
  await expect.poll(() => focused(tv)).toBe("welcome:start");
  await tv.keyboard.press("Enter");
  await expect.poll(() => focused(tv)).toBe("profile:continue");
  await tv.keyboard.press("Enter");
  await expect.poll(() => focused(tv)).toBe("setup:start");
  await tv.keyboard.press("Enter");
  await expect(tv).toHaveURL(/\/(\?form=tv)?$/, { timeout: 60_000 });
  expect(Object.values(await states()).every((s) => s === "on")).toBe(true);
  await backStaysHome(tv);
});

test("Bring my Stremio library: the import's Done adds the defaults it did not bring, then Home", async ({ browser }) => {
  const page = await device(browser, "desktop", { importDone: true });
  await page.goto("/welcome?step=setup&form=desktop");
  await page.locator('[data-f="setup:bring"]').click();
  await page.locator('[data-f="setup:start"]').click();
  await expect(page).toHaveURL(/\/import\?from=welcome/);
  await install(CINEMETA);
  await page.locator('[data-f="import:home"]').click();
  await expect(page).toHaveURL(/\/(\?form=desktop)?$/, { timeout: 60_000 });
  expect(Object.values(await states()).every((s) => s === "on")).toBe(true);
});

test("Subtitles: the device's language is chosen; More finds another; No subtitles writes an empty list", async ({ browser }) => {
  // A Turkish phone: Turkish chosen, English beside it, then No subtitles and More.
  const phone = await device(browser, "phone", { locale: "tr-TR" });
  await phone.goto("/welcome?step=setup&form=phone");
  const chips = phone.locator('[data-fgroup="setup:subs"] [data-f]');
  await expect(chips).toHaveText(["Türkçe", "English", "No subtitles", "More"]);
  await expect(phone.locator('[data-f="setup:sub:tr"]')).toHaveAttribute("aria-checked", "true");
  await shot(phone, "first-run-setup", "phone");
  await openPicker(phone);
  await expect(phone.locator('[data-f="welcome:lang:field"]')).toBeFocused();
  await expect(phone.locator('[data-f="welcome:lang:s:tr"]')).toHaveAttribute("aria-selected", "true");
  await expect(phone.locator('[data-fgroup="welcome:lang-r"] [data-f]')).toHaveCount(113);
  await phone.locator('[data-f="welcome:lang:s:none"]').click();
  await expect(phone.locator(".lp-layer")).toHaveCount(0);
  await expect(phone.locator('[data-f="setup:sub:none"]')).toHaveAttribute("aria-checked", "true");
  await phone.locator('[data-f="setup:start"]').click();
  await expect(phone).toHaveURL(/\/(\?form=phone)?$/, { timeout: 60_000 });
  expect(await languages()).toEqual([]);
});

test("Subtitles picker on desktop: farsi finds Persian, its chip is chosen and written", async ({ browser }) => {
  const page = await device(browser, "desktop", { locale: "tr-TR" });
  await page.goto("/welcome?step=setup&form=desktop");
  await openPicker(page);
  const field = page.locator('[data-f="welcome:lang:field"]');
  await expect(field).toBeFocused();
  const dir = page.locator('[data-fgroup="welcome:lang-r"]');
  await expect(dir.locator("[data-f]")).toHaveCount(113);
  await expect(dir).toHaveAttribute("role", "listbox");
  // Letters typed with a row focused go to the field; Backspace there deletes, it is not Back.
  await page.locator('[data-f="welcome:lang:r:de"]').focus();
  await page.keyboard.press("q");
  await expect(field).toBeFocused();
  await expect(field).toHaveValue("q");
  await page.locator('[data-f="welcome:lang:r:kk"]').focus();
  await page.keyboard.press("Backspace");
  await expect(field).toHaveValue("");
  await expect(page).toHaveURL(/step=setup/);
  await field.focus();
  await page.keyboard.type("farsi");
  await expect(dir.locator("[data-f]").first()).toHaveAttribute("data-f", "welcome:lang:r:fa");
  await page.locator('[data-f="welcome:lang:r:fa"]').click();
  await expect(page.locator(".lp-layer")).toHaveCount(0);
  await expect(page.locator('[data-f="setup:sub:fa"]')).toHaveAttribute("aria-checked", "true");
  await page.locator('[data-f="setup:start"]').click();
  await expect(page).toHaveURL(/\/(\?form=desktop)?$/, { timeout: 60_000 });
  expect(await languages()).toEqual(["fa"]);
});

test("Subtitles picker on TV: More opens it on the chosen language, Up reaches the keyboard, Back clears the query first", async ({ browser }) => {
  const tv = await device(browser, "tv", { locale: "tr-TR" });
  await tv.goto("/welcome?step=setup&form=tv");
  await expect.poll(() => focused(tv)).toBe("setup:start");
  await tv.locator('[data-f="setup:sub:more"]').focus();
  await tv.keyboard.press("Enter");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await tv.keyboard.press("ArrowUp");
  await expect.poll(() => focused(tv)).toMatch(/^welcome:lang:kb:/);
  await tv.keyboard.type("russk");
  await expect(tv.locator('[data-fgroup="welcome:lang-r"] [data-f]').first()).toHaveAttribute("data-f", "welcome:lang:r:ru");
  await tv.keyboard.press("Escape");
  await expect(tv.locator(".lp-field input")).toHaveValue("");
  await expect(tv).toHaveURL(/step=setup/);
  await tv.keyboard.press("ArrowDown");
  await expect.poll(() => focused(tv)).toBe("welcome:lang:s:tr");
  await tv.keyboard.press("Enter");
  await expect(tv.locator(".lp-layer")).toHaveCount(0);
  await expect(tv.locator('[data-f="setup:sub:tr"]')).toHaveAttribute("aria-checked", "true");
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

test("Settings on TV: OK on a language applies the rule and the focus stays on it", async ({ browser }) => {
  await client.req("setPrefs", { patch: { subtitleLanguages: ["en"] } });
  const tv = await device(browser, "tv", { locale: "tr-TR" });
  await tv.goto("/settings/subtitles?form=tv");
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
  // Other rows keep their host behind Details: no raw host in the row itself.
  const cinemeta = page.locator("li.addon", { hasText: "Cinemeta" });
  await expect(cinemeta.locator(".host")).toBeHidden();
  await cinemeta.locator("summary").click();
  await expect(cinemeta.locator(".host")).toHaveText("v3-cinemeta.strem.io");
  await cinemeta.locator("summary").click();
  await shot(page, "addons-built-in", "desktop");
  // The site's credit lives with its own row; About names no addon (the owner, 2026-10-30).
  await expect(builtIn).toContainText("Turkish subtitles from TurkceAltyazi.org.");
  await page.goto("/settings/about?form=desktop");
  await expect(page.locator("#set-about")).toBeVisible();
  await expect(page.locator("#set-about")).not.toContainText("TurkceAltyazi");
  await expect(page.locator("#set-about")).not.toContainText("Turkish");
});

test("Settings > Addons: a logo is asked with no Referer, and one that does not load leaves its letter", async ({ browser }) => {
  // imgur, where SubSense and Anime Kitsu keep their logos, answers 403 to a page's Referer (QA r3 Q6: empty tiles).
  await client.req("installDefaults", { choices: ["cinemeta", "subsense", "kitsu"].map((key) => ({ key, on: true })) });
  await client.req("setPrefs", { patch: { defaultsSeen: 1 } });
  const page = await device(browser, "desktop");
  const referers: string[] = [];
  await page.route("https://i.imgur.com/**", async (route) => {
    const referer = route.request().headers().referer;
    if (referer) referers.push(referer);
    // SubSense's logo as imgur serves it; Kitsu's host is down.
    if (route.request().url().endsWith("/FaDbQAp.png") && !referer) await route.fulfill({ contentType: "image/png", body: WHITE_PNG });
    else if (route.request().url().endsWith("/FaDbQAp.png")) await route.fulfill({ status: 403 });
    else await route.abort();
  });
  await page.goto("/addons?form=desktop");
  const tile = (name: string) => page.locator("li.addon", { hasText: name }).locator(".addon-logo");
  await expect.poll(() => tile("SubSense").locator("img").evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
  await expect(tile("Anime Kitsu").locator("img")).toHaveCount(0);
  await expect(tile("Anime Kitsu")).toHaveText("A");
  await expect(tile("Cinemeta")).toHaveText("C");
  expect(referers).toEqual([]);
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

test("tv: removing the last addon leaves the focus on the row above, not the first one (QA r1 Q6 row 51)", async ({ browser }) => {
  await client.req("installDefaults", {
    choices: [
      { key: "cinemeta", on: true },
      { key: "turkcealtyazi", on: true },
    ],
  });
  await client.req("setPrefs", { patch: { defaultsSeen: 1 } });
  const page = await device(browser, "tv");
  await page.goto("/addons?form=tv");
  const active = () => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f);
  await expect(page.locator("li.addon")).toHaveCount(2);
  await page.locator('[data-f="addon:1:remove"]').focus();
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-f="addon:1:remove"]')).toHaveText("Press again to remove");
  await page.keyboard.press("Enter");
  await expect(page.locator("li.addon")).toHaveCount(1);
  await expect.poll(active).toBe("addon:0:remove");
});
