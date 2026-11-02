/** A finger finds every small control on the phone (QA r1, Q1 row 29, Q4 row 51, Q5 row 40, Q6 row 39, Q7 rows 103,
 * 104): a control drawn under 48 px keeps its look, and a tap a few px above or below it still lands on it, or on the
 * control next to it where two sit closer than a finger, never on nothing. */

import { expect, test } from "./fixture";

const PAGES: [string, string][] = [
  ["/", ".hero-dots button.dot, .tile-x"],
  ["/discover", '[data-f^="kinds:"]'],
  ["/library", '[data-f^="lib-kinds:"], [data-f^="lib-sort:"]'],
  ["/title/series/tt0944947", '[data-f^="season:"]'],
  ["/title/movie/tt0056923", ".syn-more"],
  ["/settings/playback", '[data-f^="pref-next:"]'],
  ["/home-rows", 'button[data-f^="hrow:"]'],
  ["/addons", 'button[data-f^="addon:"]'],
  ["/find-addons", '[data-f^="fa-lists:"], [data-f^="fa-filter:"]'],
  ["/stream-choice", '[data-f^="choice-size:"]'],
];

test("phone: a tap just outside a small control lands on a control", async ({ browser }) => {
  test.setTimeout(180_000);
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const page = await context.newPage();
  for (const [path, selector] of PAGES) {
    await page.goto(`${path}?form=phone`);
    await expect(page.locator(selector).first()).toBeVisible({ timeout: 30_000 });
    const missed = await page.evaluate((selector) => {
      const out: string[] = [];
      const controls = [...document.querySelectorAll<HTMLElement>(selector)].slice(0, 6);
      for (const el of controls) {
        el.scrollIntoView({ block: "center", inline: "nearest" });
        const r = el.getBoundingClientRect();
        if (r.height >= 48) continue;
        const x = r.left + r.width / 2;
        // 5 px outside the drawn box, where a 36 px chip used to answer nothing.
        for (const y of [r.top - 5, r.bottom + 5]) {
          const hit = document.elementFromPoint(x, y);
          const control = hit?.closest("button, a[href], [role=button], [role=radio], [role=tab]");
          if (!control) out.push(`${el.dataset.f ?? el.className} ${Math.round(r.width)}x${Math.round(r.height)} at ${y < r.top ? "top" : "bottom"}: ${hit?.className ?? "nothing"}`);
        }
      }
      return out;
    }, selector);
    expect(missed, path).toEqual([]);
  }
  await context.close();
});
