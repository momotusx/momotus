/** Form factor and canvas fitting.
 *
 * TV draws a fixed 1920x1080 canvas scaled to the screen. Desktop reuses the
 * TV layout on a fluid canvas scaled down for arm's length. Phone is native
 * CSS pixels with its own type scale. `?form=tv|desktop|phone` overrides
 * detection and is remembered on this device. */

import { createSignal } from "solid-js";
import { browserZoom, desktopScale, isWebKit } from "./lib/zoom";
import { pageColor } from "./theme/page";

export type Form = "tv" | "desktop" | "phone";
const forms: readonly Form[] = ["tv", "desktop", "phone"];
const KEY = "mq.form";

function stored(): Form | null {
  try {
    const v = localStorage.getItem(KEY);
    return forms.includes(v as Form) ? (v as Form) : null;
  } catch {
    return null;
  }
}

function forced(): Form | null {
  const q = new URLSearchParams(location.search).get("form");
  if (q && forms.includes(q as Form)) {
    try {
      localStorage.setItem(KEY, q);
    } catch {
      // Private mode: the override lasts for this page load only.
    }
    return q as Form;
  }
  return stored();
}

const TV_UA = /SMART-TV|SmartTV|Tizen|Web0S|webOS|Android TV|AFT[A-Z]|BRAVIA|CrKey|HbbTV/i;

function detect(): Form {
  if (TV_UA.test(navigator.userAgent)) return "tv";
  return window.innerWidth <= 720 ? "phone" : "desktop";
}

let pin = forced();
const [form, setForm] = createSignal<Form>(pin ?? detect());
// The form chosen for this device, or null for Automatic (detected).
const [pinned, setPinned] = createSignal<Form | null>(pin);
export { form, pinned };
// The canvas scale (`--k`): desktop shelves are sized in CSS pixels, so their canvas size is px / k.
const [scale, setScale] = createSignal(1);
export { scale };
// The window's size in CSS pixels, as `fit` last saw it.
const [viewport, setViewport] = createSignal({ w: window.innerWidth, h: window.innerHeight });
export { viewport };

/** Lets Settings switch the form factor for this device. */
export function chooseForm(next: Form | null) {
  try {
    if (next) localStorage.setItem(KEY, next);
    else localStorage.removeItem(KEY);
  } catch {
    // Not persisted; still applies now.
  }
  pin = next;
  setPinned(next);
  fit();
}

export function fit() {
  const root = document.documentElement;
  const current = pin ?? detect();
  if (current !== form()) setForm(current);
  root.dataset.form = current;
  // The browser bar takes this form's page color (the phone's page is black).
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", pageColor(current));
  const w = window.innerWidth;
  const h = window.innerHeight;
  setViewport({ w, h });
  let k = 1;
  let cw = w;
  let ch = h;
  if (current === "tv") {
    k = Math.min(w / 1920, h / 1080);
    cw = 1920;
    ch = 1080;
  } else if (current === "desktop") {
    k = desktopScale(w, browserZoom(window.outerWidth, w, window.devicePixelRatio, isWebKit(navigator.userAgent)));
    cw = w / k;
    ch = h / k;
  }
  root.style.setProperty("--k", String(k));
  setScale(k);
  root.style.setProperty("--cw", `${cw}px`);
  root.style.setProperty("--ch", `${ch}px`);
  root.style.setProperty("--ox", `${(w - cw * k) / 2}px`);
  root.style.setProperty("--oy", `${(h - ch * k) / 2}px`);
}

export function startFitting() {
  fit();
  window.addEventListener("resize", fit, { passive: true });
}
