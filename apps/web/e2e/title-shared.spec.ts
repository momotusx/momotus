import { type Browser, expect, type Page, test } from "@playwright/test";
import { GOT } from "./core";
import { BBB_ID } from "./source";

/** The title page's shared pieces (plans/design-v3-phase34-web.md, step 1): the one Back stack, the
 * link destinations read once per navigation, the version note's reserved slot, no live render pass
 * over the art (rule 2) and the series' Library cell alone. */

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

const focusedKey = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
const path = (page: Page) => new URL(page.url()).pathname;

for (const [opener, layer] of [
  ["title:synopsis", ".syn-layer"],
  ["title:trailer", ".trailer"],
] as const) {
  // Each in a fresh page, so the overlay is the first to listen for Back: an overlay on a listener of its
  // own would then take Escape before the palette over it.
  test(`desktop: Back closes the newest overlay only, the palette over ${opener}`, async ({ browser }) => {
    const { context, page } = await open(browser, "desktop", `/title/movie/${BBB_ID}`);
    await expect(page.locator('[data-f="title:trailer"]')).toBeVisible({ timeout: 30_000 });
    await page.locator(`[data-f="${opener}"]`).click();
    await expect(page.locator(layer)).toBeVisible();
    // The title page shows no navigation, so the palette opens as the navigation opens it, through the
    // app's own module: a second overlay over the first.
    await page.evaluate(async (url) => ((await import(/* @vite-ignore */ url)) as typeof import("../src/screens/search")).openSearch(), "/src/screens/search.tsx");
    await expect(page.locator(".palette-layer .palette")).toBeVisible();
    // The palette is the newest: Escape closes it, and the overlay under it stays.
    await page.keyboard.press("Escape");
    await expect(page.locator(".palette-layer")).toHaveCount(0);
    await expect(page.locator(layer)).toBeVisible();
    // Then the overlay, with focus back on what opened it and the page where it was.
    await page.keyboard.press("Escape");
    await expect(page.locator(layer)).toHaveCount(0);
    await expect.poll(() => focusedKey(page)).toBe(opener);
    expect(path(page)).toBe(`/title/movie/${BBB_ID}`);
    await context.close();
  });
}

test("TV: the synopsis opens on its own card, Back closes it and returns to the synopsis", async ({ browser }) => {
  const { context, page } = await open(browser, "tv", `/title/movie/${BBB_ID}`);
  await expect.poll(() => focusedKey(page), { timeout: 30_000 }).toBe("title:play");
  // A synopsis longer than its clamp, so an unclamp would show.
  await page.locator(".syn").evaluate((el) => (el.textContent = `${el.textContent} `.repeat(8)));
  // The column's pieces, each: the column's own box can hold still while a grown synopsis pushes them.
  const column = () => page.evaluate(() => [...document.querySelectorAll(".info > *")].map((el) => JSON.stringify(el.getBoundingClientRect())));
  const info = await column();
  // Focus on the synopsis opens nothing in place either.
  await page.locator('[data-f="title:synopsis"]').focus();
  await page.waitForTimeout(300);
  expect(await column()).toEqual(info);
  await page.keyboard.press("Enter");
  await expect(page.locator(".syn-layer[role=dialog]")).toBeVisible();
  await expect.poll(() => focusedKey(page)).toBe("synopsis:close");
  await expect(page.locator(".title-main")).toHaveAttribute("inert", /.*/);
  // Nothing opened in place: the column under the card is where it was.
  expect(await column()).toEqual(info);
  const card = await page.locator(".syn-card").boundingBox();
  expect(card!.y).toBeGreaterThanOrEqual(0);
  expect(card!.y + card!.height).toBeLessThanOrEqual(1080);
  await page.keyboard.press("Escape");
  await expect(page.locator(".syn-layer")).toHaveCount(0);
  await expect.poll(() => focusedKey(page)).toBe("title:synopsis");
  expect(path(page)).toBe(`/title/movie/${BBB_ID}`);
  await context.close();
});

test("phone: the browser's Back closes the synopsis and the trailer and keeps the page; More gets the focus back", async ({ browser }) => {
  const { context, page } = await open(browser, "phone", `/title/movie/${BBB_ID}`);
  await expect(page.locator('[data-f="title:more"]')).toBeVisible({ timeout: 30_000 });
  const overlay = () => page.evaluate(() => (history.state as { _overlay?: number } | null)?._overlay ?? null);
  for (const [opener, layer] of [
    ["title:more", ".syn-layer"],
    ["title:trailer", ".trailer"],
  ] as const) {
    // An edge swipe or a mouse's Back: the overlay closes, the title stays.
    await page.locator(`[data-f="${opener}"]`).tap();
    await expect(page.locator(layer)).toBeVisible();
    expect(await overlay()).not.toBeNull();
    // The card fits the phone's width.
    const card = (await page.locator(layer === ".syn-layer" ? ".syn-card" : ".trailer-frame").boundingBox())!;
    expect(card.x).toBeGreaterThanOrEqual(0);
    expect(card.x + card.width).toBeLessThanOrEqual(390);
    await page.goBack();
    await expect(page.locator(layer)).toHaveCount(0);
    expect(path(page)).toBe(`/title/movie/${BBB_ID}`);
    // Closed by its own button: its entry goes with it, so the next Back is the page's.
    await page.locator(`[data-f="${opener}"]`).tap();
    await expect(page.locator(layer)).toBeVisible();
    await page.locator(layer === ".syn-layer" ? '[data-f="synopsis:close"]' : '[data-f="trailer:close"]').tap();
    await expect(page.locator(layer)).toHaveCount(0);
    await expect.poll(overlay).toBeNull();
    await expect.poll(() => focusedKey(page)).toBe(opener);
    expect(path(page)).toBe(`/title/movie/${BBB_ID}`);
  }
  await context.close();
});

const chosen = (page: Page, group: string) =>
  page.locator(`[data-fgroup="${group}"]`).evaluate((row) => {
    const on = row.querySelector('[aria-checked="true"]');
    if (on) return on.textContent?.trim() ?? null;
    return row.querySelector(".pull-pill")?.getAttribute("aria-label")?.replace(/^[^:]+: /, "") ?? null;
  });

test("desktop: a genre link opens Discover at its top on that kind and genre, once; the viewer's next pick stays", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/discover");
  const grid = page.locator("main.browse");
  await expect(page.locator(".browse .tile, .browse a[href^='/title/']").first()).toBeVisible({ timeout: 30_000 });
  // A visit scrolled down, then Home: Discover keeps that place for a plain visit.
  await grid.evaluate((el) => el.scrollTo({ top: 600 }));
  await expect.poll(() => grid.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await page.locator('[data-f="nav:/"]').click();
  await expect.poll(() => path(page)).toBe("/");
  // The link, as a chip navigates (the router's own history push).
  const follow = () =>
    page.evaluate(() => {
      history.pushState(null, "", "/discover?kind=series&genre=drama&form=desktop");
      dispatchEvent(new PopStateEvent("popstate"));
    });
  await follow();
  await expect.poll(() => chosen(page, "kinds"), { timeout: 30_000 }).toBe("Series");
  await expect.poll(() => chosen(page, "genres"), { timeout: 30_000 }).toBe("Drama");
  expect(await grid.evaluate((el) => el.scrollTop)).toBe(0);
  // Dropped from the URL with a replace: no entry was added for it.
  await expect.poll(() => new URL(page.url()).searchParams.has("genre")).toBe(false);
  // The same link again after scrolling that choice down: nothing to select, and it still lands at the top.
  await expect(page.locator(".browse a[href^='/title/']").nth(20)).toBeAttached({ timeout: 30_000 });
  await grid.evaluate((el) => el.scrollTo({ top: 600 }));
  await expect.poll(() => grid.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await page.locator('[data-f="nav:/"]').click();
  await expect.poll(() => path(page)).toBe("/");
  await follow();
  await expect.poll(() => chosen(page, "genres"), { timeout: 30_000 }).toBe("Drama");
  await page.waitForTimeout(300);
  expect(await grid.evaluate((el) => el.scrollTop)).toBe(0);
  // The viewer picks another genre: it stays, through later answers and through a visit away and Back.
  await page.locator('[data-f="genres:menu"]').click();
  const other = page.locator('[data-fgroup="genres"] .pull-opt:not(.on)').nth(1);
  const name = (await other.locator("b").textContent())!.trim();
  await other.click();
  await expect.poll(() => chosen(page, "genres")).toBe(name);
  await page.waitForTimeout(1500);
  expect(await chosen(page, "genres")).toBe(name);
  await page.locator('[data-f="nav:/"]').click();
  await expect.poll(() => path(page)).toBe("/");
  await page.goBack();
  await expect.poll(() => path(page)).toBe("/discover");
  await expect.poll(() => chosen(page, "genres"), { timeout: 30_000 }).toBe(name);
  await context.close();
});

test("desktop: a genre the catalog does not list leaves Discover on the kind with no genre", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/discover?kind=series&genre=Not%20A%20Genre");
  await expect.poll(() => chosen(page, "kinds"), { timeout: 30_000 }).toBe("Series");
  await expect.poll(() => chosen(page, "genres"), { timeout: 30_000 }).toBe("All");
  await context.close();
});

test("desktop: a name link opens Search on that query once; what the viewer types since stays", async ({ browser }) => {
  const { context, page } = await open(browser, "desktop", "/search?q=thrones");
  const field = page.locator(".search-page input");
  await expect(field).toHaveValue("thrones", { timeout: 30_000 });
  await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => new URL(page.url()).searchParams.has("q")).toBe(false);
  await field.fill("Bunny");
  await page.locator('[data-f="nav:/"]').click();
  await expect.poll(() => path(page)).toBe("/");
  await page.goBack();
  await expect.poll(() => path(page)).toBe("/search");
  await expect(page.locator(".search-page input")).toHaveValue("Bunny");
  await context.close();
});

// What the note can carry: every one of the core's `empty` lines (needsAccount with its count), and a failure with a long reason
// (problemWords passes an unknown one through), read from the app's own module (its JSON imports load in
// the page, not in the runner).
const noteWords = (page: Page) =>
  page.evaluate(async (url) => {
    const w = (await import(/* @vite-ignore */ url)) as typeof import("../src/lib/words");
    // needsAccount is said with its count (title.tsx, VersionNote): a four-digit count is the longest.
    const lines = [...Object.values(w.choiceWords.empty).map((e) => e.text), w.blockedWords("needsAccount", 1234).text];
    const failed = `Versions could not load. ${w.problemWords(`the core said ${"something long ".repeat(30)}`).text}`;
    return { lines, failed };
  }, "/src/lib/words.ts");

for (const form of Object.keys(FORMS) as Form[]) {
  test(`${form}: the version note's slot is there from the first paint, as tall as its longest line and no taller`, async ({ browser }) => {
    const { context, page } = await open(browser, form, `/title/movie/${BBB_ID}`);
    await expect(page.locator('[data-f="title:play"]')).toBeVisible({ timeout: 30_000 });
    const note = page.locator(".vnote[role=status]");
    await expect(note).toBeEmpty();
    const rects = () =>
      page.evaluate(() =>
        [...document.querySelectorAll(".info > *, .info .acts, .info .play-row")].map((el) => {
          const r = el.getBoundingClientRect();
          // The note itself only grows as wide as its words; its height and place hold.
          return el.classList.contains("vnote") ? [el.className, Math.round(r.y), Math.round(r.height)] : [el.className, Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)];
        }),
      );
    const { lines, failed } = await noteWords(page);
    const before = await rects();
    const slot = (await note.boundingBox())!.height;
    const fill = (words: string) =>
      note.evaluate((p, words) => {
        p.innerHTML = `<span class="st warn"><i class="dot"></i><span class="vtext"></span></span>`;
        (p.querySelector(".vtext") as HTMLElement).textContent = words;
        return (p.querySelector(".st") as HTMLElement).getBoundingClientRect().height;
      }, words);
    // Every line fits the slot and moves nothing; the longest needs all of it (the slot is no taller).
    const needs: number[] = [];
    for (const line of lines) {
      needs.push(await fill(line));
      expect(await rects(), line).toEqual(before);
    }
    expect(Math.abs(Math.max(...needs) - slot), `slot ${slot}, lines need ${needs.join(", ")}`).toBeLessThanOrEqual(1);
    // A long failure is clamped to the slot.
    await fill(failed);
    expect(await rects()).toEqual(before);
    await context.close();
  });

  test(`${form}: nothing makes a live render pass over the title's art; a series shows the Library cell alone`, async ({ browser }) => {
    for (const title of [`/title/series/${GOT.id}?season=1`, `/title/movie/${BBB_ID}`]) {
      const { context, page } = await open(browser, form, title);
      await expect(page.locator('[data-f="title:library"]')).toBeVisible({ timeout: 30_000 });
      if (title.includes("series")) {
        await expect(page.locator('[data-f^="ep:"]').first()).toBeVisible({ timeout: 30_000 });
        await expect(page.locator('[data-f="title:watched"]')).toHaveCount(0);
      } else await expect(page.locator('[data-f="title:watched"]')).toBeVisible();
      const walk = () =>
        page.evaluate(() =>
          [...document.querySelectorAll(".title-bd, .title-bd *, .info, .info *, .eps, .eps *")].flatMap((el) => {
            const s = getComputedStyle(el);
            const live = [s.filter, s.backdropFilter, s.maskImage, s.webkitMaskImage].filter((v) => v && v !== "none");
            return live.length ? [`${el.tagName.toLowerCase()}.${el.className}: ${live.join(" ")}`] : [];
          }),
        );
      expect(await walk()).toEqual([]);
      // Focused and hovered states too: an episode row and the synopsis.
      const row = page.locator('[data-f^="ep:"]').first();
      if (await row.count()) {
        if (form === "desktop") await row.hover();
        else await row.focus();
        expect(await walk()).toEqual([]);
      }
      if (form === "desktop") await page.locator('[data-f="title:synopsis"]').hover();
      else await page.locator('[data-f="title:synopsis"]').focus();
      expect(await walk()).toEqual([]);
      await context.close();
    }
  });
}

// The title page is drawn whole from its first frame, on every form: Remove from Continue Watching comes with the
// title's own header (TV), the Versions button holds the width of its widest count, and the logo has a fixed slot,
// the logo or the title's name at its foot. Here Home and Versions answer 3 s late and the logo 1 s late (or never),
// every frame from the first that draws the page is measured, and none differs.
const LOGOS = {
  // A stacked logo, 3:2, far from the 3.2:1 a box assumes without one.
  stacked: { svg: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400"><rect width="600" height="400" rx="40" fill="#e9e2d2"/></svg>', r: "1.500" },
  // Transparent margins around a 3:1 mark: the cut's ratio is the one the page takes, not the file's 3:2.
  margins: { svg: '<svg xmlns="http://www.w3.org/2000/svg" width="600" height="400"><rect x="150" y="150" width="300" height="100" fill="#e9e2d2"/></svg>', r: "3.000" },
  // No logo comes: the name takes the slot.
  fails: { svg: null, r: null },
} as const;

for (const form of Object.keys(FORMS) as Form[]) {
  for (const [kind, logo] of Object.entries(LOGOS)) {
    test(`${form}: the page keeps its boxes from the first frame, however late Home answers and the logo (${kind}) lands`, async ({ browser }) => {
      const [width, height, scale] = FORMS[form];
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, isMobile: form === "phone", hasTouch: form === "phone", reducedMotion: "reduce" });
      await context.routeWebSocket(/\/ws$/, (ws) => {
        const server = ws.connectToServer();
        ws.onMessage((message) => {
          const text = String(message);
          let late = false;
          try {
            const m = JSON.parse(text) as { op?: string; view?: string };
            late = m.op === "subscribe" && (m.view === "home" || m.view === "versions");
          } catch {
            // Not JSON: passed on as it is.
          }
          if (late) setTimeout(() => server.send(text), 3000);
          else server.send(text);
        });
      });
      await context.route(/\/logo\//, (route) =>
        setTimeout(
          () =>
            void (logo.svg
              ? route.fulfill({ status: 200, contentType: "image/svg+xml", headers: { "access-control-allow-origin": "*" }, body: logo.svg })
              : route.fulfill({ status: 404 })),
          1000,
        ),
      );
      // Each frame's boxes (and whether Remove is there), kept when they differ from the frame before. The logo's
      // own width is free (nothing sits beside it); its slot is what holds the page.
      await context.addInitScript(() => {
        const parts = [".info .logo-slot", ".info .meta", ".info .acts", ".info .vnote", ".info .facts"];
        const frames: string[] = [];
        (window as unknown as { frames_: string[] }).frames_ = frames;
        const tick = () => {
          if (document.querySelector('[data-f="title:play"]')) {
            const boxes = parts.map((s) => {
              const r = document.querySelector(s)?.getBoundingClientRect();
              return r ? `${s} ${Math.round(r.x)},${Math.round(r.y)} ${Math.round(r.width)}x${Math.round(r.height)}` : `${s} none`;
            });
            const frame = [...boxes, `remove ${document.querySelectorAll('[data-f="title:uncontinue"]').length}`].join("; ");
            if (frames.at(-1) !== frame) frames.push(frame);
          }
          requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      });
      const page = await context.newPage();
      await page.goto(`/title/movie/${BBB_ID}?form=${form}`);
      await expect(page.locator('[data-f="title:play"]')).toBeVisible({ timeout: 30_000 });
      // Home answers after 3 s; by 4.5 s everything that comes has come.
      await page.waitForTimeout(4500);
      const frames = await page.evaluate(() => (window as unknown as { frames_: string[] }).frames_);
      expect(frames, frames.join("\n")).toHaveLength(1);
      expect(frames[0]).not.toContain(".logo-slot none");
      // Big Buck Bunny is in Continue Watching (the run's seed): on TV its Remove is there with the first frame.
      expect(frames[0]).toContain(form === "tv" ? "remove 1" : "remove 0");
      if (logo.r) {
        expect(await page.locator(".info .title-logo").evaluate((i) => getComputedStyle(i).getPropertyValue("--r"))).toBe(logo.r);
        // The logo stands on the slot's foot and inside it.
        const [slot, mark] = [await page.locator(".info .logo-slot").boundingBox(), await page.locator(".info .title-logo").boundingBox()];
        expect(slot && mark && Math.abs(mark.y + mark.height - (slot.y + slot.height))).toBeLessThanOrEqual(0.5);
        expect(slot && mark && mark.y >= slot.y - 0.5).toBe(true);
      } else {
        await expect(page.locator(".info .logo-slot .logo-text")).toHaveText("Big Buck Bunny");
      }
      await context.close();
    });
  }
}

// A long name in the slot (its logo never comes): two lines at most, the second's descenders whole, and the slot
// keeps its height (`flex: none`: it never shrinks to make room in the column).
for (const form of ["tv", "phone"] as const) {
  test(`${form}: a long name takes two lines in the logo's slot, its descenders whole, the slot its full height`, async ({ browser }) => {
    const [width, height, scale] = FORMS[form];
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, isMobile: form === "phone", hasTouch: form === "phone", reducedMotion: "reduce" });
    await context.route(/\/logo\//, (route) => route.fulfill({ status: 404 }));
    const page = await context.newPage();
    await page.goto(`/title/movie/tt0325980?form=${form}`);
    const name = page.locator(".info .logo-slot .logo-text");
    await expect(name).toHaveText("Pirates of the Caribbean: The Curse of the Black Pearl", { timeout: 30_000 });
    const slot = await page.locator(".info .logo-slot").boundingBox();
    expect(slot?.height).toBe(form === "tv" ? 192 : 96);
    const lines = await name.evaluate((h) => {
      const range = document.createRange();
      range.selectNodeContents(h);
      const rects = [...range.getClientRects()].filter((r) => r.width > 0);
      const box = h.getBoundingClientRect();
        // The lines the clamp shows (it hides the rest under the box's foot).
      const shown = rects.filter((r) => (r.top + r.bottom) / 2 < box.bottom);
      return { lines: new Set(shown.map((r) => Math.round(r.top))).size, all: new Set(rects.map((r) => Math.round(r.top))).size, overrun: Math.max(...shown.map((r) => r.bottom)) - box.bottom, slotFoot: box.bottom };
    });
    expect(lines.lines, JSON.stringify(lines)).toBe(2);
    // Each shown line's glyph box (ascent to descent) ends inside the name's box: nothing below the baseline is cut.
    expect(lines.overrun, JSON.stringify(lines)).toBeLessThanOrEqual(0.5);
    // The name stands on the slot's foot.
    expect(Math.abs(lines.slotFoot - ((slot?.y ?? 0) + (slot?.height ?? 0)))).toBeLessThanOrEqual(0.5);
    await context.close();
  });
}
