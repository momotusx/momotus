import { type Browser, expect, type Page, test } from "@playwright/test";
import { GOT } from "./core";
import { BBB_ID } from "./source";
import { type Layer, ramps } from "../src/components/ramps";

/** The shared pieces of design v3 (plans/design-v3-rollout.md, 6; design/DECISION.md): buttons,
 * the backdrop ramps, the per-form page colors and navigation, rows, progress, the segmented
 * control and the pull-down, the ratings row and tiles. */

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
  await page.goto(`${path}${path.includes("?") ? "&" : "?"}form=${form}`);
  return { context, page };
}

const style = (page: Page, selector: string, pseudo?: string) =>
  page.locator(selector).first().evaluate((el, pseudo) => {
    const s = getComputedStyle(el, pseudo);
    return { background: s.backgroundColor, color: s.color, shadow: s.boxShadow, blur: s.backdropFilter, weight: s.fontWeight, content: s.content, size: s.fontSize, image: s.backgroundImage };
  }, pseudo);

const focusedKey = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: the primary is white with a black label, a secondary the 16 percent fill with a white label and no edge`, async ({ browser }) => {
    const { context, page } = await open(browser, form, `/title/movie/${BBB_ID}`);
    const play = page.locator('[data-f="title:play"], [data-f="title:startover"]').first();
    await expect(play).toBeVisible({ timeout: 30_000 });
    // Off the TV nothing is focused on load; on TV focus moves away from the buttons.
    if (form === "tv") await page.locator('[data-f="title:trailer"]').focus();
    const primary = await style(page, ".play-row .btn.primary");
    if (form !== "tv") expect(primary).toMatchObject({ background: form === "phone" ? "rgb(245, 245, 245)" : "rgba(255, 255, 255, 0.92)", color: "rgba(0, 0, 0, 0.88)", weight: "600", shadow: "none" });
    // The title's round secondaries over the art: the fill and a pure white glyph, no blur.
    await page.locator('[data-f="title:versions"]').focus();
    const trailer = await style(page, '[data-f="title:trailer"] svg');
    expect(trailer).toMatchObject({ background: "rgba(255, 255, 255, 0.16)", color: "rgb(255, 255, 255)", shadow: "none", blur: "none" });
    // A secondary on a plain page: the fill and no edge, and no blur with nothing to blur.
    await page.goto(`/addons?form=${form}`);
    const plain = page.locator('[data-f="addons:find"]');
    await expect(plain).toBeVisible({ timeout: 30_000 });
    if (form === "tv") await page.locator('[data-f="addons:paste"]').focus();
    const s = await plain.evaluate((el) => {
      const c = getComputedStyle(el);
      return { background: c.backgroundColor, color: c.color, shadow: c.boxShadow, blur: c.backdropFilter };
    });
    expect(s).toMatchObject({ background: "rgba(255, 255, 255, 0.16)", color: "rgb(255, 255, 255)", shadow: "none", blur: "none" });
    await context.close();
  });

  test(`${form}: the page takes its form's color, and the hero's ramps fade to it`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/");
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    const page_ = { tv: "rgb(28, 28, 30)", desktop: "rgb(31, 31, 31)", phone: "rgb(0, 0, 0)" }[form];
    expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).toBe(page_);
    expect(await page.locator('meta[name="theme-color"]').getAttribute("content")).toBe({ tv: "#1C1C1E", desktop: "#1F1F1F", phone: "#000000" }[form]);
    // The ramps come from components/ramps.ts in the page color, never v2's blue-black.
    const ramp = await page.locator(".hero-page.on .hero-bd .ramp").evaluate((el) => (el as HTMLElement).style.background);
    expect(ramp).toContain(page_.replace("rgb(", "rgba(").replace(")", ", 0."));
    expect(ramp).not.toContain("6, 9, 14");
    if (form === "phone") expect((await style(page, ".row .tile .pic")).background).toBe("rgb(28, 28, 30)");
    await context.close();
  });

  test(`${form}: shelf titles are title3 with no bar, no hairline and their caption under the art`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/");
    await expect(page.locator(".row .tile").first()).toBeVisible({ timeout: 30_000 });
    expect((await style(page, ".row-head", "::before")).content).toBe("none");
    expect((await style(page, ".row-head", "::after")).content).toBe("none");
    // In CSS pixels: TV's title3 38 on its canvas, tv.apple.com's 17 on desktop, the iPhone's 20.
    const k = await page.evaluate(() => Number(getComputedStyle(document.documentElement).getPropertyValue("--k")) || 1);
    expect(Math.round(parseFloat((await style(page, ".row-head h2")).size) * k)).toBe({ tv: 38, desktop: 17, phone: 20 }[form]);
    // The title alone: no row names its addon (e2e/addon-names.spec.ts has two of one title).
    await expect(page.locator(".row-head .row-src")).toHaveCount(0);
    // Every artwork's 25 percent inner keyline over the picture; the name at 600 under it.
    expect((await style(page, ".row .tile .pic", "::before")).shadow).toBe("rgba(255, 255, 255, 0.25) 0px 0px 0px 1px inset");
    // A caption under the art: every TV row's, Continue Watching's elsewhere (posters carry none off the TV).
    const name = page.locator(".row").nth(form === "tv" ? 1 : 0).locator(".tile .cap b").first();
    expect(await name.evaluate((el) => getComputedStyle(el).fontWeight)).toBe("600");
    // Focus leaves the caption's color as it is.
    const before = await name.evaluate((el) => getComputedStyle(el).color);
    await page.locator(".row").nth(form === "tv" ? 1 : 0).locator(".tile").first().focus();
    expect(await name.evaluate((el) => getComputedStyle(el).color)).toBe(before);
    await context.close();
  });

  test(`${form}: progress is white on a dark track`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/");
    const track = page.locator(".row .tile .bar-track").first();
    await expect(track).toBeVisible({ timeout: 30_000 });
    // Its height in CSS pixels: 8 on TV, 4 on desktop (tv.apple.com) and the phone.
    const s = await track.evaluate((el) => ({ track: getComputedStyle(el).backgroundColor, fill: getComputedStyle(el.firstElementChild as Element).backgroundColor, h: Math.round(el.getBoundingClientRect().height) }));
    expect(s).toEqual({ track: "rgba(0, 0, 0, 0.55)", fill: "rgb(255, 255, 255)", h: form === "tv" ? 8 : 4 });
    await context.close();
  });

  test(`${form}: no eyebrow over a page title, and Versions' pick says what it is best for`, async ({ browser }) => {
    const { context, page } = await open(browser, form, "/discover");
    await expect(page.locator(".browse h1")).toHaveText("Discover", { timeout: 30_000 });
    expect((await style(page, ".browse h1", "::before")).content).toBe("none");
    await expect(page.locator(".eyebrow, [data-eyebrow]")).toHaveCount(0);
    await page.goto(`/title/series/${GOT.id}/versions/${GOT.resume}?form=${form}`);
    // The pick once it is chosen (its waiting state says the same).
    await expect(page.locator(".pick h2").first()).toBeVisible({ timeout: 30_000 });
    // The form's own words, one per device on every platform: this TV, this phone, this computer.
    await expect(page.locator(".pick .best")).toHaveText({ tv: "Best for this TV", phone: "Best for this phone", desktop: "Best for this computer" }[form], { timeout: 30_000 });
    expect((await style(page, ".pick .best")).color).toBe(form === "phone" ? "rgba(235, 235, 245, 0.6)" : "rgba(255, 255, 255, 0.64)");
    await context.close();
  });

  test(`${form}: the ratings row shows each score in white after its service's mark`, async ({ browser }) => {
    const { context, page } = await open(browser, form, `/title/series/${GOT.id}`);
    const score = page.locator(".info .meta .scores .score").first();
    await expect(score).toBeVisible({ timeout: 30_000 });
    expect(await score.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(255, 255, 255)");
    // The service is named by its own mark (an image with no text of its own), never by a word.
    await expect(score.locator("img.mark")).toHaveAttribute("alt", "");
    await expect(score.locator(".num")).toHaveText(/^\d+(\.\d)?\/10$/);
    await context.close();
  });
}

test("Search's sections show their title alone, each title once", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/search");
  await page.locator('[data-f="search:input"]').fill("thrones");
  await expect(page.locator(".hits .hit").first()).toBeVisible({ timeout: 30_000 });
  const titles = await page.locator(".hits-head .kicker").allTextContents();
  expect(new Set(titles).size).toBe(titles.length);
  await expect(page.locator(".hits-src")).toHaveCount(0);
  await context.close();
});

test("Customize Home names each row's addon on a quiet second line; Discover's catalogs name none", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/home-rows");
  await expect(page.locator(".hrow-main").first()).toBeVisible({ timeout: 30_000 });
  const rows = await page.locator(".hrow").evaluateAll((els) =>
    els.map((el) => ({
      title: el.querySelector(".hrow-text b")?.textContent ?? "",
      label: el.querySelector(".hrow-main")?.getAttribute("aria-label") ?? "",
      note: el.querySelector(".hrow-text small")?.textContent ?? null,
    })),
  );
  for (const r of rows) {
    const source = (r.note ?? "").replace(/ · Hidden from Home$/, "");
    expect(source.length, r.title).toBeGreaterThan(0);
    const head = `${r.title}, ${source}, `;
    expect(r.label.startsWith(head) && /^(hidden|shown), row \d+ of \d+$/.test(r.label.slice(head.length)), r.label).toBe(true);
  }
  await page.goto("/discover?form=desktop");
  // The catalogs are the Sort menu's choices, each once, with no addon's name beside it.
  await page.locator('[data-f="sorts:menu"]').click({ timeout: 30_000 });
  const catalogs = page.locator('[data-fgroup="sorts"] .pull-opt');
  await expect(catalogs.first()).toBeVisible({ timeout: 30_000 });
  const names = await catalogs.allTextContents();
  expect(new Set(names).size).toBe(names.length);
  await expect(page.locator('[data-fgroup="sorts"] .pull-opt small')).toHaveCount(0);
  await context.close();
});

test("desktop: a hovered poster takes tv.apple.com's grey scrim and does not lift", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  const tile = page.locator(".row").nth(1).locator(".tile").first();
  await expect(tile).toBeVisible({ timeout: 30_000 });
  await tile.hover();
  // At once, no transition: a lift with a transitioned shadow repainted the whole row on every frame of a wheel scroll.
  const s = await tile.evaluate((el) => {
    const after = getComputedStyle(el.querySelector(".pic") as Element, "::after");
    return { lift: getComputedStyle(el).transform, scrim: after.backgroundColor, opacity: after.opacity, transition: after.transitionDuration };
  });
  expect(s).toEqual({ lift: "none", scrim: "rgba(51, 51, 51, 0.3)", opacity: "1", transition: "0s" });
  await context.close();
});

test("TV: the solid tab bar, 68 tall at 46, every tab with its word, the active one in a white 20 percent pill", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/");
  const bar = page.locator(".topnav .tabbar");
  await expect(bar).toBeVisible({ timeout: 30_000 });
  const box = (await bar.boundingBox())!;
  expect(Math.round(box.y)).toBe(46);
  expect(Math.round(box.height)).toBe(68);
  expect(Math.abs(box.x + box.width / 2 - 960)).toBeLessThanOrEqual(1);
  expect((await style(page, ".topnav .tabbar")).background).toBe("rgb(44, 44, 46)");
  const tabs = bar.locator(".nav:not(.pavatar)");
  // Settings has no tab: the profile at the bar's end opens it (plans/nav-profile.md).
  await expect(tabs).toHaveText(["Home", "Discover", "Library", "Calendar", "Search"]);
  await expect(bar.locator(".nav").last()).toHaveAttribute("data-f", "nav:/profiles");
  // Off the active tab (focus moves to the content on load).
  await page.locator(".row .tile").first().focus();
  expect(await style(page, ".tabbar .nav.on")).toMatchObject({ background: "rgba(255, 255, 255, 0.2)", color: "rgb(255, 255, 255)" });
  expect((await style(page, '.tabbar .nav[href="/discover"]')).color).toBe("rgba(255, 255, 255, 0.64)");
  // The wordmark at the left, on the bar's middle line, with no dot.
  const mark = (await page.locator(".topnav .brand").boundingBox())!;
  expect(Math.round(mark.x)).toBe(80);
  expect(Math.abs(mark.y + mark.height / 2 - 80)).toBeLessThanOrEqual(1);
  expect((await style(page, ".topnav .brand", "::before")).content).toBe("none");
  await context.close();
});

test("desktop: tv.apple.com's 52 px black header, text tabs centered, Search and the profile at the right", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/");
  const bar = page.locator(".topnav");
  await expect(bar.locator(".tabbar")).toBeVisible({ timeout: 30_000 });
  const box = (await bar.boundingBox())!;
  expect(box).toMatchObject({ x: 0, y: 0, width: 1440 });
  expect(Math.round(box.height)).toBe(52);
  expect((await style(page, ".topnav")).background).toBe("rgb(0, 0, 0)");
  const tabs = bar.locator(".tabbar .nav");
  await expect(tabs).toHaveText(["Home", "Discover", "Library", "Calendar"]);
  const tabBox = (await bar.locator(".tabbar").boundingBox())!;
  expect(Math.abs(tabBox.x + tabBox.width / 2 - 720)).toBeLessThanOrEqual(1);
  // 16/600 in CSS pixels on the scaled canvas; the active one at 92 percent, the rest at 64.
  const font = await tabs.first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { size: parseFloat(getComputedStyle(el).fontSize) * (r.width / (el as HTMLElement).offsetWidth), weight: getComputedStyle(el).fontWeight };
  });
  expect(Math.abs(font.size - 16)).toBeLessThanOrEqual(0.1);
  expect(font.weight).toBe("600");
  expect((await style(page, ".tabbar .nav.on")).color).toBe("rgba(255, 255, 255, 0.92)");
  expect((await style(page, '.tabbar .nav[href="/discover"]')).color).toBe("rgba(255, 255, 255, 0.64)");
  const field = (await bar.locator('.topnav-end [data-f="nav:/search"]').boundingBox())!;
  expect(field.x).toBeGreaterThan(tabBox.x + tabBox.width);
  await expect(bar.locator(".topnav-end .pavatar")).toBeVisible();
  await context.close();
});

test("phone: the floating glass capsule, 348x62 and 21 from the edges, five tabs, the selected one in key blue", async ({ browser }) => {
  const { context, page } = await open(browser, "phone", "/");
  const bar = page.locator("nav.tabs");
  await expect(bar).toBeVisible({ timeout: 30_000 });
  const box = (await bar.boundingBox())!;
  expect(box).toMatchObject({ x: 21, width: 348, height: 62 });
  expect(Math.round(844 - box.y - box.height)).toBe(21);
  await expect(bar.locator("a")).toHaveText(["Home", "Discover", "Library", "Search"]);
  // The profile last, its avatar over "Profile" (plans/nav-profile.md).
  await expect(bar.locator(":scope > *").last().locator(".ptab > span:not(.avatar)")).toHaveText("Profile");
  const s = await style(page, "nav.tabs");
  expect(s.background).toBe("rgba(32, 32, 32, 0.8)");
  expect(s.blur).toContain("blur(24px)");
  expect(await style(page, "nav.tabs a.on")).toMatchObject({ color: "rgb(10, 132, 255)", background: "rgba(10, 10, 10, 0.92)" });
  expect((await style(page, 'nav.tabs a[href="/library"]')).color).toBe("rgb(255, 255, 255)");
  await context.close();
});

test("phone: Calendar is a row at the top of Library, and Library stays lit on it", async ({ browser }) => {
  const { context, page } = await open(browser, "phone", "/library");
  const row = page.locator(".cal-row");
  await expect(row).toHaveText("Calendar", { timeout: 30_000 });
  await row.tap();
  await expect(page).toHaveURL(/\/calendar/);
  await expect(page.locator("nav.tabs a.on")).toHaveText("Library");
  await expect(page).toHaveTitle("Calendar · Momotus");
  await context.close();
});

test("TV: two to four choices are the segmented control, more a pull-down whose list keeps the remote", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/discover");
  const kinds = page.locator('[data-fgroup="kinds"] .chip');
  await expect(kinds.first()).toBeVisible({ timeout: 30_000 });
  expect(await kinds.count()).toBeLessThanOrEqual(4);
  // The selected segment one grey step up with a white label, the rest on the track.
  await page.locator('[data-fgroup="kinds"] .chip:not(.on)').first().focus();
  await page.locator('[data-f="genres:menu"]').focus();
  expect(await style(page, '[data-fgroup="kinds"] .chip.on')).toMatchObject({ background: "rgb(99, 99, 102)", color: "rgb(255, 255, 255)", shadow: "none" });
  expect(await style(page, '[data-fgroup="kinds"] .chip:not(.on)')).toMatchObject({ background: "rgba(118, 118, 128, 0.24)", color: "rgba(255, 255, 255, 0.64)" });
  // More than four: a pill that names the choice.
  const pill = page.locator('[data-f="genres:menu"]');
  await expect(pill).toBeVisible();
  const chosen = (await pill.textContent())?.trim();
  await pill.focus();
  await page.keyboard.press("Enter");
  const list = page.locator('[data-fgroup="genres"] .pull-list');
  await expect(list).toBeVisible();
  // It opens on the chosen option; Down moves in the list; Right does not leave it.
  await expect.poll(() => page.evaluate(() => document.activeElement?.getAttribute("aria-selected"))).toBe("true");
  const first = await focusedKey(page);
  await page.keyboard.press("ArrowDown");
  const second = await focusedKey(page);
  expect(second).not.toBe(first);
  expect(second).toMatch(/^genres:/);
  await page.keyboard.press("ArrowRight");
  expect(await focusedKey(page)).toBe(second);
  // Back closes it, on the pill, with the choice as it was.
  await page.keyboard.press("Escape");
  await expect(list).toHaveCount(0);
  expect(await focusedKey(page)).toBe("genres:menu");
  await expect(pill).toHaveText(chosen ?? "");
  // Choosing one closes it and names the new choice on the pill.
  await page.keyboard.press("Enter");
  await expect(list).toBeVisible();
  await page.keyboard.press("ArrowDown");
  const picked = (await page.evaluate(() => document.activeElement?.querySelector("b")?.textContent)) ?? "";
  await page.keyboard.press("Enter");
  await expect(list).toHaveCount(0);
  await expect(pill).toContainText(picked);
  await expect.poll(() => focusedKey(page)).toBe("genres:menu");
  await context.close();
});

test("desktop: a press outside a pull-down closes it", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/discover");
  const pill = page.locator('[data-f="genres:menu"]');
  await expect(pill).toBeVisible({ timeout: 30_000 });
  await pill.click();
  await expect(page.locator(".pull-list")).toBeVisible();
  await page.locator(".browse h1").click();
  await expect(page.locator(".pull-list")).toHaveCount(0);
  await context.close();
});

test("TV: every catalog row is 2:3 posters, movies and series, from the tile tokens", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/");
  await expect(page.locator(".row").nth(1).locator(".tile").first()).toBeVisible({ timeout: 30_000 });
  // Rows mount their tiles near the screen: walk down the page, reading each row that has them, with
  // its body's height (art, the pads and the caption: 84, the name and the year; a poster's scores are
  // in its RPDB cover, as Stremio's rows).
  const shapes: { kind?: string; w?: number; h?: number; body?: number }[] = [];
  for (let i = 0; i < 8; i++) {
    shapes.push(
      ...(await page.evaluate(() =>
        [...document.querySelectorAll(".row")].slice(1).flatMap((r) => {
          const t = r.querySelector<HTMLAnchorElement>(".tile");
          const pic = t?.querySelector<HTMLElement>(".pic");
          return t ? [{ kind: t.getAttribute("href")?.split("/")[2], w: pic?.offsetWidth, h: pic?.offsetHeight, body: r.querySelector<HTMLElement>(".row-body")?.offsetHeight }] : [];
        }),
      )),
    );
    await page.evaluate(() => document.querySelector("[data-vscroll]")?.scrollBy({ top: 700 }));
    await page.waitForTimeout(250);
  }
  const movie = shapes.find((s) => s.kind === "movie");
  const series = shapes.find((s) => s.kind === "series");
  expect(movie).toMatchObject({ w: 217, h: 326, body: 326 + 2 * 34 + 84 });
  expect(series).toMatchObject({ w: 217, h: 326, body: 326 + 2 * 34 + 84 });
  await context.close();
});

test("TV: the top shelf is full bleed, the first row's title at 850, and nothing on the art blurs or filters", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/");
  // The logo held back a moment: the column must not move when it lands (its box is fixed).
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  await context.route(/images\.metahub\.space\/logo\//, async (route) => {
    await held;
    await route.continue();
  });
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  const column = () =>
    page.evaluate(() => [".hero-page.on .genre", ".hero-page.on .meta", ".hero-page.on .reason", '[data-f="hero:details"]'].map((s) => Math.round(document.querySelector(s)?.getBoundingClientRect().top ?? -1)));
  const before = await column();
  release();
  await expect(page.locator(".hero-page.on .hero-logo:not(.pending)")).toBeVisible({ timeout: 30_000 });
  await page.waitForTimeout(300);
  expect(await column()).toEqual(before);
  const geo = await page.evaluate(() => {
    const r = (s: string) => document.querySelector(s)?.getBoundingClientRect();
    const hero = r(".hero") as DOMRect;
    const logo = r(".hero-page.on .hero-logo") as DOMRect;
    return { hero: [hero.left, hero.top, hero.width, hero.height], logo: [logo.width, logo.height], row: Math.round(r(".row .row-head h2")?.top ?? -1), details: Math.round(r('[data-f="hero:details"]')?.bottom ?? -1) };
  });
  expect(geo).toEqual({ hero: [0, 0, 1920, 1080], logo: [640, 160], row: 850, details: 770 });
  // Nothing over the art makes a live render pass: no filter, no backdrop filter (a failed tile's dimmed art aside).
  const filtered = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".hero, .hero *, .row:first-of-type .tile, .row:first-of-type .tile *")]
      .filter((el) => !el.closest(".tile-slot.failed"))
      .filter((el) => {
        const c = getComputedStyle(el);
        return c.filter !== "none" || c.backdropFilter !== "none";
      })
      .map((el) => el.className || el.tagName),
  );
  expect(filtered).toEqual([]);
  // The rising row's lanes take no press meant for the hero's buttons.
  for (const key of ["hero:play", "hero:details"]) {
    const hit = await page.locator(`[data-f="${key}"]`).evaluate((el) => {
      const b = el.getBoundingClientRect();
      return el.contains(document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2));
    });
    expect(hit, key).toBe(true);
  }
  await context.close();
});

test("desktop: tv.apple.com's hero at every window, its column where the glass check measures it", async ({ browser }) => {
  for (const [w, h] of [[1440, 900], [1280, 720], [900, 600], [1440, 500], [1920, 1080], [1440, 1200]] as const) {
    const context = await browser.newContext({ viewport: { width: w, height: h }, reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto("/?form=desktop");
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".row .row-head h2").first()).toBeVisible({ timeout: 30_000 });
    // The core's Play lands a moment after its page (its slot is held meanwhile, with no key to measure).
    await expect(page.locator('.hero-page.on [data-f="hero:play"]')).toBeVisible({ timeout: 30_000 });
    const g = await page.evaluate(() => {
      const r = (s: string) => document.querySelector(s)?.getBoundingClientRect() as DOMRect;
      const text = (s: string) => {
        const range = document.createRange();
        range.selectNodeContents(document.querySelector(s) as Element);
        const rs = [...range.getClientRects()];
        return { top: rs[0]?.top ?? -1, right: Math.max(...rs.map((x) => x.right)) };
      };
      const hero = r(".hero");
      return { left: hero.left, top: hero.top, width: hero.width, height: hero.height, foot: hero.bottom, row: r(".row .row-head h2").top, genre: text(".hero-page.on .genre").top, meta: text(".hero-page.on .meta").top, syn: text(".hero-page.on .hero-syn"), reason: text(".hero-page.on .reason").top, play: r('[data-f="hero:play"]').bottom };
    });
    const label = `${w}x${h}`;
    // Under the 52 px header, 16:9 of the width, no taller than the window under the header less 90, no lower than 400.
    const want = Math.max(Math.min((w * 9) / 16, h - 52 - 90), 400);
    expect([g.left, Math.round(g.top), Math.round(g.width)], label).toEqual([0, 52, w]);
    expect(Math.abs(g.height - want), label).toBeLessThanOrEqual(1);
    // The first shelf's title 12 below the foot, inside the window.
    expect(Math.abs(g.row - g.foot - 12), label).toBeLessThanOrEqual(1.5);
    expect(g.row, label).toBeLessThan(h - 20);
    // The column's lines from the foot, as art.test.ts takes them: genre 254, meta 225, synopsis 198, reason 147, buttons 75.
    for (const [name, top, up] of [["genre", g.genre, 254], ["meta", g.meta, 225], ["synopsis", g.syn.top, 198], ["reason", g.reason, 147]] as const) {
      expect(Math.abs(g.foot - top - up), `${label} ${name}`).toBeLessThanOrEqual(2);
    }
    expect(Math.abs(g.foot - g.play - 75), label).toBeLessThanOrEqual(1.5);
    expect(g.syn.right, label).toBeLessThanOrEqual(Math.min(600, 0.48 * w) + 1);
    await context.close();
  }
});

/** One pixel of the page as drawn, read back from a screenshot through a canvas. */
async function pixel(page: Page, x: number, y: number): Promise<number[]> {
  const png = await page.screenshot({ clip: { x, y, width: 1, height: 1 }, type: "png" });
  return page.evaluate(async (b64) => {
    const img = new Image();
    img.src = `data:image/png;base64,${b64}`;
    await img.decode();
    const c = document.createElement("canvas");
    c.width = c.height = 1;
    const g = c.getContext("2d") as CanvasRenderingContext2D;
    g.drawImage(img, 0, 0);
    return [...g.getImageData(0, 0, 1, 1).data.slice(0, 3)];
  }, png.toString("base64"));
}

test("the hero's foot meets the page with no seam, on every form", async ({ browser }) => {
  for (const form of ["tv", "desktop", "phone"] as const) {
    const { context, page } = await open(browser, form, "/");
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".hero-page.on .hero-bd img.in")).toBeAttached({ timeout: 30_000 });
    await page.waitForTimeout(400);
    const { foot, width } = await page.evaluate(() => ({ foot: document.querySelector(".hero")!.getBoundingClientRect().bottom, width: innerWidth }));
    const bg = (await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).match(/\d+/g)!.map(Number);
    // The hero's last row of pixels and the page under it are both the page color, across the width (the
    // vignette is darkest at the right). TV's first row rises over its foot, so TV reads its last row only.
    // TV: right of Continue Watching's two tiles, which rise over the foot.
    for (const x of form === "tv" ? [Math.round(width * 0.75), width - 4] : [Math.round(width * 0.25), Math.round(width * 0.5), width - 4]) {
      const rows = form === "tv" ? [foot - 2] : [foot - 2, foot + 2];
      for (const y of rows) {
        const got = await pixel(page, x, Math.round(y));
        for (let i = 0; i < 3; i++) expect(Math.abs(got[i]! - bg[i]!), `${form} at ${x},${Math.round(y)}: ${got} against ${bg}`).toBeLessThanOrEqual(3);
      }
    }
    await context.close();
  }
});

test("TV: over white art, the genre line's spot reads as the legibility check computes it", async ({ browser }) => {
  // The check (art.test.ts) composites the ramps by hand; here the browser paints them over a white backdrop
  // and the pixel under the genre line's spot (TV_EDGE 0.4688, 0.4083) is read with the column's words hidden.
  const { context, page } = await open(browser, "tv", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await page.evaluate((white) => {
    const img = document.querySelector<HTMLImageElement>(".hero-bd img");
    if (img) img.src = white;
    for (const el of document.querySelectorAll<HTMLElement>(".hero-body")) el.style.visibility = "hidden";
  }, "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC");
  await page.waitForTimeout(500);
  const [x, y] = [Math.round(0.4688 * 1920), Math.round(0.4083 * 1080)];
  const got = await pixel(page, x, y);
  // The model: white art under the hero ramps in TV's page color, the bottom layer first.
  const page_ = [28, 28, 30];
  const alphaAt = ({ deg, stops }: Layer) => {
    const r = (deg * Math.PI) / 180;
    const len = Math.abs(1920 * Math.sin(r)) + Math.abs(1080 * Math.cos(r));
    const t = 0.5 + ((x / 1920 - 0.5) * 1920 * Math.sin(r) - (y / 1080 - 0.5) * 1080 * Math.cos(r)) / len;
    const i = stops.findIndex(([at]) => at >= t);
    if (i === 0) return stops[0]![1];
    if (i === -1) return stops[stops.length - 1]![1];
    const [[p0, a0], [p1, a1]] = [stops[i - 1]!, stops[i]!];
    return a0 + ((t - p0) / (p1 - p0)) * (a1 - a0);
  };
  const model = [...ramps.hero].reverse().reduce((c, l) => c.map((v, j) => page_[j]! * alphaAt(l) + v * (1 - alphaAt(l))), [255, 255, 255]);
  // The vignette only darkens: the painted pixel is no lighter than the model, and not far darker.
  for (let j = 0; j < 3; j++) {
    expect(got[j]!, `${got} against the model ${model.map(Math.round)}`).toBeLessThanOrEqual(model[j]! + 3);
    expect(got[j]!, `${got} against the model ${model.map(Math.round)}`).toBeGreaterThanOrEqual(model[j]! - 30);
  }
  await context.close();
});

test("phone: the iPhone's hero at every screen, the large title and the profile button over it", async ({ browser }) => {
  for (const [w, h] of [[360, 640], [375, 667], [390, 844], [430, 932]] as const) {
    const context = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: 2, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
    const page = await context.newPage();
    await page.goto("/?form=phone");
    await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(".row .row-head").first()).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('.hero-page.on [data-f="hero:play"]')).toBeVisible({ timeout: 30_000 });
    const g = await page.evaluate(() => {
      const r = (s: string) => document.querySelector(s)?.getBoundingClientRect() as DOMRect;
      const tabs = r("nav.tabs");
      return { hero: [r(".hero").left, r(".hero").top, r(".hero").width, r(".hero").height], foot: r(".hero").bottom, head: [r(".row .row-head").top, r(".row .row-head").bottom], tabs: tabs.top, lines: r(".hero-page.on .hero-lines").top, reason: r(".hero-page.on .reason").top, play: r('[data-f="hero:play"]').bottom, title: r(".ph-title").top };
    });
    const label = `${w}x${h}`;
    // 680 at 390 wide, never taller than the window less 164: the first shelf's header shows above the tab bar.
    const want = Math.min((w * 680) / 390, h - 164);
    expect([g.hero[0], g.hero[1], g.hero[2]], label).toEqual([0, 0, w]);
    expect(Math.abs((g.hero[3] as number) - want), label).toBeLessThanOrEqual(1);
    expect(Math.abs(g.head[0]! - g.foot), label).toBeLessThanOrEqual(1);
    expect(g.head[1]!, label).toBeLessThanOrEqual(g.tabs);
    // The column from the foot, as art.test.ts takes it: the lines 166, the reason 140, the buttons 56.
    expect(Math.abs(g.foot - g.lines - 166), `${label} lines`).toBeLessThanOrEqual(3);
    expect(Math.abs(g.foot - g.reason - 140), `${label} reason`).toBeLessThanOrEqual(3);
    expect(Math.abs(g.foot - g.play - 56), `${label} buttons`).toBeLessThanOrEqual(1.5);
    expect(g.title, label).toBeGreaterThanOrEqual(47);
    await context.close();
  }
  // "Home" is the page's heading, the hero's title the next level; the profile is the capsule's, not the header's.
  const { context, page } = await open(browser, "phone", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Home");
  await expect(page.locator(".hero-page.on").getByRole("heading", { level: 2 })).toHaveCount(1);
  await expect(page.locator(".ph-top .pavatar")).toHaveCount(0);
  await context.close();
});

/** Where a secondary's label starts and ends over a backdrop, as fractions of the backdrop: the
 * glass check (src/components/art.test.ts) composites the ramps at these spots, so a layout that
 * moves a label past one leaves the check measuring the wrong place. */
const spot = (page: Page, selector: string, backdrop: string, maxWidth = false) =>
  page.evaluate(
    ([selector, backdrop, maxWidth]) => {
      const el = document.querySelector(selector) as HTMLElement;
      const bd = (document.querySelector(backdrop) as HTMLElement).getBoundingClientRect();
      const r = el.getBoundingClientRect();
      const k = r.width / el.offsetWidth || 1;
      const right = maxWidth ? r.left + parseFloat(getComputedStyle(el).maxWidth) * k : r.right;
      return { x: (right - bd.left) / bd.width, y: (r.top - bd.top) / bd.height };
    },
    [selector, backdrop, maxWidth] as const,
  );

test("TV: the secondaries over a backdrop sit where the glass check measures them", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", "/");
  await expect(page.locator(".hero-page.on .reason")).toBeVisible({ timeout: 30_000 });
  // The top shelf's column (art.test.ts, TV_EDGE 0.4688 for every line): each line's top no higher than
  // the check measures it, the column no wider, Details' label no further right.
  for (const [selector, y] of [[".hero-page.on .genre", 0.4083], [".hero-page.on .hero-syn", 0.4509], [".hero-page.on .meta", 0.5491], [".hero-page.on .reason", 0.6046]] as const) {
    // Over the page's ramps, which the glass check composites: the art box itself reaches past the frame (B2).
    const at = await spot(page, selector, ".hero-page.on .hero-bd .ramp", true);
    expect(at.x, selector).toBeLessThanOrEqual(0.4688 + 0.002);
    expect(at.y, selector).toBeGreaterThanOrEqual(y - 0.005);
  }
  const details = await spot(page, '[data-f="hero:details"]', ".hero-page.on .hero-bd .ramp");
  expect(details.x).toBeLessThanOrEqual(0.3225 + 34 / 1920 + 0.002);
  expect(details.y).toBeGreaterThanOrEqual(0.663 - 18 / 1080 - 0.005);
  await page.goto(`/title/series/${GOT.id}?form=tv`);
  await expect(page.locator('[data-f="title:startover"]')).toBeVisible({ timeout: 30_000 });
  const over = await spot(page, '[data-f="title:startover"]', ".title-bd");
  expect(over.x).toBeLessThanOrEqual(0.4398 + 0.002);
  expect(over.y).toBeGreaterThanOrEqual(0.3817 - 0.005);
  await context.close();
});
