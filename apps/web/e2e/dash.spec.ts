import { expect, test } from "./fixture";
import { GOT, startIfPaused } from "./core";
import { DASH_EPISODE } from "./source";

test.use({ viewport: { width: 1440, height: 900 } });

// Chromium plays HLS itself (142+); the second run hides that, as Firefox has none, so hls.js carries it.
for (const native of [true, false]) {
  test(`a DASH source plays as HLS ${native ? "in the browser's own player" : "through hls.js"}`, async ({ page }) => {
    if (!native) {
      await page.addInitScript(() => {
        const own = HTMLMediaElement.prototype.canPlayType;
        HTMLMediaElement.prototype.canPlayType = function (type: string) {
          return /mpegurl/i.test(type) ? "" : own.call(this, type);
        };
      });
    }
    await page.goto(`/play/series/${GOT.id}/${DASH_EPISODE}?form=desktop`);
    await expect(page.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
    try {
      await startIfPaused(page);
      const time = () => page.evaluate(() => document.querySelector("video")?.currentTime ?? 0);
      await expect.poll(time, { timeout: 30_000 }).toBeGreaterThan(4);
      // The core's HLS URL in the element, or hls.js's Media Source.
      const src = await page.evaluate(() => document.querySelector("video")?.currentSrc ?? "");
      expect(src).toMatch(native ? /^http:\/\/127\.0\.0\.1:\d+\/s\/\w+\/.*\.m3u8$/ : /^blob:/);
      await expect(page.locator(".endcard")).toHaveCount(0);
    } finally {
      await page.evaluate(() => document.querySelector("video")?.pause());
    }
  });
}
