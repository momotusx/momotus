/** Desktop sign-in, variant 4 (the owner's pick of 2026-11-04, design/exploration/desktop-signin): every sign-in step
 * on desktop is a stage, the first run's still full bleed, the step at the lower left with its ways in one row
 * (Google and Facebook in their own marks and colors, the other providers, email, "I already use Momotus on another
 * device"), Other ways and Not now under it, and the phone's live QR at the lower right. Escape is Not now. The
 * phone keeps its column. A relay of its own lists Google, Facebook, email and the mock OpenID provider; the mock
 * signs in as alice for the `new` step. */

import { type Browser, expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client, PORTS, SHOTS, SIGN_IN_PORTS as P, startBridge, startMockProvider, startRelay } from "./core";

const SERVER = `http://127.0.0.1:${P.relay}`;
const procs: { proc: ChildProcess; data?: string }[] = [];
let scratch = "";
/** The next `op` the page asks is answered with no code, as when the sync service does not answer. */
let failNext = "";

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
};

async function device(browser: Browser, form: keyof typeof FORMS): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.addInitScript((server) => localStorage.setItem("mq.syncServer", server), SERVER);
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${P.core}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => ws.send(String(e.data)));
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      const request = JSON.parse(String(m)) as { id?: number; op?: string };
      if (failNext && request.op === failNext) {
        failNext = "";
        return ws.send(JSON.stringify({ id: request.id, ok: false, error: "the sync service did not answer" }));
      }
      if (core.readyState === WebSocket.OPEN) core.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  await page.goto(`/settings?form=${form}`);
  return page;
}

const go = (page: Page, path: string) =>
  page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);

const box = async (page: Page, selector: string) => (await page.locator(selector).first().boundingBox())!;

test.describe.serial("desktop sign-in stage", () => {
  test.beforeAll(async () => {
    // A first run builds the relay and the mock provider.
    test.setTimeout(600_000);
    scratch = mkdtempSync(join(tmpdir(), "mq-sign-in-e2e-"));
    const mock = await startMockProvider(P.mock);
    procs.push(mock);
    const identity = join(scratch, "identity.json");
    writeFileSync(
      identity,
      JSON.stringify({
        key: randomBytes(32).toString("hex"),
        publicUrl: SERVER,
        google: { webClientId: "web", webClientSecret: "ws", issuer: mock.url },
        facebook: { appId: "app", issuer: mock.url },
        oidc: [{ id: "mock", issuer: mock.url, clientId: "marquee", clientSecret: "ms" }],
        email: { accessKeyId: "AKIDMOCK", secretAccessKey: "mock/secret/key", region: "us-east-1", from: "noreply@example.org", endpoint: mock.url },
      }),
    );
    procs.push(await startRelay(P.relay, undefined, ["--identity", identity, "--public-host", "127.0.0.1"]));
    procs.push(await startBridge(P.core));
    // Something on Home, so the device is past the welcome steps.
    const c = new Client(`ws://127.0.0.1:${P.core}/ws`);
    await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
    c.close();
  });

  test.afterAll(async () => {
    for (const { proc, data } of procs) {
      proc.kill();
      if (data) rmSync(data, { recursive: true, force: true });
    }
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  test("desktop: the stage, the ways in one row, the brands' buttons, the live QR at the lower right; Tab, Escape", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await go(page, "/sign-in");
    const stage = page.locator(".sign-in-stage");
    await expect(stage.locator("h1")).toHaveText("Sign in");
    await expect(stage.locator(".lead")).toHaveText("This device's library will be added to your account.");
    await expect(stage.locator(".still")).toHaveCount(1);
    await expect(page.locator(".stage-row .btn")).toHaveText([
      "Continue with Google",
      "Continue with Facebook",
      "Continue with mock",
      "Continue with email",
      "I already use Momotus on another device",
    ]);
    // Google's dark button and Facebook's blue one, each with its own mark; the others keep the app's own.
    const look = (f: string) =>
      page.locator(`[data-f="${f}"]`).evaluate((el) => {
        const s = getComputedStyle(el);
        const mark = el.querySelector("img.provider-mark") as HTMLImageElement | null;
        return { bg: s.backgroundColor, color: s.color, edge: s.boxShadow, mark: mark?.naturalWidth ?? 0 };
      });
    const google = await look("signin:google");
    expect(google).toMatchObject({ bg: "rgb(19, 19, 20)", color: "rgb(227, 227, 227)" });
    expect(google.edge).toContain("rgb(142, 145, 143)");
    expect(google.mark).toBeGreaterThan(0);
    const facebook = await look("signin:facebook");
    expect(facebook).toMatchObject({ bg: "rgb(24, 119, 242)", color: "rgb(255, 255, 255)" });
    expect(facebook.mark).toBeGreaterThan(0);
    expect((await look("signin:mock")).mark).toBe(0);

    // The step at the lower left: the heading on the page's 80 px inset, the providers on one line.
    const h1 = await box(page, ".sign-in-stage h1");
    expect(Math.round(h1.x)).toBe(80);
    const [g, f, m] = [await box(page, '[data-f="signin:google"]'), await box(page, '[data-f="signin:facebook"]'), await box(page, '[data-f="signin:mock"]')];
    expect(Math.abs(g.y - f.y)).toBeLessThanOrEqual(1);
    expect(Math.abs(g.y - m.y)).toBeLessThanOrEqual(1);
    expect(f.x).toBeGreaterThan(g.x + g.width);

    // The corner: "Sign in with your phone", how, and the live code, 56 from the right and 84 from the foot.
    const corner = page.locator("#sign-in-qr");
    await expect(corner.locator("h2")).toHaveText("Sign in with your phone");
    await expect(corner.locator(".qrbox svg")).toBeVisible({ timeout: 30_000 });
    await expect(corner.locator(".pair-steps .step")).toHaveText("On your phone, open Momotus and tap Sign in another device");
    const c = await box(page, "#sign-in-qr");
    expect(Math.abs(c.x + c.width - (1440 - 56))).toBeLessThanOrEqual(2);
    expect(Math.abs(c.y + c.height - (900 - 84))).toBeLessThanOrEqual(2);
    for (const f of ["signin:google", "signin:email", "signin:already", "signin:not-now"]) {
      const b = await box(page, `[data-f="${f}"]`);
      expect(b.x + b.width, `${f} runs into the corner`).toBeLessThan(c.x);
    }
    await page.screenshot({ path: `${SHOTS}/sign-in-stage-desktop.png` });

    // Tab walks the ways in order, then the links, then the corner's Copy code, each with a visible focus.
    await page.locator('[data-f="signin:google"]').focus();
    const walked: string[] = [];
    for (let i = 0; i < 7; i++) {
      await page.keyboard.press("Tab");
      walked.push(
        await page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          return `${el.dataset.f}:${getComputedStyle(el).outlineStyle !== "none"}`;
        }),
      );
    }
    expect(walked).toEqual([
      "signin:facebook:true",
      "signin:mock:true",
      "signin:email:true",
      "signin:already:true",
      "signin:other:true",
      "signin:not-now:true",
      "pair:copy:true",
    ]);

    // "I already use Momotus on another device" is the corner's way in: the focus goes to it.
    await page.locator('[data-f="signin:already"]').click();
    await expect(corner).toBeFocused();

    // A window closed without a return: the line and its buttons in the step, the corner stays.
    const [closed] = await Promise.all([page.waitForEvent("popup"), page.locator('[data-f="signin:mock"]').click()]);
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await page.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(stage.locator(".sign-in-failure .st")).toHaveText("Didn't finish signing in?");
    await expect(corner.locator(".qrbox svg")).toBeVisible();

    // Escape is Not now: Account's sync page.
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(/\/sync$/);
    await page.context().close();
  });

  test("desktop: no sync service in reach says so in the step with Try again and Not now", async ({ browser }) => {
    const page = await device(browser, "desktop");
    failNext = "relayInfo";
    await go(page, "/sign-in");
    await expect(page.locator(".sign-in-stage .sign-in-failure .st")).toHaveText("Momotus can't reach its sync service right now.");
    await expect(page.locator(".sign-in-stage .sign-in-failure .btn")).toHaveText(["Try again", "Not now"]);
    await page.locator('[data-f="signin:relay:retry"]').click();
    await expect(page.locator('[data-f="signin:google"]')).toBeVisible();
    await page.context().close();
  });

  test("desktop: a failed create after a provider leaves the same stage, Create and the corner's way in, Escape back to the first step", async ({ browser }) => {
    const page = await device(browser, "desktop");
    await go(page, "/sign-in");
    // The account is created at once after a provider sign-in; only a failed create shows this stage.
    failNext = "createSyncAccount";
    const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator('[data-f="signin:mock"]').click()]);
    await popup.getByText("Sign in as alice").click();
    await expect(page).toHaveURL(/\/sign-in\/new$/);
    const stage = page.locator(".sign-in-stage");
    await expect(stage.locator("h1")).toHaveText("Create your Momotus account");
    await expect(stage.locator(".lead")).toHaveText("This sign-in has no Momotus account yet. This device's library will be added to your account.");
    await expect(page.locator(".sign-in-stage > .acts .btn")).toHaveText(["Create your Momotus account", "I already use Momotus on another device"]);
    await expect(page.locator(".sign-in-failure")).toBeVisible();
    await expect(page.locator("#sign-in-qr .qrbox svg")).toBeVisible({ timeout: 30_000 });
    const c = await box(page, "#sign-in-qr");
    expect(Math.abs(c.x + c.width - (1440 - 56))).toBeLessThanOrEqual(2);
    await page.screenshot({ path: `${SHOTS}/sign-in-stage-new-desktop.png` });
    await page.locator('[data-f="signin:already"]').click();
    await expect(page.locator("#sign-in-qr")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(/\/sign-in$/);
    await page.context().close();
  });

  test("phone: the column as before, no stage, no corner, the app's own buttons", async ({ browser }) => {
    const page = await device(browser, "phone");
    await go(page, "/sign-in");
    await expect(page.locator(".step-body h1")).toHaveText("Sign in");
    await expect(page.locator('[data-f="signin:google"]')).toBeVisible();
    await expect(page.locator(".sign-in-stage")).toHaveCount(0);
    await expect(page.locator("#sign-in-qr")).toHaveCount(0);
    await expect(page.locator('[data-f="signin:already"]')).toHaveCount(0);
    await expect(page.locator("[data-brand]")).toHaveCount(0);
    await page.context().close();
  });
});
