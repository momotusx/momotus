/** Screenshots of every screen on live data, per form factor and theme,
 * written to e2e/shots/ as JPEG (quality 80) for review. */

import { type BrowserContext, expect, type Page, test } from "@playwright/test";
import { GOT, startIfPaused } from "./core";
import { BBB_ID } from "./source";

type Form = "tv" | "desktop" | "phone";
const FORMS: Record<Form, { width: number; height: number; scale: number }> = {
  tv: { width: 1920, height: 1080, scale: 1 },
  desktop: { width: 1440, height: 900, scale: 1 },
  phone: { width: 390, height: 844, scale: 2 },
};

interface Screen {
  name: string;
  path: string;
  ready: (page: Page, form: Form) => Promise<void>;
}

const visible = (sel: string, timeout = 30_000) => async (page: Page) => {
  await expect(page.locator(sel).first()).toBeVisible({ timeout });
};

const SCREENS: Screen[] = [
  {
    name: "home",
    path: "/",
    ready: async (page) => {
      await visible(".hero .reason")(page);
      for (const row of [0, 1]) {
        await expect(page.locator(".row").nth(row).locator(".tile img.in").first()).toBeVisible({ timeout: 30_000 });
      }
      // Every mounted poster in Continue Watching has decoded.
      await expect.poll(() => page.locator(".row").first().locator(".tile img:not(.in)").count()).toBe(0);
    },
  },
  { name: "title", path: `/title/series/${GOT.id}`, ready: visible(".ep-t") },
  {
    name: "versions",
    path: `/title/series/${GOT.id}/versions/${GOT.resume}`,
    ready: async (page, form) => {
      await visible(".vrow")(page);
      // On TV and desktop show a focused row with its full release name.
      if (form !== "phone") await page.locator(".vrow").first().focus();
    },
  },
  {
    name: "player",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(152);
      await page.locator('[data-f="player:toggle"]').focus();
    },
  },
  { name: "discover", path: "/discover", ready: visible(".grid .tile img.in") },
  {
    name: "search",
    path: "/search",
    ready: async (page) => {
      await page.locator('[data-f="search:input"]').fill("thrones");
      await visible(".results .tile img.in")(page);
    },
  },
  { name: "library", path: "/library", ready: visible(".grid .tile img.in") },
  { name: "addons", path: "/addons", ready: visible(".addon") },
  { name: "settings", path: "/settings", ready: visible(".chips-row") },
  { name: "welcome", path: "/welcome", ready: visible(".check li") },
  { name: "import", path: "/import", ready: visible(".code span, .problem") },
  { name: "pairing", path: "/pair", ready: visible(".install") },
];

const LIGHT = new Set(["home", "title", "versions", "discover", "settings"]);

// Each shot opens a fresh context, so artwork is fetched once here and
// served to every context after that, instead of asking the image CDN for
// the same posters dozens of times a minute.
const art = new Map<string, { status: number; headers: Record<string, string>; body: Buffer }>();
const ART = /^https:\/\/(images|episodes)\.metahub\.space\//;
async function cacheArt(context: BrowserContext) {
  await context.route(ART, async (route) => {
    const url = route.request().url();
    try {
      let hit = art.get(url);
      if (!hit) {
        const r = await route.fetch();
        hit = { status: r.status(), headers: r.headers(), body: await r.body() };
        if (r.ok()) art.set(url, hit);
      }
      await route.fulfill(hit);
    } catch {
      // The shot finished and its context closed while this image was loading.
    }
  });
}

for (const form of Object.keys(FORMS) as Form[]) {
  for (const screen of SCREENS) {
    for (const scheme of ["dark", "light"] as const) {
      if (scheme === "light" && (form === "tv" || !LIGHT.has(screen.name))) continue;
      test(`${screen.name} ${form} ${scheme}`, async ({ browser }) => {
        const f = FORMS[form];
        const context = await browser.newContext({
          viewport: { width: f.width, height: f.height },
          deviceScaleFactor: f.scale,
          colorScheme: scheme,
          isMobile: form === "phone",
          hasTouch: form === "phone",
        });
        await cacheArt(context);
        const page = await context.newPage();
        const errors: string[] = [];
        page.on("pageerror", (e) => errors.push(e.message));
        await page.goto(`${screen.path}?form=${form}`);
        await screen.ready(page, form);
        // Let focus motion and image fades settle.
        await page.waitForTimeout(700);
        const suffix = scheme === "light" ? "-light" : "";
        await page.screenshot({ path: `e2e/shots/${screen.name}-${form}${suffix}.jpg`, type: "jpeg", quality: 80 });
        expect(errors).toEqual([]);
        await context.unrouteAll({ behavior: "ignoreErrors" });
        await context.close();
      });
    }
  }
}
