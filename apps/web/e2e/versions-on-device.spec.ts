/** Versions says "On this device" where the core does (plans/player-polish.md
 * 2b): as the row's line, and on the recommended card. A bridge built before
 * `onDevice` sends none, so the shared core's Versions of Game of Thrones
 * S2:E3 (a pick and one row) gets it on its way to the page (e2e/patch.ts). */

import { expect, test } from "@playwright/test";
import { GOT, PORTS } from "./core";
import { patchedContext } from "./patch";

type Row = { key: string; onDevice?: string | null };
// Which version holds its start on this device: the row, or the pick.
let held: "row" | "pick" = "row";
const patch = (view: string, s: { recommended?: Row | null; rows?: Row[] }) => {
  if (view !== "versions" || !s?.rows) return s;
  for (const r of [...(s.recommended ? [s.recommended] : []), ...s.rows]) r.onDevice = null;
  const target = held === "row" ? s.rows[0] : s.recommended;
  if (target) target.onDevice = "On this device";
  return s;
};

test("the version this device holds says so: the row's line, or a line on the pick", async ({ browser }) => {
  for (const which of ["row", "pick"] as const) {
    held = which;
    const { context } = await patchedContext(browser, PORTS.bridge, { viewport: { width: 1440, height: 900 } }, patch);
    const page = await context.newPage();
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=desktop`);
    const row = page.locator(".vrow").first();
    await expect(row).toBeVisible({ timeout: 30_000 });
    if (which === "row") {
      await expect(row.locator(".st.line .lt")).toHaveText("On this device");
      await expect(page.locator(".why li", { hasText: /^On this device$/ })).toHaveCount(0);
    } else {
      await expect(page.locator(".why li").first()).toHaveText("On this device");
      await expect(row.locator(".st.line .lt")).not.toHaveText("On this device");
    }
    await context.close();
  }
});
