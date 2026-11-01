/** Screenshots of every screen on live data, per form factor and theme,
 * written to SHOTS (e2e/core.ts) as JPEG (quality 80) for review. */

import { expect, test } from "./fixture";
import { SHOTS } from "./core";
import { type Form, FORMS, SCREENS } from "./screens";

// SHOTS_PREFIX names a set (a design audit's before and after).
const OUT = SHOTS;
const PREFIX = process.env.SHOTS_PREFIX ?? "";

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
