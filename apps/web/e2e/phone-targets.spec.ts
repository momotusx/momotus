/** Every control on the phone is a 48 x 48 touch target (the owner's rule: 48 dp in both dimensions). QA r1 gave the small
 * ones an invisible reach (Q1 row 29, Q4 row 51, Q5 row 40, Q6 row 39, Q7 rows 103, 104); QA r2 still found them short
 * (Q1 row 21, Q2 row 35, Q4 row 13, Q5 rows 30, 39, Q7 rows 21 to 23). The reach is measured as a finger finds it: the
 * extent around the control's centre where elementFromPoint answers the control itself, so a neighbour that takes
 * part of it shows as a shorter reach. */

import { type Page } from "@playwright/test";
import { expect, test } from "./fixture";
import { startIfPaused } from "./core";

const CONTROL = "button, a[href], [role=button], [role=radio], [role=tab], [role=checkbox], input";
const PLAYER = "/play/movie/tt1254207/tt1254207";

// [path, selector, how many to measure]. A page dot's width is its share of the strip, since the pager's dots sit 16 px
// apart (home.css); the strip answers every point between them as the nearest dot (components/hero.tsx).
const PAGES: [string, string, number][] = [
  ["/", ".tile-x", 3],
  ["/title/series/tt0944947", "a.back", 1],
  ["/discover", '[data-f^="kinds:"]', 6],
  ["/library", '[data-f^="lib-kinds:"], [data-f^="lib-sort:"]', 6],
  ["/title/series/tt0944947", '[data-f^="season:"]', 10],
  ["/title/movie/tt0056923", ".syn-more", 1],
  ["/settings/playback", ".chip[data-f]", 8],
  ["/settings/subtitles", ".chip[data-f]", 14],
  ["/settings/device", ".chip[data-f]", 6],
  ["/home-rows", 'button[data-f^="hrow:"]', 9],
  ["/addons", 'button[data-f^="addon:"]', 6],
  ["/find-addons", '[data-f^="fa-lists:"], [data-f^="fa-filter:"]', 6],
  ["/stream-choice", ".chip[data-f]", 14],
];

/** The width and height around each control's centre that answer the control, in CSS px (half a px steps, 40 each way). */
async function reaches(page: Page, selector: string, count: number) {
  return page.evaluate(
    ({ selector, count, control }) => {
      const out: { name: string; w: number; h: number; own: boolean }[] = [];
      const controls = [...document.querySelectorAll<HTMLElement>(selector)].slice(0, count);
      for (const el of controls) {
        el.scrollIntoView({ block: "center", inline: "center" });
        const r = el.getBoundingClientRect();
        const cx = r.left + r.width / 2;
        const cy = r.top + r.height / 2;
        const mine = (x: number, y: number) => document.elementFromPoint(x, y)?.closest(control) === el;
        const run = (dx: number, dy: number) => {
          let d = 0;
          while (d < 40 && mine(cx + dx * (d + 0.5), cy + dy * (d + 0.5))) d += 0.5;
          return d;
        };
        // The drawn box must stay its own: a reach that took a neighbour's edge would show here.
        const own = [[1, 0], [-1, 0], [0, 1], [0, -1]].every(([sx = 0, sy = 0]) => mine(cx + sx * (r.width / 2 - 1.5), cy + sy * (r.height / 2 - 1.5)));
        out.push({
          name: `${el.dataset.f ?? el.className}`,
          w: run(1, 0) + run(-1, 0),
          h: run(0, 1) + run(0, -1),
          own,
        });
      }
      return out;
    },
    { selector, count, control: CONTROL },
  );
}

// The probe steps by half a px, and two reaches that meet at a shared edge leave that edge point to the later one.
const HALF_PX = 0.5;

type Reach = { name: string; w: number; h: number; own: boolean };

/** The controls of `rows` that fall short, each named with the page it is on. */
function shortOf(where: string, rows: Reach[], minW = 48) {
  expect(rows.length, `${where} has controls to measure`).toBeGreaterThan(0);
  return rows.filter((r) => r.w < minW - HALF_PX || r.h < 48 - HALF_PX || !r.own).map((r) => `${where} ${r.name} ${r.w}x${r.h}${r.own ? "" : " (loses its own edge)"}`);
}

const expectReach = (where: string, rows: Reach[], minW = 48) => expect(shortOf(where, rows, minW)).toEqual([]);

test("phone: every small control reaches 48 x 48 and keeps its own box", async ({ browser }) => {
  test.setTimeout(240_000);
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  const short: string[] = [];
  for (const [path, selector, count] of PAGES) {
    await page.goto(`${path}?form=phone`);
    await expect(page.locator(selector).first()).toBeVisible({ timeout: 30_000 });
    short.push(...shortOf(path, await reaches(page, selector, count)));
  }
  expect(short).toEqual([]);
  await context.close();
});

test("phone: the hero pager dots reach 48 tall and answer every point between them", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto("/?form=phone");
  await expect(page.locator(".hero-dots button.dot").first()).toBeVisible({ timeout: 30_000 });
  const rows = await reaches(page, ".hero-dots button.dot", 8);
  expect(rows.length).toBeGreaterThan(1);
  // 16 px of the strip each, the pitch of the dots; 48 only where the pager has room, which three dots do not give.
  expectReach("hero dots", rows, 15);
  // The strip is the one 48 x 48 control (Android's HeroDots, iOS's page control): a tap anywhere on it pages to the nearest dot.
  const strip = await page.locator(".hero-dots").boundingBox();
  expect(strip?.height).toBeGreaterThanOrEqual(48);
  expect(strip?.width).toBeGreaterThanOrEqual(48);
  await context.close();
});

test("phone: the player's Audio, Speed and the subtitle sheet's search field reach 48 x 48", async ({ browser }) => {
  test.setTimeout(120_000);
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  await page.goto(`${PLAYER}?form=phone`);
  await expect(page.locator('[data-f="player:toggle"]')).toBeAttached({ timeout: 30_000 });
  await page.evaluate(() => {
    const v = document.querySelector("video");
    if (v) v.muted = true;
  });
  await startIfPaused(page);
  await page.locator('[data-f="player:toggle"]').tap();
  await expect(page.locator('[data-f="player:audio"]')).toBeVisible();
  expectReach("player controls", await reaches(page, '[data-f="player:audio"], [data-f="player:speed"], [data-f="player:subs"], [data-f="player:versions"], [data-f="player:leave"]', 5));
  await page.locator('[data-f="player:subs"]').tap();
  await expect(page.locator('[data-f="sub:search"]')).toBeVisible();
  expectReach("subtitle sheet", await reaches(page, '[data-f="sub:search"], [data-f="sub:done"], [data-f^="subopen:"]', 4));
  await context.close();
});
