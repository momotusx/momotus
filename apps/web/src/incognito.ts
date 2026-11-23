/** Incognito's web glue (plans/batch-10-incognito.md 2.7 and 5.5): the click that puts a link's route in place, the
 * neutral tab title, and the desktop's idle sender. */

import { createEffect, createRoot, on } from "solid-js";
import { op } from "./core/client";
import { endsSoon, incognito, setEndsSoon } from "./core/profiles";
import { placingFocus } from "./focus/dom";
import { form } from "./form";

// The one delegated handler for Incognito's links (components/route-link.tsx): for a plain click or Enter, the anchor
// carries its route, so the router and the anchor's own handlers see it as they do outside Incognito. Its "/" comes back
// once the click is handled. Registered in the capture phase, ahead of every other click listener.
window.addEventListener(
  "click",
  (e) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>("a[data-route]") : null;
    const route = a?.getAttribute("data-route");
    if (!a || !route) return;
    a.setAttribute("href", route);
    setTimeout(() => a.hasAttribute("data-route") && a.setAttribute("href", "/"));
  },
  { capture: true },
);

/** The tab's title. In Incognito it never names the page or the title (focus on each page's heading names the place). */
export function setTitle(title: string) {
  document.title = incognito() ? "Incognito · Momotus" : title;
}

const lifecycle = (event: string) => void op("lifecycle", { event }).catch(() => undefined);

// The desktop's idle sender (2.7): `idle` after a minute with no input and no video playing, `active` on the next
// input. Focus counts too, since screen readers move it. The core ends Incognito 30 minutes into an absence.
const IDLE_MS = 60_000;
let timer: ReturnType<typeof setTimeout> | undefined;
let idle = false;
const watching = () => incognito() && form() === "desktop";
const playing = () => [...document.querySelectorAll("video")].some((v) => !v.paused && !v.ended);
function arm() {
  clearTimeout(timer);
  timer = setTimeout(() => {
    if (playing()) return arm();
    idle = true;
    lifecycle("idle");
  }, IDLE_MS);
}

/** Back from an absence: the core hears it, and the warning goes. */
export function stayIncognito() {
  setEndsSoon(false);
  if (idle) {
    idle = false;
    lifecycle("active");
  }
  arm();
}

const notice = () => document.querySelector<HTMLElement>(".incog-stay");
// A pointer resting still sends no input: a move must change its place (a page moving under it can send moves). Its place
// is kept from every move, Incognito or not: the browser repeats the last place when the page changes under a resting
// pointer (the warning showing, a scroll), and a place last kept before Incognito began made that repeat a return, which
// took the warning away under the viewer (CI's idle test, its Tab on from the button).
let at = "";
const input = (e: Event) => {
  const here = e instanceof PointerEvent && e.type === "pointermove" ? `${e.clientX},${e.clientY}` : null;
  const still = here !== null && here === at;
  if (here !== null) at = here;
  if (!watching() || still) return;
  // The warning's own button acts by its click; reaching it by Tab, or Tab on from it, is not yet a return.
  if (e.target instanceof Node && notice()?.contains(e.target)) return;
  if (e instanceof FocusEvent && e.relatedTarget && e.relatedTarget === notice()) return;
  // The app's own focus moves are no return: a focus that fell to the page (Tab on past the warning's button) goes back
  // to the screen's target on the page's next change, and that took the warning away under the viewer (CI's idle test).
  if (e.type === "focusin" && placingFocus()) return;
  stayIncognito();
};
for (const type of ["pointermove", "pointerdown", "touchstart", "wheel", "focusin"]) window.addEventListener(type, input, { capture: true, passive: true });
window.addEventListener(
  "keydown",
  (e) => {
    if (!watching()) return;
    // While the warning shows, Tab goes to its button first (it never takes the focus by itself), as the next stop
    // after the current focus.
    const stay = notice();
    // On the button: its keys are the button's (Enter and Space press it, Tab moves on and the warning stays).
    if (stay && document.activeElement === stay) return;
    if (endsSoon() && stay && e.key === "Tab" && !e.shiftKey) {
      // Once the button is used and goes, the focus system puts the focus back where the viewer had it.
      e.preventDefault();
      stay.focus();
      return;
    }
    stayIncognito();
  },
  { capture: true },
);

createRoot(() =>
  createEffect(
    on(watching, (on) => {
      clearTimeout(timer);
      idle = false;
      if (on) arm();
    }),
  ),
);
