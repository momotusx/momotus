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
    // A screen reader meets a switch with its name and its state.
    await expect(page.getByRole("switch", { name: /^Trakt/ })).toHaveAttribute("aria-checked", "true");
    // One switch turned off, so both states are on screen.
    if (form === "phone") await rows.first().tap();
    else await rows.first().click();
    await expect(rows.first()).toHaveAttribute("aria-checked", "false");
    const tracks = await rows.evaluateAll((els) =>
      els.map((r) => ({ on: r.getAttribute("aria-checked") === "true", track: getComputedStyle(r.querySelector(".switch") as Element).backgroundColor })),
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

// Fifth pass (design/audit-web-2026-10-19.md): what still read below the bar
// of the Apple TV app, Netflix and Infuse on the viewer's core flow.

const SHOT = process.env.SHOTS_DIR ?? "e2e/shots";
/** A logo of a given shape, served for every title's logo request. */
const logoSvg = (w: number, h: number) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}"><rect width="${w}" height="${h}" rx="${Math.min(w, h) / 5}" fill="#e9e2d2"/></svg>`;

/** The drawn logo's size in canvas px (the image fits its box, contained). */
async function drawnLogo(browser: Browser, form: Form, path: string, selector: string, w: number, h: number, shot?: string) {
  const { context, page, go } = await open(browser, form, path);
  await context.route(/^https:\/\/images\.metahub\.space\/logo\//, (r) =>
    r.fulfill({ status: 200, contentType: "image/svg+xml", headers: { "access-control-allow-origin": "*" }, body: logoSvg(w, h) }),
  );
  await go();
  const logo = page.locator(selector).first();
  await expect(logo).toBeVisible({ timeout: 30_000 });
  await expect(logo).not.toHaveClass(/pending/);
  await page.waitForTimeout(300);
  if (shot) await page.screenshot({ path: `${SHOT}/${shot}-${form}.jpg`, type: "jpeg", quality: 80 });
  // On Home the hero's eyebrow stays clear of the pill whatever the logo's
  // shape, over a synopsis that runs long (the layout rhythm's rule).
  const clear =
    path === "/" && form !== "phone"
      ? await page.evaluate(() => {
          const syn = document.querySelector(".hero-syn");
          if (syn) syn.textContent = "A long synopsis. ".repeat(40);
          const chip = document.querySelector(".hero .reason")?.getBoundingClientRect().top ?? 0;
          const nav = document.querySelector(".topnav")?.getBoundingClientRect().bottom ?? 0;
          return chip - nav;
        })
      : null;
  if (clear !== null) expect(clear, `${w}x${h} logo: eyebrow below the pill`).toBeGreaterThan(12);
  const box = await logo.evaluate((el) => {
    const img = el as HTMLImageElement;
    const r = img.naturalWidth / img.naturalHeight;
    const bw = img.offsetWidth;
    const bh = img.offsetHeight;
    const dw = Math.min(bw, bh * r);
    return { w: dw, h: dw / r };
  });
  await context.close();
  return box;
}

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: a tall logo and a wide one carry about the same weight, on Home's hero and on Title`, async ({ browser }) => {
    for (const [path, selector] of [
      ["/", ".hero-logo"],
      [`/title/series/${GOT.id}`, ".title-logo"],
    ] as const) {
      const name = path === "/" ? "audit5-logo-home" : "audit5-logo-title";
      const tall = await drawnLogo(browser, form, path, selector, 300, 360, `${name}-tall`);
      const wide = await drawnLogo(browser, form, path, selector, 1200, 150, `${name}-wide`);
      const ratio = (wide.w * wide.h) / (tall.w * tall.h);
      // A stacked logo is no stamp beside a wide one (Netflix sizes logos by area).
      // Within 2.5 times (one height for all drew 4.7 times); the hero caps
      // a stacked logo at 140 px so its stack still clears the pill.
      expect(ratio, `${path}: wide ${Math.round(wide.w)}x${Math.round(wide.h)}, tall ${Math.round(tall.w)}x${Math.round(tall.h)}`).toBeLessThan(2.5);
      expect(ratio).toBeGreaterThan(1 / 2.5);
    }
  });
}

test("tv: paused, the picture dims and the title says what it is", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", `/play/movie/tt1254207/tt1254207`);
  await go();
  const toggle = page.locator('[data-f="player:toggle"]');
  await expect(toggle).toBeVisible({ timeout: 30_000 });
  await page.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 2);
  if ((await toggle.getAttribute("aria-label")) === "Play") await toggle.press("Enter");
  await expect(toggle).toHaveAttribute("aria-label", "Pause");
  // Playing: no dim, no synopsis.
  await expect(page.locator(".player-scr")).not.toHaveClass(/\bpaused\b/);
  await toggle.press("Enter");
  await expect(toggle).toHaveAttribute("aria-label", "Play");
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${SHOT}/audit5-paused-tv.jpg`, type: "jpeg", quality: 80 });
  await expect(page.locator(".player-scr")).toHaveClass(/\bpaused\b/);
  await expect(page.locator(".psyn")).toBeVisible();
  await expect(page.locator(".psyn")).toContainText(/\w+/);
  expect(await page.locator(".pdim").evaluate((d) => getComputedStyle(d).opacity)).toBe("1");
  // Playing again clears both.
  await toggle.press("Enter");
  await expect(page.locator(".player-scr")).not.toHaveClass(/\bpaused\b/);
  await expect(page.locator(".psyn")).toBeHidden();
  await context.close();
});

test("tv: a still that is still loading pulses instead of sitting as a dark box", async ({ browser }) => {
  const { context, page, go } = await open(browser, "tv", "/");
  // Every image waits until released, as on a slow link.
  let release: () => void = () => {};
  const held = new Promise<void>((ok) => (release = ok));
  await context.route(/^https:\/\/(images|episodes)\.metahub\.space\//, async (r) => {
    await held;
    await r.continue();
  });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await go();
  const pic = page.locator('[data-f^="tile:home:continue-watching:"] .pic').nth(1);
  await expect(pic).toBeVisible({ timeout: 30_000 });
  await page.screenshot({ path: `${SHOT}/audit5-loading-tv.jpg`, type: "jpeg", quality: 80 });
  expect(await pic.evaluate((p) => getComputedStyle(p).animationName)).toBe("arriving");
  // The surface breathes, not the box: the focus ring on it stays solid.
  expect(await pic.evaluate((p) => getComputedStyle(p).opacity)).toBe("1");
  release();
  await expect(pic.locator("img.in")).toBeVisible({ timeout: 30_000 });
  expect(await pic.evaluate((p) => getComputedStyle(p).animationName)).toBe("none");
  await context.close();
});

test("phone: an episode's title is read whole, on up to two lines", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}?season=1`);
  await go();
  const titles = page.locator(".ep-t");
  // Season 1 has "Cripples, Bastards, and Broken Things".
  await expect(titles.first()).toHaveText("Winter Is Coming", { timeout: 30_000 });
  await page.waitForTimeout(300);
  await page.locator(".ep").nth(2).scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${SHOT}/audit5-episodes-phone.jpg`, type: "jpeg", quality: 80 });
  const cut = await titles.evaluateAll((els) =>
    els.slice(0, 10).filter((t) => t.scrollWidth > t.clientWidth + 1 || t.scrollHeight > t.clientHeight + 1).map((t) => t.textContent),
  );
  expect(cut).toEqual([]);
  await context.close();
});

test("phone: a meta line that wraps never ends or starts a line with a dot", async ({ browser }) => {
  const { context, page, go } = await open(browser, "phone", `/title/series/${GOT.id}`);
  await go();
  const line = page.locator(".info .meta");
  await expect(line).toContainText("seasons", { timeout: 30_000 });
  await page.waitForTimeout(400);
  const at = await line.evaluate((el) => {
    const mid = (e: Element) => {
      const r = e.getBoundingClientRect();
      return (r.top + r.bottom) / 2;
    };
    const kids = [...el.children];
    const lines = new Set(kids.map((k) => Math.round(mid(k) / 10))).size;
    const stray = [...el.querySelectorAll(":scope > b")]
      .filter((b) => getComputedStyle(b).visibility !== "hidden")
      .filter((b) => {
        const p = b.previousElementSibling;
        const n = b.nextElementSibling;
        return !p || !n || Math.abs(mid(p) - mid(b)) > 8 || Math.abs(mid(n) - mid(b)) > 8;
      })
      .map((b) => `${b.previousElementSibling?.textContent} | ${b.nextElementSibling?.textContent}`);
    return { lines, stray };
  });
  // It wraps on a phone (else this proves nothing), and no dot hangs at a break.
  expect(at.lines).toBeGreaterThan(1);
  expect(at.stray).toEqual([]);
  await context.close();
});
