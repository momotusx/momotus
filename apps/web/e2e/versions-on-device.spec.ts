/** Versions says "On this device" where the core does (plans/player-polish.md
 * 2b): as the row's line, and on the recommended card. The core sets
 * `onDevice` only on a version its torrent engine plays, and the e2e bridge
 * has no torrent engine, so the shared core's Versions of Game of Thrones
 * S2:E3 (a pick and one row) gets it on its way to the page (e2e/patch.ts). */

import { expect, test } from "@playwright/test";
import { GOT, PORTS } from "./core";
import { patchedContext } from "./patch";

type Row = { key: string; onDevice?: string | null; line?: unknown; reasons?: unknown[] };
// Which version holds its start on this device: the row, the pick, or none (the row only warns).
let held: "row" | "pick" | "none" = "row";
// A line that warns with details ("Few people sharing"), on the first row.
const FEW = { kind: "few", seeders: 2 };
const patch = (view: string, s: { recommended?: Row | null; rows?: Row[] }) => {
  if (view !== "versions" || !s?.rows) return s;
  for (const r of [...(s.recommended ? [s.recommended] : []), ...s.rows]) r.onDevice = null;
  if (s.rows[0]) s.rows[0].line = FEW;
  // Three reasons on the pick, the most the card shows.
  const reasons = s.recommended?.reasons ?? [];
  if (s.recommended && reasons.length) s.recommended.reasons = [0, 1, 2].map((i) => reasons[i % reasons.length]);
  const target = held === "row" ? s.rows[0] : held === "pick" ? s.recommended : null;
  if (target) target.onDevice = "On this device";
  return s;
};

test("the version this device holds says so: the row's line with no stale warning, or a line on the pick", async ({ browser }) => {
  for (const which of ["none", "row", "pick"] as const) {
    held = which;
    const { context } = await patchedContext(browser, PORTS.bridge, { viewport: { width: 1440, height: 900 } }, patch);
    const page = await context.newPage();
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=desktop`);
    const row = page.locator(".vrow").first();
    await expect(row).toBeVisible({ timeout: 30_000 });
    const why = page.locator(".pick .why1");
    if (which === "none") {
      // The warning and its details, as the core sent them.
      await expect(row.locator(".st.line .lt")).not.toContainText("on this device");
      await expect(row.locator(".more")).toContainText("Shared by 2 people");
      await expect(why).not.toHaveText("On this device");
    } else if (which === "row") {
      // Its size first, then the core's words.
      await expect(row.locator(".st.line .lt")).toHaveText(/^\S+ [KMG]B, on this device$/);
      // The words that stand for the warning take its details with them.
      await expect(row.locator(".more")).not.toContainText("Shared by");
      await expect(why).not.toHaveText("On this device");
    } else {
      // The card's one why line says it in place of the core's line.
      await expect(why).toHaveText("On this device");
      await expect(row.locator(".st.line .lt")).not.toContainText("on this device");
    }
    await context.close();
  }
});
