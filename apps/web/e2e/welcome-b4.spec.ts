import { type BrowserContext, expect, type Page, test } from "@playwright/test";
import { drawnContrast, WHITE_PNG } from "./contrast";
import { PORTS } from "./core";
import { patchedContext } from "./patch";

/** First run B4 (design/DECISION.md, the owner's pick of 2026-10-28): the question steps are a 2.39:1 film frame
 * holding a still with the question as its subtitle, the choices in the bar under it. */

type Form = "tv" | "desktop" | "phone";
const SIZE: Record<Form, [number, number]> = { tv: [1920, 1080], desktop: [1440, 900], phone: [390, 844] };

async function open(context: BrowserContext, form: Form, step: string) {
  const page = await context.newPage();
  await page.goto(`/welcome?step=${step}&form=${form}`);
  await expect(page.locator(".welcome h1").first()).toBeVisible({ timeout: 30_000 });
  return page;
}
/** A device whose profile has no subtitle language yet, whatever other specs set on the shared core: the subtitles
 * step shows instead of passing by. `raw`: the core's messages as they are, for a spec that routes them itself. */
const device = async (browser: import("@playwright/test").Browser, form: Form, size = SIZE[form], raw = false) => {
  const options = { viewport: { width: size[0], height: size[1] }, isMobile: form === "phone", hasTouch: form === "phone", reducedMotion: "reduce" as const };
  if (raw) return browser.newContext(options);
  const { context } = await patchedContext(browser, PORTS.bridge, options, (view, snapshot) => (view === "settings" ? { ...snapshot, subtitleLanguages: [] } : snapshot));
  return context;
};
const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? null);
/** The core answers `firstRunStill` with nothing (offline, past its budget). */
const noStill = (context: BrowserContext) =>
  context.routeWebSocket(/\/ws$/, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      if (m.op === "firstRunStill") ws.send(JSON.stringify({ id: m.id, ok: true, result: null }));
      else server.send(String(raw));
    });
  });

for (const [form, size] of [["tv", [1920, 1080]], ["desktop", [1440, 900]], ["desktop", [1280, 720]], ["desktop", [1920, 1080]], ["phone", [390, 844]], ["phone", [844, 390]]] as const) {
  test(`${form} ${size.join("x")}: the frame is 2.39:1, as wide as the height the header and bar leave allows, centered`, async ({ browser }) => {
    const context = await device(browser, form, size as [number, number]);
    const page = await open(context, form, "doors");
    const box = await page.locator(".film").evaluate((f) => {
      const r = f.getBoundingClientRect();
      const main = document.querySelector(".welcome.page") as HTMLElement;
      return { w: r.width, h: r.height, left: r.left, right: innerWidth - r.right, vw: innerWidth, vh: innerHeight, scrolls: main.scrollHeight > main.clientHeight + 1 };
    });
    expect(box.w / box.h).toBeCloseTo(2.39, 1);
    expect(Math.abs(box.left - box.right)).toBeLessThanOrEqual(2);
    // The gallery's desktop: the picture edge to edge, 602 high under a 149 header, over a 149 bar.
    if (form === "desktop" && size[0] === 1440) {
      expect(Math.abs(box.w - 1440)).toBeLessThanOrEqual(2);
      expect(Math.abs(box.h - 602)).toBeLessThanOrEqual(2);
    }
    // TV and desktop: header, frame and bar fit the screen, nothing to scroll for; the frame takes most of the width.
    if (form !== "phone") {
      expect(box.scrolls, "the doors step fits the screen").toBe(false);
      expect(box.w).toBeGreaterThanOrEqual(box.vw * 0.75);
    }
    // The phone's frame spans the width.
    if (form === "phone" && size[0] < size[1]) expect(box.w).toBeGreaterThanOrEqual(box.vw - 1);
    // The bar holds every choice under the frame, inside the screen's width.
    const doors = await page.locator(".framed .door").evaluateAll((ds, film) => ds.map((d) => d.getBoundingClientRect()).every((r) => r.top >= (film as number) && r.right <= innerWidth + 1), box.h);
    expect(doors).toBe(true);
    await context.close();
  });
}

test("only the question steps are framed: the doors, the stream question and subtitles", async ({ browser }) => {
  const context = await device(browser, "desktop");
  for (const [step, framed] of [["doors", 1], ["choice", 1], ["subs", 1], ["2", 0], ["3", 0]] as const) {
    const page = await open(context, "desktop", step);
    await expect(page.locator(".film"), `step ${step}`).toHaveCount(framed);
    if (framed) await expect(page.locator(".film h1")).toHaveAttribute("id", "welcome-q");
    if (framed) await expect(page.locator('[aria-labelledby="welcome-q"][role]')).toHaveCount(1);
    await page.close();
  }
  await context.close();
});

test("TV: the first choice takes the focus on open", async ({ browser }) => {
  const context = await device(browser, "tv");
  for (const [step, first] of [["doors", "welcome:stremio"], ["choice", "welcome:fast"]] as const) {
    const page = await open(context, "tv", step);
    await expect.poll(() => focused(page)).toBe(first);
    await page.close();
  }
  await context.close();
});

test("desktop: a step change puts the focus on the step's heading", async ({ browser }) => {
  const context = await device(browser, "desktop");
  const page = await open(context, "desktop", "doors");
  await page.locator('[data-f="welcome:fresh"]').click();
  await expect(page).toHaveURL(/step=2/);
  await expect.poll(() => page.evaluate(() => document.activeElement?.tagName)).toBe("H1");
  await page.locator('[data-f="welcome:back"]').click();
  await expect(page).toHaveURL(/step=doors|welcome$|welcome\?form/);
  await expect.poll(() => page.evaluate(() => document.activeElement?.id)).toBe("welcome-q");
  await context.close();
});

test("no still: the frame stays black with its question", async ({ browser }) => {
  const context = await device(browser, "desktop", SIZE.desktop, true);
  await noStill(context);
  const page = await open(context, "desktop", "doors");
  await expect(page.locator(".film h1")).toHaveText("How would you like to begin?");
  await page.waitForTimeout(500);
  await expect(page.locator(".film img")).toHaveCount(0);
  expect(await page.locator(".film").evaluate((f) => getComputedStyle(f).backgroundColor)).toBe("rgb(0, 0, 0)");
  await context.close();
});

for (const form of ["tv", "desktop", "phone"] as const) {
  test(`${form}: the question keeps 4.5:1 over a white still`, async ({ browser }) => {
    const context = await device(browser, form);
    const first = await open(context, form, "doors");
    const still = first.locator(".film img");
    await expect(still).toHaveClass(/\bin\b/, { timeout: 30_000 });
    const src = (await still.getAttribute("src")) as string;
    await first.close();
    await context.route(src, (r) => r.fulfill({ body: WHITE_PNG, contentType: "image/png" }));
    for (const step of ["doors", "choice", "subs"]) {
      const page = await open(context, form, step);
      await expect(page.locator(".film img")).toHaveClass(/\bin\b/, { timeout: 30_000 });
      const measured = await drawnContrast(page, ".film-cue", ["h1 span", "p span"], []);
      expect(measured.length).toBeGreaterThanOrEqual(1);
      for (const m of measured) expect(m.ratio, `${step} ${m.what}: ${m.ratio}`).toBeGreaterThanOrEqual(m.floor);
      await page.close();
    }
    await context.close();
  });
}

for (const [form, size] of [["tv", [1920, 1080]], ["desktop", [1440, 900]], ["desktop", [1920, 1200]]] as const) {
  test(`${form} ${size.join("x")}: the frame and its bar sit in the middle of the space under the header`, async ({ browser }) => {
    const context = await device(browser, form, size as [number, number]);
    for (const step of ["doors", "choice"]) {
      const page = await open(context, form, step);
      await expect(page.locator(".framed .door").first()).toBeVisible();
      const gaps = await page.evaluate(() => {
        const main = document.querySelector(".welcome.page") as HTMLElement;
        const k = main.getBoundingClientRect().height / main.offsetHeight;
        const bottom = main.getBoundingClientRect().bottom - parseFloat(getComputedStyle(main).paddingBottom) * k;
        const head = (document.querySelector(".welcome-head") as HTMLElement).getBoundingClientRect().bottom;
        const block = document.querySelector(".step-body.framed") as HTMLElement;
        const top = (block.querySelector(".film") as HTMLElement).getBoundingClientRect().top;
        const end = Math.max(...[...block.children].map((c) => c.getBoundingClientRect().bottom));
        return { above: top - head, below: bottom - end };
      });
      expect(gaps.above, `${step} under the header`).toBeGreaterThanOrEqual(0);
      expect(Math.abs(gaps.above - gaps.below), `${step} ${JSON.stringify(gaps)}`).toBeLessThanOrEqual(4);
      await page.close();
    }
    await context.close();
  });
}

for (const [form, size] of [["tv", [1920, 1080]], ["desktop", [1440, 900]], ["desktop", [1920, 1080]]] as const) {
  test(`${form} ${size.join("x")}: the header and the bar keep one column, the frame runs past it`, async ({ browser }) => {
    const context = await device(browser, form, size as [number, number]);
    const page = await open(context, form, "doors");
    await expect(page.locator(".framed .door")).toHaveCount(3);
    const at = await page.evaluate(() => {
      const r = (s: string) => (document.querySelector(s) as HTMLElement).getBoundingClientRect();
      // The bar's own box (a focused door's lift would scale its edge).
      const bar = r(".framed .doors");
      return { brand: r(".welcome-head .brand").left, skip: r('[data-f="welcome:skip"]').right, first: bar.left, last: bar.right, film: r(".film") };
    });
    expect(Math.abs(at.first - at.brand), "the bar starts on the header's edge").toBeLessThanOrEqual(2);
    expect(Math.abs(at.last - at.skip), "the bar ends on the header's edge").toBeLessThanOrEqual(2);
    expect(at.film.left).toBeLessThan(at.brand);
    await context.close();
  });
}

// A small window takes the desktop form (a landscape phone, a small tablet): the question's type follows the
// frame, so it stays inside the picture.
for (const size of [[844, 390], [1024, 600]] as const) {
  test(`desktop ${size.join("x")}: the question and its line stay inside the frame on every framed step`, async ({ browser }) => {
    const context = await device(browser, "desktop", size as unknown as [number, number]);
    for (const step of ["doors", "choice", "subs"]) {
      const page = await open(context, "desktop", step);
      const inside = await page.evaluate(() => {
        const film = (document.querySelector(".film") as HTMLElement).getBoundingClientRect();
        return [...document.querySelectorAll<HTMLElement>(".film-cue h1, .film-cue p")].map((el) => {
          const r = el.getBoundingClientRect();
          return { what: el.tagName, ok: r.top >= film.top - 0.5 && r.bottom <= film.bottom + 0.5, r: [r.top, r.bottom], film: [film.top, film.bottom] };
        });
      });
      expect(inside.length).toBeGreaterThanOrEqual(1);
      for (const i of inside) expect(i.ok, `${step} ${JSON.stringify(i)}`).toBe(true);
      await page.close();
    }
    await context.close();
  });
}

test("phone: Back and Skip are 48 px targets with room between them", async ({ browser }) => {
  const context = await device(browser, "phone");
  const page = await open(context, "phone", "subs");
  const [back, skip] = await Promise.all(['[data-f="welcome:back"]', '[data-f="welcome:skip"]'].map((s) => page.locator(s).evaluate((el) => el.getBoundingClientRect().toJSON())));
  for (const b of [back, skip]) expect(Math.min(b.width, b.height)).toBeGreaterThanOrEqual(48);
  expect(skip.left - back.right).toBeGreaterThanOrEqual(8);
  await context.close();
});
