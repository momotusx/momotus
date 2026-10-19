import { type Browser, expect, type Page, test } from "@playwright/test";
import { GOT } from "./core";

/** Where focus lands stays readable: a focused card and its ring sit clear
 * of a list's fade, and a chip row fades only toward chips left to scroll
 * to (mirrored right to left). */

async function open(browser: Browser, path: string, viewport: { width: number; height: number }, rtl = false) {
  // Reduced motion: scrolling and the focus lift land at once, so a check
  // reads the final layout.
  const context = await browser.newContext({ viewport, reducedMotion: "reduce" });
  const page = await context.newPage();
  if (rtl) await page.addInitScript(() => document.addEventListener("DOMContentLoaded", () => (document.documentElement.dir = "rtl")));
  await page.goto(path);
  return { context, page };
}

/** The focused element, its 4 px ring at a 4 px offset included, inside its
 * list, below its group's pinned heading if it has one, and above the list's
 * fade (canvas pixels, scaled like the page). */
const clearOfFade = (page: Page, fade: number) =>
  page.evaluate((fade) => {
    const el = document.activeElement as HTMLElement;
    const list = el.closest<HTMLElement>("[data-vscroll]");
    if (!list) return { key: el.dataset.f, ok: false, why: "outside any list", left: true };
    const l = list.getBoundingClientRect();
    const k = l.height / list.offsetHeight || 1;
    const e = el.getBoundingClientRect();
    const ring = 8 * k;
    const head = el.closest(".vgroup")?.querySelector("h3")?.getBoundingClientRect();
    const top = Math.max(l.top, head ? head.bottom : l.top);
    const why = `ring ${Math.round(e.top - ring)} to ${Math.round(e.bottom + ring)}, clear from ${Math.round(top)} to ${Math.round(l.bottom - fade * k)}`;
    return { key: el.dataset.f, ok: e.top - ring >= top - 1 && e.bottom + ring <= l.bottom - fade * k + 1, why, left: false };
  }, fade);

/** Presses a key until focus stops moving or leaves the list, checking every stop. */
async function walk(page: Page, key: "ArrowDown" | "ArrowUp", fade: number, max: number) {
  const seen: string[] = [];
  for (let i = 0; i < max; i++) {
    await page.keyboard.press(key);
    const at = await clearOfFade(page, fade);
    if (at.left || (at.key && seen.at(-1) === at.key)) break;
    seen.push(at.key ?? "");
    expect(at.ok, `focused ${at.key} clear of the fade: ${at.why}`).toBe(true);
  }
  return seen;
}

test("TV: the last episode of a season, focused, sits clear of the list's fade", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}?season=1&form=tv`, { width: 1920, height: 1080 });
  const first = page.locator('[data-f="ep:tt0944947:1:1"]');
  await expect(first).toBeVisible({ timeout: 30_000 });
  await first.focus();
  const seen = await walk(page, "ArrowDown", 56, 14);
  // Down from the last card goes on to its actions; it got there clear too.
  expect(seen).toContain("ep:tt0944947:1:10");
  await context.close();
});

test("TV: rows of a group taller than the list clear its pinned heading and the fade, down and up", async ({ browser }) => {
  const { context, page } = await open(browser, `/title/series/${GOT.id}/versions/${GOT.resume}?form=tv`, { width: 1920, height: 1080 });
  const first = page.locator(".vrow").first();
  await expect(first).toBeVisible({ timeout: 30_000 });
  // Every addon has answered, so the list no longer changes under the copies.
  await expect(page.locator(".vstate .live")).toHaveCount(0, { timeout: 60_000 });
  // Live groups hold a row or two: copies of the first row make its group
  // taller than the list. Focus and scrolling read the DOM, not the data.
  await first.evaluate((row) => {
    for (let i = 0; i < 14; i++) {
      const copy = row.cloneNode(true) as HTMLElement;
      copy.dataset.f = `v:copy:${i}`;
      row.after(copy);
    }
  });
  await first.focus();
  const down = await walk(page, "ArrowDown", 48, 20);
  expect(down.length).toBeGreaterThanOrEqual(14);
  const up = await walk(page, "ArrowUp", 48, 20);
  expect(up.length).toBeGreaterThanOrEqual(14);
  await context.close();
});

for (const rtl of [false, true]) {
test(`TV: a chip row fades only toward chips left to scroll to (${rtl ? "right to left" : "left to right"})`, async ({ browser }) => {
  const { context, page } = await open(browser, "/discover?form=tv", { width: 1920, height: 1080 }, rtl);
  const genres = page.locator('[data-fgroup="genres"] .chips-track');
  await expect(genres.locator(".chip").nth(12)).toBeAttached({ timeout: 30_000 });
  const mask = (row: string) =>
    page.locator(`[data-fgroup="${row}"] .chips-track`).evaluate((t) => getComputedStyle(t).maskImage || getComputedStyle(t).webkitMaskImage);
  const fadesAtEnd = (m: string) => m.endsWith("rgba(0, 0, 0, 0))");
  const fadesAtStart = (m: string) => /^linear-gradient\(\d+deg, rgba\(0, 0, 0, 0\)/.test(m);
  // The fade runs toward the row's end: rightward, or leftward right to left.
  const way = rtl ? "linear-gradient(270deg" : "linear-gradient(90deg";

  // Nothing to scroll to: no fade at all.
  await expect.poll(() => mask("kinds")).toBe("none");
  // At the start of a long row: the end fades, the start does not.
  await expect.poll(async () => fadesAtEnd(await mask("genres"))).toBe(true);
  expect(fadesAtStart(await mask("genres"))).toBe(false);
  expect((await mask("genres")).startsWith(way)).toBe(true);
  // At its end: the last chip is not faded; the start now fades.
  await genres.evaluate((t, rtl) => t.scrollTo({ left: rtl ? -t.scrollWidth : t.scrollWidth }), rtl);
  await expect.poll(async () => fadesAtEnd(await mask("genres"))).toBe(false);
  expect(fadesAtStart(await mask("genres"))).toBe(true);
  await context.close();
});
}
