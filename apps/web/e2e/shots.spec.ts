/** Screenshots of every screen on live data, per form factor and theme,
 * written to e2e/shots/ as JPEG (quality 80) for review. */

import { type BrowserContext, expect, type Page, test } from "@playwright/test";
import { GOT, reseedBbb, startIfPaused } from "./core";
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
  /** Puts shared core state back after the shot. */
  after?: (page: Page) => Promise<void>;
  /** Live codes and links covered in the image (account link codes). */
  mask?: string;
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
      await expect.poll(() => page.locator(".row").first().locator(".tile img.fade:not(.in)").count()).toBe(0);
    },
  },
  { name: "title", path: `/title/series/${GOT.id}`, ready: visible(".ep-t") },
  {
    name: "title-trailer",
    path: `/title/movie/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="title:trailer"]')(page);
      await page.locator('[data-f="title:trailer"]').focus();
      await page.keyboard.press("Enter");
      await visible(".trailer iframe")(page);
      // Let the embedded player paint its first frame.
      await page.waitForTimeout(5000);
    },
  },
  {
    name: "title-watched",
    path: `/title/movie/${BBB_ID}`,
    ready: async (page) => {
      const watched = page.locator('[data-f="title:watched"]');
      await expect(watched).toBeVisible({ timeout: 30_000 });
      // By keyboard, as a remote would, so the focused toggle shows its word.
      await watched.focus();
      if ((await watched.getAttribute("aria-pressed")) !== "true") await page.keyboard.press("Enter");
      await expect(watched).toHaveAttribute("aria-pressed", "true");
    },
    // Watched movies leave Continue Watching; later shots need it back.
    after: async (page) => {
      const watched = page.locator('[data-f="title:watched"]');
      await watched.click();
      await expect(watched).toHaveAttribute("aria-pressed", "false");
    },
  },
  {
    // An episode row with focus shows its secondary actions (phone: always).
    name: "title-episode",
    path: `/title/series/${GOT.id}?season=1`,
    ready: async (page, form) => {
      const row = page.locator('[data-f="ep:tt0944947:1:5"]');
      await expect(row).toBeVisible({ timeout: 30_000 });
      if (form !== "phone") await row.focus();
      else await row.evaluate((el) => el.scrollIntoView({ block: "center" }));
    },
  },
  // A movie whose metadata lists no trailer: no Trailer action at all.
  { name: "title-no-trailer", path: "/title/movie/tt43718175", ready: visible('[data-f="title:watched"]') },
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
    ready: async (page, form) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(152);
      // Touch has no focus ring; TV and desktop show where the remote is.
      if (form !== "phone") await page.locator('[data-f="player:toggle"]').focus();
    },
  },
  {
    // Inside the live TheIntroDB credits, controls hidden, Skip focused.
    name: "player-skip",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(150);
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) v.currentTime = 493;
      });
      await visible('[data-f="player:skip"]')(page);
      await expect(page.locator(".player-scr")).toHaveClass(/idle/, { timeout: 10_000 });
    },
    after: async (page) => {
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) {
          v.currentTime = 150;
          v.pause();
        }
      });
      await page.waitForTimeout(500);
    },
  },
  {
    // The movie's end: the card with Back to the title and Play again.
    name: "player-end",
    path: `/play/movie/${BBB_ID}/${BBB_ID}`,
    ready: async (page) => {
      await visible('[data-f="player:toggle"]')(page);
      await startIfPaused(page);
      await expect.poll(() => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0), { timeout: 30_000 }).toBeGreaterThan(150);
      await page.evaluate(() => {
        const v = document.querySelector("video");
        if (v) v.currentTime = v.duration - 2;
      });
      await visible(".endcard", 15_000)(page);
    },
    after: async (page) => {
      await reseedBbb(page);
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
  {
    // TV and desktop: the first row picked up for moving.
    name: "home-rows",
    path: "/home-rows",
    ready: async (page, form) => {
      await visible(".hrow-main")(page);
      if (form === "phone") return;
      await page.locator(".hrow-main").first().focus();
      await page.keyboard.press("Enter");
      await visible(".hrow.moving")(page);
    },
    after: async (page) => {
      if (await page.locator(".hrow.moving").count()) await page.keyboard.press("Enter");
    },
  },
  {
    name: "calendar",
    path: "/calendar",
    ready: async (page) => {
      await visible(".cal-card")(page);
      await expect(page.locator(".cal-head .st.warn")).toHaveCount(0, { timeout: 60_000 });
      // Stills on screen have decoded (a missing one keeps its plain frame).
      await page
        .waitForFunction(
          () =>
            [...document.querySelectorAll<HTMLImageElement>(".cal-card img.fade")].every((img) => {
              const r = img.getBoundingClientRect();
              return img.classList.contains("in") || r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth;
            }),
          null,
          { timeout: 15_000 },
        )
        .catch(() => undefined);
    },
  },
  {
    // Playback (prefs) and the addons line come from live views; "This
    // device" renders at once, so it cannot stand for the whole screen.
    name: "settings",
    path: "/settings",
    ready: async (page) => {
      await visible('[data-fgroup="pref-quality"]')(page);
      await expect(page.locator('[data-f="settings:addons"] small')).toContainText(/\d+ installed/);
    },
  },
  { name: "welcome", path: "/welcome", ready: visible(".door") },
  { name: "welcome-source", path: "/welcome?step=2", ready: visible(".sugg") },
  { name: "welcome-check", path: "/welcome?step=3", ready: visible(".check li") },
  { name: "import", path: "/import", ready: visible(".code, .problem"), mask: ".code, .qrbox, b.mono, [data-f='import:open']" },
  { name: "pairing", path: "/pair", ready: visible(".install") },
  { name: "find-addons", path: "/find-addons", ready: visible(".fa-card:not(.skel)") },
  { name: "debrid", path: "/debrid", ready: visible(".dcard") },
  { name: "profiles", path: "/profiles", ready: visible(".ptile") },
  { name: "profiles-manage", path: "/profiles/manage", ready: visible(".prow") },
  { name: "profile-edit", path: "/profiles/new", ready: visible(".pedit") },
];

// SHOTS_DIR and SHOTS_PREFIX send a run elsewhere (a design audit's before
// and after) without touching the committed set.
const OUT = process.env.SHOTS_DIR ?? "e2e/shots";
const PREFIX = process.env.SHOTS_PREFIX ?? "";


// Each shot opens a fresh context, so artwork is fetched once here and
// served to every context after that, instead of asking the image CDN for
// the same posters dozens of times a minute.
const art = new Map<string, { status: number; headers: Record<string, string>; body: Buffer }>();
const ART = /^https:\/\/(images|episodes|live)\.metahub\.space\//;
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
    {
      // Dark only (design/DECISION.md v2).
      test(`${screen.name} ${form}`, async ({ browser }) => {
        const f = FORMS[form];
        const context = await browser.newContext({
          viewport: { width: f.width, height: f.height },
          deviceScaleFactor: f.scale,
          colorScheme: "dark",
          isMobile: form === "phone",
          hasTouch: form === "phone",
        });
        await cacheArt(context);
        const page = await context.newPage();
        const errors: string[] = [];
        page.on("pageerror", (e) => errors.push(e.message));
        const join = screen.path.includes("?") ? "&" : "?";
        await page.goto(`${screen.path}${join}form=${form}`);
        await screen.ready(page, form);
        // Give on-screen art up to 15 s to decode; a tile still loading shows
        // its name card, which is a valid state, so this never fails.
        await page
          .waitForFunction(
            () =>
              [...document.querySelectorAll<HTMLImageElement>(".tile img.fade:not(.in)")].every((img) => {
                const r = img.getBoundingClientRect();
                return r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth;
              }) && [...document.querySelectorAll<HTMLImageElement>("img.logo, img.art-logo")].every((img) => img.complete && !!img.getAttribute("src")),
            null,
            { timeout: 15_000 },
          )
          .catch(() => undefined);
        // Let focus motion and image fades settle.
        await page.waitForTimeout(700);
        const mask = screen.mask ? [page.locator(screen.mask)] : [];
        await page.screenshot({ path: `${OUT}/${PREFIX}${screen.name}-${form}.jpg`, type: "jpeg", quality: 80, mask });
        await screen.after?.(page);
        expect(errors).toEqual([]);
        await context.unrouteAll({ behavior: "ignoreErrors" });
        await context.close();
      });
    }
  }
}
