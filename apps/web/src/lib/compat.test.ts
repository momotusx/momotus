import { expect, test } from "bun:test";
import { installCompat } from "./compat";

/** Runs `fn` with `target[name]` missing, then puts the native one back. */
async function without(target: object, name: string, fn: () => void | Promise<void>) {
  const own = Object.getOwnPropertyDescriptor(target, name);
  Object.defineProperty(target, name, { value: undefined, configurable: true, writable: true });
  try {
    await fn();
  } finally {
    if (own) Object.defineProperty(target, name, own);
    else delete (target as Record<string, unknown>)[name];
  }
}

test("replaceAll replaces every match, text or global pattern", () =>
  without(String.prototype, "replaceAll", () => {
    installCompat();
    expect("a.b.c".replaceAll(".", "-")).toBe("a-b-c");
    expect("{n} of {n}".replaceAll("{n}", "$&!")).toBe("{n}! of {n}!");
    expect("aXbX".replaceAll(/x/gi, (m) => m.toLowerCase())).toBe("axbx");
    expect(() => "a".replaceAll(/a/, "b")).toThrow(TypeError);
  }));

test("Array.at counts back from the end", () =>
  without(Array.prototype, "at", () => {
    installCompat();
    expect([1, 2, 3].at(-1)).toBe(3);
    expect([1, 2, 3].at(0)).toBe(1);
    expect([1, 2, 3].at(5)).toBeUndefined();
    expect([].at(-1)).toBeUndefined();
  }));

test("Object.hasOwn ignores the prototype chain", () =>
  without(Object, "hasOwn", () => {
    installCompat();
    expect(Object.hasOwn({ a: 1 }, "a")).toBe(true);
    expect(Object.hasOwn({ a: 1 }, "toString")).toBe(false);
  }));

test("AbortSignal.timeout aborts after the time", () =>
  without(AbortSignal, "timeout", async () => {
    installCompat();
    const signal = AbortSignal.timeout(5);
    expect(signal.aborted).toBe(false);
    await new Promise((resolve) => signal.addEventListener("abort", resolve));
    expect(signal.aborted).toBe(true);
  }));

test("crypto.randomUUID makes version 4 ids that differ", () =>
  without(crypto, "randomUUID", () => {
    installCompat();
    const a = crypto.randomUUID();
    expect(a).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(crypto.randomUUID()).not.toBe(a);
  }));

test("Intl.DisplayNames names the common languages and no others", () =>
  without(Intl, "DisplayNames", () => {
    installCompat();
    const names = new Intl.DisplayNames(["en"], { type: "language" });
    expect(names.of("tr")).toBe("Turkish");
    expect(names.of("FR")).toBe("French");
    // An unknown code has no name, so languageName shows the code itself (as without the fallback).
    expect(names.of("zz")).toBeUndefined();
  }));

test("Intl.Segmenter splits by code point", () =>
  without(Intl, "Segmenter", () => {
    installCompat();
    const parts = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment("a\u{1F600}b")];
    expect(parts.map((p) => p.segment)).toEqual(["a", "\u{1F600}", "b"]);
    expect(parts.map((p) => p.index)).toEqual([0, 1, 3]);
    expect(new Intl.Segmenter().segment("")[Symbol.iterator]().next().done).toBe(true);
  }));

test("replaceChildren empties an element and appends the new nodes", () => {
  class FakeElement {
    kids: unknown[] = [];
    get firstChild() {
      return this.kids[0] ?? null;
    }
    removeChild(k: unknown) {
      this.kids.splice(this.kids.indexOf(k), 1);
    }
    append(...n: unknown[]) {
      this.kids.push(...n);
    }
  }
  installCompat({ Element: FakeElement } as unknown as typeof globalThis);
  const el = new FakeElement();
  el.kids = ["old", "older"];
  (el as unknown as { replaceChildren(...n: unknown[]): void }).replaceChildren("new");
  expect(el.kids).toEqual(["new"]);
  (el as unknown as { replaceChildren(): void }).replaceChildren();
  expect(el.kids).toEqual([]);
});

test("scrollend fires once on each target after its scrolling goes quiet", async () => {
  const doc = new EventTarget();
  const inner = new EventTarget();
  installCompat({ document: doc } as unknown as typeof globalThis);
  const ended: string[] = [];
  inner.addEventListener("scrollend", () => ended.push("inner"));
  doc.addEventListener("scrollend", () => ended.push("doc"));
  for (const target of [inner, inner, doc]) {
    const e = new Event("scroll");
    // A real page's capture listener on the document hears a scrolled element; here the target is set by hand.
    Object.defineProperty(e, "target", { value: target });
    doc.dispatchEvent(e);
  }
  expect(ended).toEqual([]);
  await new Promise((resolve) => setTimeout(resolve, 160));
  expect(ended.sort()).toEqual(["doc", "inner"]);
});

test("a modern engine keeps its own APIs", () => {
  const native = String.prototype.replaceAll;
  installCompat();
  expect(String.prototype.replaceAll).toBe(native);
});

test("scrollend is left alone where the engine has it", () => {
  const doc = new EventTarget();
  let added = 0;
  doc.addEventListener = () => void added++;
  installCompat({ document: doc, onscrollend: null } as unknown as typeof globalThis);
  expect(added).toBe(0);
});

test("inert below Chrome 102: the property sets the attribute, hides from readers, and keeps focus out", async () => {
  class El {
    attrs = new Map<string, string>();
    parent: El | null = null;
    isConnected = true;
    focused = 0;
    blurred = 0;
    hasAttribute = (n: string) => this.attrs.has(n);
    getAttribute = (n: string) => this.attrs.get(n) ?? null;
    setAttribute = (n: string, v: string) => void this.attrs.set(n, v);
    removeAttribute = (n: string) => void this.attrs.delete(n);
    closest(sel: string): El | null {
      if (sel !== "[inert]") throw new Error(sel);
      for (let e: El | null = this; e; e = e.parent) if (e.hasAttribute("inert")) return e;
      return null;
    }
    focus() {
      this.focused++;
    }
    order = 0;
    tabIndex = 0;
    compareDocumentPosition(other: El) {
      return other.order > this.order ? 4 : 2;
    }
    blur() {
      this.blurred++;
    }
  }
  const styles: { textContent: string }[] = [];
  const focusin: ((e: { target: El }) => void)[] = [];
  const keydown: ((e: { key: string; shiftKey: boolean }) => void)[] = [];
  const tabbables: El[] = [];
  const document = {
    head: { appendChild: (s: { textContent: string }) => styles.push(s) },
    createElement: () => ({ textContent: "" }),
    querySelectorAll: () => tabbables,
    addEventListener: (type: string, fn: never) => (type === "focusin" ? focusin.push(fn) : type === "keydown" ? keydown.push(fn) : 0),
  };
  installCompat({ HTMLElement: El, document } as unknown as typeof globalThis);
  const region = new El() as unknown as HTMLElement & El;
  const inside = new El();
  inside.parent = region;
  const outside = new El();

  region.inert = true;
  expect(region.inert).toBe(true);
  expect(region.getAttribute("inert")).toBe("");
  expect(region.getAttribute("aria-hidden")).toBe("true");
  expect(styles[0]?.textContent).toContain("[inert]");
  // Tab or a script's focus() into the region: focus goes back to where it was.
  focusin[0]!({ target: outside });
  focusin[0]!({ target: inside });
  expect(outside.focused).toBe(1);
  // Tab steps over the region to the next live control that way, Shift+Tab to the one before it.
  const before = new El();
  const after = new El();
  [before, region, inside, after].forEach((e, i) => (e.order = i));
  tabbables.push(before, inside, after);
  keydown[0]!({ key: "Tab", shiftKey: false });
  focusin[0]!({ target: inside });
  expect(after.focused).toBe(1);
  keydown[0]!({ key: "Tab", shiftKey: true });
  focusin[0]!({ target: inside });
  expect(before.focused).toBe(1);
  await new Promise((r) => setTimeout(r));
  region.inert = false;
  expect(region.inert).toBe(false);
  expect(region.hasAttribute("aria-hidden")).toBe(false);
  // An element hidden from readers on its own stays hidden.
  const own = new El() as unknown as HTMLElement & El;
  own.setAttribute("aria-hidden", "true");
  own.inert = true;
  own.inert = false;
  expect(own.getAttribute("aria-hidden")).toBe("true");
});
