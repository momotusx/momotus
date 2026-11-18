/** Small local fallbacks for APIs newer than Chromium 79 (webOS 6), installed once at startup and only where the
 * engine lacks them. No polyfill bundle: each one covers exactly what the app calls. Import this module first in
 * main.tsx, before anything that reads these APIs at load time (lib/words.ts and lib/avatar.ts read Intl). */

type Bag = Record<string, unknown>;

/** Defines `name` on `target` when it is not a function there; a method added this way stays out of for-in loops. */
function install(target: unknown, name: string, value: unknown) {
  const t = target as Bag | undefined;
  if (!t || typeof t[name] === "function") return;
  Object.defineProperty(t, name, { value, configurable: true, writable: true });
}

// The names the app shows for the languages its picker lists first; any other code stays as given.
const LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  tr: "Turkish",
  es: "Spanish",
  fr: "French",
  de: "German",
  it: "Italian",
  pt: "Portuguese",
  ja: "Japanese",
};

class DisplayNamesFallback {
  of(code: string): string | undefined {
    // Unknown: none, so the caller shows its own form of the code (Intl.DisplayNames would name it).
    return LANGUAGE_NAMES[code.toLowerCase()];
  }
}

/** Code points, not grapheme clusters: a joined emoji or a letter with a combining mark splits. */
class SegmenterFallback {
  segment(input: string) {
    let index = 0;
    const parts = [...input].map((segment) => {
      const part = { segment, index, input };
      index += segment.length;
      return part;
    });
    return { [Symbol.iterator]: () => parts[Symbol.iterator]() };
  }
}

function replaceAllFallback(this: string, search: string | RegExp, replacement: string | ((...a: string[]) => string)): string {
  const text = String(this);
  if (search instanceof RegExp) {
    if (!search.global) throw new TypeError("replaceAll must be called with a global RegExp");
    return text.replace(search, replacement as string);
  }
  return text.replace(new RegExp(String(search).replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "g"), replacement as string);
}

function atFallback(this: ArrayLike<unknown>, index: number) {
  const i = Math.trunc(index) || 0;
  return this[i < 0 ? this.length + i : i];
}

function randomUUIDFallback(crypto: Crypto): string {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6]! & 0x0f) | 0x40;
  b[8] = (b[8]! & 0x3f) | 0x80;
  const hex = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function replaceChildrenFallback(this: Node, ...nodes: (Node | string)[]) {
  while (this.firstChild) this.removeChild(this.firstChild);
  (this as unknown as { append(...n: (Node | string)[]): void }).append(...nodes);
}

/** `scrollend` (Chrome 114): a `scroll` quiet for this long ends it. */
const SCROLL_QUIET_MS = 100;

function scrollEndFallback(doc: Document) {
  const timers = new WeakMap<object, ReturnType<typeof setTimeout>>();
  doc.addEventListener(
    "scroll",
    (e) => {
      const target = e.target as EventTarget;
      clearTimeout(timers.get(target));
      timers.set(
        target,
        setTimeout(() => target.dispatchEvent(new Event("scrollend", { bubbles: target === doc })), SCROLL_QUIET_MS),
      );
    },
    { capture: true, passive: true },
  );
}

const TABBABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]';

/** `inert` (Chrome 102) where the engine only has the attribute's name (LG webOS 6 and 22): the property sets the
 * attribute, which the D-pad engine already skips (focus/dom.ts), and hides the element from screen readers; one rule
 * takes the pointer off every `[inert]` region and one focus guard keeps focus out of them. So every `inert` in the
 * app (Solid writes the JSX one as the property, language-picker.tsx the attribute) works there as it does natively. */
function inertFallback(g: typeof globalThis) {
  const proto = g.HTMLElement?.prototype;
  const doc = g.document;
  if (!proto || !doc || "inert" in proto) return;
  const HIDDEN = "data-mq-inert-hid";
  Object.defineProperty(proto, "inert", {
    configurable: true,
    get(this: HTMLElement) {
      return this.hasAttribute("inert");
    },
    set(this: HTMLElement, on: unknown) {
      if (on) {
        this.setAttribute("inert", "");
        if (!this.hasAttribute("aria-hidden")) this.setAttribute(HIDDEN, "");
        this.setAttribute("aria-hidden", "true");
      } else {
        this.removeAttribute("inert");
        if (this.hasAttribute(HIDDEN)) this.removeAttribute("aria-hidden");
        this.removeAttribute(HIDDEN);
      }
    },
  });
  const style = doc.createElement("style");
  style.textContent = "[inert],[inert] *{pointer-events:none!important;user-select:none!important}";
  doc.head.appendChild(style);
  // Tab steps over an inert region to the next live control that way, as native inert does; any other focus that lands
  // in one (a script's focus(), a click) goes back where it was.
  let last: HTMLElement | null = null;
  let tabbing = 0;
  doc.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Tab") return;
      tabbing = e.shiftKey ? -1 : 1;
      setTimeout(() => (tabbing = 0));
    },
    true,
  );
  doc.addEventListener(
    "focusin",
    (e) => {
      const to = e.target as HTMLElement;
      if (!to.closest?.("[inert]")) return void (last = to);
      if (tabbing) {
        const live = [...doc.querySelectorAll<HTMLElement>(TABBABLE)].filter((el) => el.tabIndex >= 0 && !el.closest("[inert]"));
        const way = tabbing > 0 ? 4 : 2; // Node.DOCUMENT_POSITION_FOLLOWING, Node.DOCUMENT_POSITION_PRECEDING
        const next = (tabbing > 0 ? live : live.reverse()).find((el) => to.compareDocumentPosition(el) & way);
        if (next) return void next.focus();
      }
      if (last?.isConnected && !last.closest("[inert]")) last.focus();
      else to.blur();
    },
    true,
  );
}

/** Installs every missing fallback on `g`; the tests pass a stand-in global where the engine is not a browser. */
export function installCompat(g: typeof globalThis = globalThis) {
  install(g.String?.prototype, "replaceAll", replaceAllFallback);
  install(g.Array?.prototype, "at", atFallback);
  install(g.Object, "hasOwn", (o: object, key: PropertyKey) => Object.prototype.hasOwnProperty.call(o, key));
  install(g.AbortSignal, "timeout", (ms: number) => {
    const controller = new AbortController();
    setTimeout(() => controller.abort(), ms);
    return controller.signal;
  });
  if (g.crypto) install(g.crypto, "randomUUID", () => randomUUIDFallback(g.crypto));
  install(g.Element?.prototype, "replaceChildren", replaceChildrenFallback);
  install(g.Intl, "DisplayNames", DisplayNamesFallback);
  install(g.Intl, "Segmenter", SegmenterFallback);
  if (g.document && !("onscrollend" in (g as object))) scrollEndFallback(g.document);
  inertFallback(g);
}

installCompat();
