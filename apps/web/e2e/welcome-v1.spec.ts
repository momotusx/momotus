import { type BrowserContext, expect, type Page, test } from "./fixture";
import { drawnContrast, WHITE_PNG } from "./contrast";
import { PORTS } from "./core";
import { patchedContext } from "./patch";

/** First run V1, Full Bleed (design/DECISION.md, the pick of 2026-10-30, design/exploration/onboarding-v2): the
 * question steps are one film still filling the screen, the question and the choices on a ramp at the foot (phone) or
 * in a column at the lower left (TV and desktop). */

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

for (const [form, size] of [["tv", [1920, 1080]], ["desktop", [1440, 900]], ["desktop", [1280, 720]], ["phone", [390, 844]], ["phone", [360, 740]]] as const) {
  test(`${form} ${size.join("x")}: the still fills the screen and every door is on it, at the foot`, async ({ browser }) => {
    const context = await device(browser, form, size as unknown as [number, number]);
    for (const step of ["doors", "choice"]) {
      const page = await open(context, form, step);
      await expect(page.locator(".framed .door").first()).toBeVisible();
      const at = await page.evaluate(() => {
        const still = (document.querySelector(".framed .still") as HTMLElement).getBoundingClientRect();
        const head = (document.querySelector(".welcome-head") as HTMLElement).getBoundingClientRect();
        const h1 = (document.querySelector(".framed h1") as HTMLElement).getBoundingClientRect();
        const doors = [...document.querySelectorAll<HTMLElement>(".framed .door")].map((d) => d.getBoundingClientRect());
        const last = doors[doors.length - 1] as DOMRect;
        return { still: [still.left, still.top, still.width, still.height], vw: innerWidth, vh: innerHeight, head: head.bottom, top: h1.top, bottom: last.bottom, right: last.right };
      });
      // The still is the whole screen, edge to edge.
      at.still.forEach((v, i) => expect(v, `still ${i}`).toBeCloseTo([0, 0, at.vw, at.vh][i] as number, 0));
      // The question under the header, every door on the first screen, the last one at the foot: no band under it.
      expect(at.top, `${step} the question under the header`).toBeGreaterThanOrEqual(at.head - 0.5);
      expect(at.bottom, `${step} the last door on the screen`).toBeLessThanOrEqual(at.vh + 0.5);
      expect(at.vh - at.bottom, `${step} the gap under the last door`).toBeLessThanOrEqual(form === "desktop" ? 100 : 120);
      // TV and desktop: a column at the lower left, never across the picture.
      if (form !== "phone") expect(at.right).toBeLessThan(at.vw * 0.62);
      await page.close();
    }
    await context.close();
  });
}

test("only the question steps have the still: the doors, the stream question and subtitles", async ({ browser }) => {
  const context = await device(browser, "desktop");
  for (const [step, framed] of [["doors", 1], ["choice", 1], ["subs", 1], ["2", 0], ["3", 0]] as const) {
    const page = await open(context, "desktop", step);
    await expect(page.locator(".framed .still"), `step ${step}`).toHaveCount(framed);
    if (framed) await expect(page.locator(".framed h1")).toHaveAttribute("id", "welcome-q");
    if (framed) await expect(page.locator('[aria-labelledby="welcome-q"][role]')).toHaveCount(1);
    if (framed) await expect(page.locator(".framed .still")).toHaveAttribute("aria-hidden", "true");
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

test("no still: the screen stays black with its question", async ({ browser }) => {
  const context = await device(browser, "desktop", SIZE.desktop, true);
  await noStill(context);
  const page = await open(context, "desktop", "doors");
  await expect(page.locator(".framed h1")).toHaveText("How would you like to begin?");
  await page.waitForTimeout(500);
  await expect(page.locator(".framed .still img")).toHaveCount(0);
  expect(await page.locator(".framed .still").evaluate((f) => getComputedStyle(f).backgroundColor)).toBe("rgb(0, 0, 0)");
  await context.close();
});

for (const [form, size] of [["tv", SIZE.tv], ["desktop", SIZE.desktop], ["desktop", [1024, 768]], ["phone", SIZE.phone]] as const) {
  test(`${form} ${size.join("x")}: every word keeps 4.5:1 over a white still`, async ({ browser }) => {
    const context = await device(browser, form, size as unknown as [number, number]);
    const first = await open(context, form, "doors");
    const still = first.locator(".framed .still img");
    await expect(still).toHaveAttribute("src", /./, { timeout: 30_000 });
    const src = (await still.getAttribute("src")) as string;
    await first.close();
    await context.route(src, (r) => r.fulfill({ body: WHITE_PNG, contentType: "image/png" }));
    for (const step of ["doors", "choice", "subs"]) {
      const page = await open(context, form, step);
      await expect(page.locator(".framed .still img")).toHaveClass(/\bin\b/, { timeout: 30_000 });
      const words = await drawnContrast(page, ".framed", ["h1", "> .lead", ".door b", ".door small", ".subrow b"], []);
      const head = await drawnContrast(page, ".welcome-head", [".skip"], []);
      expect(words.length).toBeGreaterThanOrEqual(2);
      for (const m of [...words, ...head]) expect(m.ratio, `${step} ${m.what}: ${m.ratio}`).toBeGreaterThanOrEqual(m.floor);
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
