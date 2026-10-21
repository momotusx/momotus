/** The player's loading layer (plans/player-polish.md 1.2) on the shared
 * core, Big Buck Bunny from the sample source. On its way to the page the
 * playback is held at `resolving` with fixture `start` values (e2e/patch.ts):
 * a local file starts before a test could look, and a bridge built before
 * `start` existed sends none. The title's logo is a fixture with transparent
 * margins, so the trim shows; a title with no logo fills its name. Muted,
 * headless. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import { PORTS, SHOTS, startIfPaused } from "./core";
import { patchedContext } from "./patch";
import { BBB_ID } from "./source";

// 800x310 with its drawn box at (196, 9), 417x292, as the frames' title logo (design/exploration/v3/src/data.ts).
const LOGO = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="310"><rect x="196" y="9" width="417" height="292" rx="36" fill="#3ddc5a"/><text x="404.5" y="186" font-family="sans-serif" font-size="92" font-weight="700" text-anchor="middle" fill="#0d3b18">BUNNY</text></svg>',
)}`;

/** A torrent start as the core reports it (plans/core-protocol.md, `start`). */
const start = (progress: number, caption: string | null = null) => ({
  stage: "buffering",
  peers: 3,
  haveBytes: Math.round(progress * 8_400_000),
  needBytes: 8_400_000,
  bytesPerS: 900_000,
  source: 1,
  sources: 1,
  progress,
  caption,
});

// What the page is told: the playback held while `hold` is set, the title's logo when `logo` is not undefined.
let hold: Record<string, unknown> | null = null;
let logo: string | null | undefined;
const patch = (view: string, s: { header?: { details?: { logo: string | null } } } & Record<string, unknown>) => {
  if (view === "playback" && hold) return { ...s, phase: "resolving", url: null, ...hold };
  if (view === "title" && logo !== undefined && s?.header?.details) s.header.details.logo = logo;
  return s;
};

async function open(browser: Browser, form: "tv" | "desktop" | "phone", reducedMotion: "reduce" | "no-preference" = "no-preference") {
  const viewport = form === "tv" ? { width: 1920, height: 1080 } : form === "desktop" ? { width: 1440, height: 900 } : { width: 844, height: 390 };
  const { context, repush } = await patchedContext(browser, PORTS.bridge, { viewport, reducedMotion }, patch);
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/play/movie/${BBB_ID}/${BBB_ID}?form=${form}`);
  await mute(page);
  await expect(page.locator(".ld.on")).toBeVisible({ timeout: 30_000 });
  return { context, page, repush, errors };
}

const mute = (page: Page) =>
  page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
const fillOf = (page: Page) => page.locator(".ld.on .ld-mark").evaluate((el) => (el as HTMLElement).style.getPropertyValue("--p"));
/** Waits for the fill to ease to where it was asked to (1 s), then shoots. */
const shoot = async (page: Page, name: string) => {
  await page.waitForTimeout(1300);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
};

test.afterEach(() => {
  hold = null;
  logo = undefined;
});

test("desktop: the logo fills with the start, the words under it, then the picture; a stall brings it back", async ({ browser }) => {
  hold = { start: start(0.2) };
  logo = LOGO;
  const { context, page, repush, errors } = await open(browser, "desktop");
  const layer = page.locator(".ld.on");
  await expect.poll(() => fillOf(page)).toBe("0.2");
  await expect(layer.locator(".ld-cap span")).toHaveText([/^Starting .+ from /]);

  // The logo trimmed to its drawn box, about 40 percent of the width.
  const solid = layer.locator("img.ld-logo.ld-solid");
  await expect(solid).toBeVisible();
  expect(await solid.getAttribute("src")).toMatch(/^data:image\/png/);
  expect(await solid.evaluate((img: HTMLImageElement) => img.naturalWidth / img.naturalHeight)).toBeCloseTo(417 / 292, 1);
  const box = await solid.boundingBox();
  expect((box?.width ?? 0) / 1440).toBeCloseTo(0.4, 1);
  // Its faint copy sits under it, whole.
  await expect(layer.locator("img.ld-logo.ld-faint")).toHaveCSS("opacity", "0.2");
  // Back and nothing else: no controls, no title text.
  await expect(page.locator('[data-f="player:leave"]')).toBeVisible();
  await expect(page.locator('[data-f="player:toggle"]')).toHaveCount(0);
  await expect(page.locator(".ptitle h1")).toBeHidden();
  // The backdrop, dimmed and blurred.
  await expect(layer.locator(".ld-bd")).toHaveCSS("opacity", "0.3");

  hold = { start: start(0.4) };
  repush("playback");
  await expect.poll(() => fillOf(page)).toBe("0.4");
  await shoot(page, "player-loading-desktop");

  // The core's words once a stage takes long, under the pick's.
  hold = { start: start(0.6, "Finding peers, 3 so far") };
  repush("playback");
  await expect.poll(() => fillOf(page)).toBe("0.6");
  await expect(layer.locator(".ld-cap span")).toHaveText([/^Starting .+ from /, "Finding peers, 3 so far"]);
  await shoot(page, "player-loading-wait-desktop");
  // The fill never goes back within one start.
  hold = { start: start(0.45, "Buffering, 3.8 of 8.4 MB") };
  repush("playback");
  await expect(layer.locator(".ld-cap span").last()).toHaveText("Buffering, 3.8 of 8.4 MB");
  expect(await fillOf(page)).toBe("0.6");

  // Ready: the player's own buffer fills the rest, then the first frame takes over.
  hold = null;
  repush("playback");
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await mute(page);
  await startIfPaused(page);
  await expect(page.locator(".ld.on")).toHaveCount(0);
  await expect(page.locator(".ptitle h1")).toBeVisible();

  // A stall after the first frame: the layer over the paused frame, no backdrop, no words.
  await page.evaluate(() => document.querySelector("video")?.dispatchEvent(new Event("waiting")));
  await expect(page.locator(".ld.on.stall")).toBeVisible({ timeout: 5_000 });
  await expect(page.locator(".ld.on .ld-bd")).toBeHidden();
  await expect(page.locator(".ld.on .ld-cap span")).toHaveCount(0);
  // The controls stay usable under it.
  await expect(page.locator('[data-f="player:toggle"]')).toBeVisible();
  await page.evaluate(() => document.querySelector("video")?.dispatchEvent(new Event("playing")));
  await expect(page.locator(".ld.on")).toHaveCount(0);
  expect(errors).toEqual([]);
  await context.close();
});

test("TV: nothing is focused while it loads, and a title with no logo fills its name", async ({ browser }) => {
  hold = { start: start(0.4) };
  logo = null;
  const { context, page, errors } = await open(browser, "tv");
  const layer = page.locator(".ld.on");
  await expect(layer.locator(".ld-name.ld-solid")).toHaveText("Big Buck Bunny");
  await expect(layer.locator(".ld-name.ld-faint")).toHaveCSS("opacity", "0.2");
  await expect(layer.locator("img.ld-logo")).toHaveCount(0);
  await expect.poll(() => fillOf(page)).toBe("0.4");
  // No Back to focus: the remote's Back cancels.
  await expect(page.locator('[data-f="player:leave"]')).toHaveCount(0);
  expect(await page.evaluate(() => document.activeElement?.getAttribute("data-f") ?? null)).toBeNull();
  await shoot(page, "player-loading-name-tv");
  await page.keyboard.press("Escape");
  await expect(page).not.toHaveURL(/\/play\//);
  expect(errors).toEqual([]);
  await context.close();
});

test("TV and phone: the logo at the form's size, Back on the phone", async ({ browser }) => {
  for (const form of ["tv", "phone"] as const) {
    hold = { start: start(0.4) };
    logo = LOGO;
    const { context, page, errors } = await open(browser, form);
    const solid = page.locator(".ld.on img.ld-logo.ld-solid");
    await expect(solid).toBeVisible();
    await expect.poll(() => fillOf(page)).toBe("0.4");
    const width = (await solid.boundingBox())?.width ?? 0;
    const screen = form === "tv" ? 1920 : 844;
    // 40 percent of the width on TV; on the landscape phone the height binds (60 percent of it).
    if (form === "tv") expect(width / screen).toBeCloseTo(0.4, 1);
    else expect(width).toBeCloseTo(390 * 0.6 * (417 / 292), -1);
    await expect(page.locator('[data-f="player:leave"]')).toHaveCount(form === "phone" ? 1 : 0);
    await shoot(page, `player-loading-${form}`);
    expect(errors).toEqual([]);
    await context.close();
  }
});

test("reduced motion: no pulse, the fill stays", async ({ browser }) => {
  hold = { start: start(0.4) };
  logo = LOGO;
  const { context, page } = await open(browser, "desktop", "reduce");
  await expect.poll(() => fillOf(page)).toBe("0.4");
  await expect(page.locator(".ld.on .ld-mark")).toHaveCSS("animation-name", "none");
  await context.close();
  hold = { start: start(0.4) };
  const moving = await open(browser, "desktop");
  await expect(moving.page.locator(".ld.on .ld-mark")).toHaveCSS("animation-name", "ld-pulse");
  await moving.context.close();
});
