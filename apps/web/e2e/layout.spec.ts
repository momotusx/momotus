import { type Browser, expect, test } from "@playwright/test";
import { GOT } from "./core";

/** Layout rules the design audit settled (design/audit-web-2026-10-19.md). */

const FORMS = { tv: [1920, 1080, 1], desktop: [1440, 900, 1], phone: [390, 844, 2] } as const;
type Form = keyof typeof FORMS;

async function open(browser: Browser, form: Form, path: string) {
  const [width, height, scale] = FORMS[form];
  const context = await browser.newContext({
    viewport: { width, height },
    deviceScaleFactor: scale,
    isMobile: form === "phone",
    hasTouch: form === "phone",
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  return { context, page, go: () => page.goto(`${path}${path.includes("?") ? "&" : "?"}form=${form}`) };
}

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: the hero sits on the rows' grid line`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/");
    await go();
    await expect(page.locator(".hero .meta")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".row-head").first()).toBeVisible({ timeout: 30_000 });
    const at = await page.evaluate((phone) => {
      const head = document.querySelector<HTMLElement>(".row-head") as HTMLElement;
      const r = head.getBoundingClientRect();
      const k = r.width / head.offsetWidth || 1;
      const grid = r.left + parseFloat(getComputedStyle(head).paddingLeft) * k;
      // TV and desktop: the words start on the line; phone: the card's edge does.
      const hero = document.querySelector(phone ? ".hero" : ".hero .meta") as HTMLElement;
      return { grid, left: hero.getBoundingClientRect().left };
    }, form === "phone");
    expect(Math.abs(at.left - at.grid)).toBeLessThanOrEqual(1);
    await context.close();
  });
}

for (const form of ["tv", "desktop"] as const) {
  test(`${form}: settings pages are one centered column, no empty right third`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/settings");
    for (const [path, block] of [
      ["/settings", ".link-row"],
      ["/addons", ".addon"],
      ["/recommended-addons", "button.drow"],
      ["/profiles/manage", ".prow"],
    ] as const) {
      await page.goto(`${path}?form=${form}`);
      await expect(page.locator(block).first()).toBeVisible({ timeout: 30_000 });
      const box = await page.evaluate((sel) => {
        const r = (document.querySelector(sel) as HTMLElement).getBoundingClientRect();
        return { left: r.left, right: innerWidth - r.right, width: innerWidth };
      }, block);
      expect(Math.abs(box.left - box.right), `${path} is centered`).toBeLessThanOrEqual(2);
      expect(box.width - box.right, `${path} reaches past two thirds`).toBeGreaterThanOrEqual((box.width * 2) / 3);
    }
    await context.close();
  });
}

test("phone: a version row's quality starts on its size's line", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}/versions/${GOT.resume}`);
  await go();
  const row = page.locator(".vrow").filter({ has: page.locator(".size:not(:empty)") }).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  const tops = await row.evaluate((r) => ({
    q: (r.querySelector(".q") as HTMLElement).getBoundingClientRect().top,
    size: (r.querySelector(".size") as HTMLElement).getBoundingClientRect().top,
  }));
  expect(Math.abs(tops.q - tops.size)).toBeLessThanOrEqual(3);
  await context.close();
});

// A phone wraps the link onto its own line under Save.
for (const form of ["tv", "desktop"] as const) {
  test(`${form}: the Debrid key page link is centered on Save`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/debrid");
    await go();
    const form_ = page.locator(".dkey").first();
    await expect(form_).toBeVisible({ timeout: 30_000 });
    const mid = await form_.evaluate((f) => {
      const c = (el: Element) => {
        const r = el.getBoundingClientRect();
        return r.top + r.height / 2;
      };
      return { link: c(f.querySelector(".dlink") as Element), save: c(f.querySelector('button[type="submit"]') as Element) };
    });
    expect(Math.abs(mid.link - mid.save)).toBeLessThanOrEqual(2);
    await context.close();
  });
}

test("tv: an episode whose still fails shows the series art with its number, not a dark box", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", `/title/series/${GOT.id}?season=1`);
  // Every episode still fails to load.
  await context.route(/^https:\/\/episodes\.metahub\.space\//, (r) => r.fulfill({ status: 404, body: "" }));
  await go();
  await expect(page.locator(".ep-t").first()).toBeVisible({ timeout: 30_000 });
  const stills = page.locator(".ep-still");
  for (let i = 0; i < 4; i++) {
    const still = stills.nth(i);
    await expect(still.locator("img.stand-in.in")).toBeVisible({ timeout: 15_000 });
    await expect(still.locator(".ep-n")).toBeVisible();
  }
  await context.close();
});

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: recommended addons read at a glance: filled when on, a mark each, one white primary`, async ({ browser }) => {
    const { context, page, go } = await open(browser, form, "/welcome?step=2");
    await go();
    const rows = page.locator("button.drow");
    await expect(rows.first()).toBeVisible({ timeout: 30_000 });
    // One switch turned off, so both states are on screen.
    if (form === "phone") await rows.first().tap();
    else await rows.first().click();
    await expect(rows.first()).toHaveAttribute("aria-pressed", "false");
    const tracks = await rows.evaluateAll((els) =>
      els.map((r) => ({ on: r.getAttribute("aria-pressed") === "true", track: getComputedStyle(r.querySelector(".switch") as Element).backgroundColor })),
    );
    expect(tracks.some((t) => t.on)).toBe(true);
    // On is a white track, off an empty one: the state reads without the word.
    for (const t of tracks) expect(t.track).toBe(t.on ? "rgb(255, 255, 255)" : "rgba(0, 0, 0, 0)");
    // Every row shows its mark: the logo, or the name's initial.
    const marks = await page.locator(".drow").evaluateAll((els) =>
      els.map((r) => ({ name: r.querySelector(".dtext b")?.textContent ?? "", mark: r.querySelector(".dlogo img") ? "logo" : (r.querySelector(".dlogo b")?.textContent ?? "") })),
    );
    expect(marks.length).toBeGreaterThan(0);
    for (const m of marks) expect(m.mark === "logo" || m.mark === m.name.charAt(0), `${m.name} has a mark`).toBe(true);
    // Continue is the one white primary.
    await expect(page.locator(".btn.primary")).toHaveCount(1);
    await expect(page.locator(".btn.primary")).toContainText("Continue");
    if (form !== "phone") {
      const gaps = await page.locator(".dgroups").evaluate((g) => {
        const r = g.getBoundingClientRect();
        return { left: r.left, right: innerWidth - r.right };
      });
      expect(Math.abs(gaps.left - gaps.right), "the step is centered").toBeLessThanOrEqual(2);
    }
    await context.close();
  });
}
