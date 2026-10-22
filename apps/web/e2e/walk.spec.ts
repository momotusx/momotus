/** The viewer's core flow at each size, as a person would walk it: Home,
 * a series (seasons, episodes, Versions), Back, a film played and paused,
 * Back to Home, and Search. Each step is timed from the action to the
 * moment the screen shows what it is for, and shot. Headless and muted.
 * Timings go to bench/design/web-walk.jsonl (WALK_BENCH=1); shots to
 * SHOTS_DIR as walk-<step>-<form>.jpg. Read by the design audit's fifth
 * pass (design/audit-web-2026-10-19.md). */

import { appendFileSync } from "node:fs";
import { type Browser, expect, type Page, test } from "@playwright/test";
import { BROWSER_ARGS, GOT, SHOTS, startIfPaused } from "./core";
import { BBB_ID } from "./source";

test.use({ launchOptions: { args: ["--mute-audio", ...BROWSER_ARGS] } });

type Form = "tv" | "desktop" | "phone";
const FORMS = {
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};
const OUT = SHOTS;
const cw = (id: string) => `[data-f="tile:home:continue-watching:${id}"]`;

/** Presses, clicks or taps, as each form's viewer does. */
async function act(page: Page, form: Form, selector: string) {
  const el = page.locator(selector).first();
  if (form === "tv") {
    await el.focus();
    await page.keyboard.press("Enter");
  } else if (form === "phone") await el.tap();
  else await el.click();
}

/** Back as each form's viewer goes back. */
async function back(page: Page, form: Form) {
  if (form === "phone") await page.goBack();
  else await page.keyboard.press("Escape");
}

const imageLoaded = (page: Page, selector: string) =>
  page.waitForFunction((sel) => {
    const img = document.querySelector<HTMLImageElement>(sel);
    return !!img && img.complete && img.naturalWidth > 0;
  }, selector);

async function walk(browser: Browser, form: Form) {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark" });
  const page = await context.newPage();
  const ms: Record<string, number> = {};
  const time = async (name: string, until: () => Promise<unknown>) => {
    const t0 = Date.now();
    await until();
    ms[name] = Date.now() - t0;
  };
  const shot = (step: string) => page.screenshot({ path: `${OUT}/walk-${step}-${form}.jpg`, type: "jpeg", quality: 80 });

  // 1. Home: the hero's words, then Continue Watching's first still.
  const t0 = Date.now();
  await page.goto(`/?form=${form}`);
  // The hero's reason shows on every form (the phone's meta line holds only the ratings row, empty without scores).
  await expect(page.locator(".hero .reason")).toBeVisible({ timeout: 30_000 });
  ms.homeHero = Date.now() - t0;
  await imageLoaded(page, `${cw(BBB_ID)} img`);
  ms.homeContinue = Date.now() - t0;
  await page.waitForTimeout(400);
  await shot("1-home");

  // 2. A series: its logo and its episodes.
  await time("title", async () => {
    await act(page, form, cw(GOT.id));
    await expect(page.locator(".title-logo, .title-name").first()).toBeVisible({ timeout: 30_000 });
  });
  await time("episodes", () => expect(page.locator(".ep-t").first()).toBeVisible({ timeout: 30_000 }));
  await page.waitForTimeout(400);
  await shot("2-title");

  // 3. Another season.
  await time("season", async () => {
    await act(page, form, '[data-f="season:3"]');
    await expect(page.locator(".ep-t").first()).toHaveText("Valar Dohaeris", { timeout: 30_000 });
  });
  await page.waitForTimeout(400);
  await shot("3-season");
  await act(page, form, '[data-f="season:2"]');
  await expect(page.locator(".ep-t").first()).toHaveText("The North Remembers");

  // 4. Versions: the pick.
  await time("versions", async () => {
    await act(page, form, '[data-f="title:versions"]');
    await expect(page.locator(".pick h2")).toBeVisible({ timeout: 30_000 });
  });
  await page.waitForTimeout(400);
  await shot("4-versions");

  // 5. Back to Home.
  await time("backHome", async () => {
    await back(page, form);
    await expect(page).toHaveURL(/\/title\//);
    await back(page, form);
    await expect(page.locator(cw(BBB_ID))).toBeVisible();
  });

  // 6. A film: its title, then play (first frame moving).
  await act(page, form, cw(BBB_ID));
  await expect(page.locator('[data-f="title:play"]')).toBeVisible({ timeout: 30_000 });
  // From the press to the first frame playing, as the page saw it.
  await page.evaluate(() => {
    const w = window as unknown as { mqPress: number; mqPlaying: number };
    document.addEventListener("pointerdown", () => (w.mqPress = performance.now()), { capture: true, once: true });
    document.addEventListener("keydown", () => (w.mqPress = performance.now()), { capture: true, once: true });
    document.addEventListener("playing", () => (w.mqPlaying ||= performance.now()), { capture: true });
  });
  await act(page, form, '[data-f="title:play"]');
  await startIfPaused(page);
  await page.waitForFunction(() => (window as unknown as { mqPlaying?: number }).mqPlaying, null, { timeout: 30_000 });
  ms.firstFrame = Math.round(await page.evaluate(() => {
    const w = window as unknown as { mqPress: number; mqPlaying: number };
    return w.mqPlaying - w.mqPress;
  }));
  await page.waitForTimeout(3000);
  // Controls up, as after a press (a tap while they show would hide them).
  const idle = () => page.locator(".player-scr").evaluate((el) => el.classList.contains("idle"));
  if (form === "phone") {
    if (await idle()) await page.locator(".player-scr").tap({ position: { x: 195, y: 300 } });
  } else await page.mouse.move(400, 400);
  await page.waitForTimeout(300);
  await shot("5-player");

  // 7. Pause. A phone's controls hide on their own after a while: a tap on
  // the picture brings them back first, then the tap on Pause counts.
  if (form === "phone") {
    if (await idle()) await page.locator(".player-scr").tap({ position: { x: 195, y: 300 } });
    await expect(page.locator(".player-scr")).not.toHaveClass(/\bidle\b/);
  }
  await time("pause", async () => {
    if (form === "phone") await page.locator('[data-f="player:toggle"]').tap({ force: true });
    else await act(page, form, '[data-f="player:toggle"]');
    await expect(page.locator('[data-f="player:toggle"]')).toHaveAttribute("aria-label", "Play");
  });
  await page.waitForTimeout(400);
  await shot("6-paused");

  // 8. Back to Home: Continue Watching knows the new place.
  await time("homeAfterPlay", async () => {
    await back(page, form);
    await expect(page).toHaveURL(/\/title\//);
    await back(page, form);
    await expect(page.locator(cw(BBB_ID))).toBeVisible();
  });
  await page.waitForTimeout(600);
  await shot("7-home-after");

  // 9. Search.
  await time("search", async () => {
    await act(page, form, form === "phone" ? '.tabs a[href="/search"]' : '[data-f="nav:/search"]');
    await page.locator('[data-f="search:input"]').fill("thrones");
    await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  });
  await page.waitForTimeout(600);
  await shot("8-search");

  await context.close();
  return ms;
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`walk on ${form}: Home, a series, Versions, a film played and paused, Back, Search`, async ({ browser }) => {
    test.setTimeout(240_000);
    const ms = await walk(browser, form);
    console.log(`walk ${form} ${JSON.stringify(ms)}`);
    if (process.env.WALK_BENCH) appendFileSync("../../bench/design/web-walk.jsonl", `${JSON.stringify({ date: new Date().toISOString(), check: "web-walk", form, ms })}\n`);
  });
}
