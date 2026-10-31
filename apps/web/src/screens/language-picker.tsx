/** The language picker, "Search first" (plans/onboarding-language-picker.md, owner pick 4): the search on
 * top, the suggested languages while nothing is typed, then every language the core knows. The core
 * answers every query (`languagePicker`): its order, its suggestions, where No subtitles goes. Used by the
 * onboarding subtitles step (one pick) and by Settings' subtitle and audio languages (ranked picks).
 *
 * Forms: the phone lists in one column under chips; desktop shows an A to Z directory in four columns; TV
 * draws its own one-line keyboard (no system keyboard), large round flags and a four-column grid. The
 * field and the keyboard stay pinned while the results scroll. */

import AudioLines from "lucide-solid/icons/audio-lines";
import CaptionsOff from "lucide-solid/icons/captions-off";
import Check from "lucide-solid/icons/check";
import Delete from "lucide-solid/icons/delete";
import SearchIcon from "lucide-solid/icons/search";
import X from "lucide-solid/icons/x";
import { batch, createEffect, createMemo, createSignal, For, type JSX, on, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { deviceLanguage, op, reasonOf } from "../core/client";
import type { LanguageChoice } from "../core/generated/LanguageChoice";
import type { LanguageList } from "../core/generated/LanguageList";
import { pushBack } from "../focus/back";
import { autoFocus, focusEl, inputCount, remember } from "../focus/dom";
import { form } from "../form";
import { found, KEYS, type Key, press } from "../lib/language-picker";
import { languageName } from "../lib/words";
import { Flag } from "../player/flags";
import "./language-picker.css";

/** The common languages, the fallback when the core's list does not come. */
const QUICK = ["en", "tr", "es", "fr", "de", "it", "pt", "ja"];
/** Scripts TV browsers often lack (Santali, Syriac, Tifinagh): their own name is left out on TV. */
const RARE_ON_TV = new Set(["sat", "syr", "zgh"]);

/** One language, No subtitles (`code` null), or the spoken language (`SPOKEN`). `native` is empty when the own name is
 * not known. */
export type Pickable = { code: string | null; name: string; native: string };

/** The Second line's "Spoken language" row: the word `Prefs.secondSubtitle` stores for it. */
export const SPOKEN = "audio";
const none = (name = "No subtitles"): Pickable => ({ code: null, name, native: "" });
const pickable = (l: LanguageChoice): Pickable => ({ code: l.code, name: l.name, native: l.native });
/** The common languages, named in English, for when the core does not answer. */
const fallback = (): Pickable[] => QUICK.map((code) => ({ code, name: languageName(code), native: "" }));
const keyOf = (code: string | null) => code ?? "none";

/** No subtitles' round mark, where a flag would be. */
export const NoneFlag = () => (
  <span class="flag none lp-off" aria-hidden="true">
    <CaptionsOff />
  </span>
);

/** The spoken language's round mark, where a flag would be. */
export const SpokenFlag = () => (
  <span class="flag none lp-off" aria-hidden="true">
    <AudioLines />
  </span>
);

/** What the core pre-chooses on the onboarding step: the first suggestion, or No subtitles. */
export const preChosen = (reply: LanguageList): Pickable | null => {
  const first = reply.suggested[0];
  return reply.noneFirst || !first ? null : pickable(first);
};

/** The first answer for the onboarding step: the pre-chosen language and how many there are. With no
 * answer, No subtitles. */
export async function firstChoice(): Promise<{ chosen: Pickable | null; total: number | null }> {
  try {
    const reply = await op<LanguageList>("languagePicker", { device: deviceLanguage() });
    return { chosen: preChosen(reply), total: reply.results.length };
  } catch {
    return { chosen: null, total: null };
  }
}

/** A text field that takes keys (the TV's query is read-only: the keys type into it). */
const editable = (t: EventTarget | null) =>
  (t instanceof HTMLInputElement && !t.readOnly) || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable);

export function LanguageSearch(props: {
  /** The data-f prefix, unique on the page. */
  group: string;
  /** Audio: one row per primary language, no No subtitles. */
  audio?: boolean;
  /** The profile's languages in rank order (Settings), for the core's suggestions and ties. */
  chosen: string[];
  /** Ranked picks (Settings) or one (onboarding). */
  multi?: boolean;
  isOn: (code: string | null) => boolean;
  /** Settings: a chosen language's rank, 1 first. */
  rank?: (code: string) => number;
  /** Settings: what a tap does, spoken with the row ("" for nothing). */
  action?: (code: string | null) => string;
  onPick: (code: string | null, picked: Pickable) => void;
  /** Each answer to the empty query (the onboarding step pre-chooses from it). */
  onList?: (reply: LanguageList) => void;
  /** Desktop and phone: the field takes focus when the picker opens. */
  focusField?: boolean;
  /** TV: the first suggestion (the pre-chosen one) takes focus once the list is in, unless the viewer moved. */
  focusFirst?: boolean;
  /** Under the field (Settings' hint). */
  hint?: string;
  /** Pinned under the results (the step's error line and Continue). */
  foot?: JSX.Element;
  /** The No subtitles row's name (Settings' Second line: "Off"). */
  none?: string;
  /** The Second line: a "Spoken language" row (`SPOKEN`) right after the first, which is then always None. */
  spoken?: string;
  /** Why a language cannot be picked here, shown on its row ("Subtitle language"); undefined for one that can. */
  blocked?: (code: string) => string | undefined;
}): JSX.Element {
  const tv = () => form() === "tv";
  const [query, setQuery] = createSignal("");
  // The last answer and the query it answers: the layout follows what is drawn, not the key just pressed.
  const [list, setList] = createSignal<LanguageList | null>(null);
  const [shown, setShown] = createSignal("");
  const [failed, setFailed] = createSignal<string | null>(null);
  const [count, setCount] = createSignal("");
  const [total, setTotal] = createSignal<number | null>(null);
  const [lastKey, setLastKey] = createSignal<Key>("a");
  let seq = 0;
  let disposed = false;
  let root: HTMLDivElement | undefined;
  let head: HTMLDivElement | undefined;
  let foot: HTMLDivElement | undefined;
  let field: HTMLInputElement | undefined;
  let firstFocus = !!props.focusFirst;
  // Where the focus goes once the next answer is drawn (typing or clearing under a focused row).
  let settleOnReply = false;
  const asked = inputCount();
  onCleanup(() => {
    disposed = true;
  });

  const ask = (q: string) => {
    const mine = ++seq;
    op<LanguageList>("languagePicker", { device: deviceLanguage(), query: q, chosen: props.chosen, audio: !!props.audio }).then(
      (reply) => {
        // An older query's answer that arrives last changes nothing.
        if (disposed || mine !== seq) return;
        batch(() => {
          setFailed(null);
          setList(reply);
          setShown(q);
          if (!q.trim()) setTotal(reply.results.length);
        });
        if (!q.trim()) props.onList?.(reply);
        answered();
      },
      (e: unknown) => {
        if (disposed || mine !== seq) return;
        batch(() => {
          setFailed(reasonOf(e));
          setShown(q);
        });
        answered();
      },
    );
  };
  /** After an answer is drawn: the TV's first focus (once, and never over a move the viewer made), and
   * the focus a changed list leaves without a target. */
  const answered = () => {
    if (firstFocus && tv()) {
      firstFocus = false;
      const first = root?.querySelector<HTMLElement>(`[data-fgroup="${props.group}-s"] [data-f]`);
      if (inputCount() === asked) autoFocus(first);
    }
    if (settleOnReply) {
      settleOnReply = false;
      settle();
    }
  };
  createEffect(on(query, (q) => ask(q)));

  const typed = () => shown().trim().length > 0;
  const loaded = () => list() !== null || failed() !== null;
  const suggested = createMemo<Pickable[]>(() => {
    if (typed() || !loaded()) return [];
    const reply = list();
    const langs = reply ? reply.suggested.map(pickable) : fallback();
    if (props.audio) return langs;
    if (props.spoken) return [none(props.none), { code: SPOKEN, name: props.spoken, native: "" }, ...langs];
    return (reply?.noneFirst ?? true) ? [none(props.none), ...langs] : [...langs, none(props.none)];
  });
  const results = createMemo<LanguageChoice[]>(() => (failed() ? [] : (list()?.results ?? [])));

  // One Tab stop per list (the last focused option, else the first); the arrows move inside.
  const [sugAt, setSugAt] = createSignal<string | null>(null);
  const [rowAt, setRowAt] = createSignal<string | null>(null);
  const sugStop = createMemo(() => {
    const keys = suggested().map((p) => keyOf(p.code));
    const at = sugAt();
    return at && keys.includes(at) ? at : keys[0];
  });
  const rowStop = createMemo(() => {
    const at = rowAt();
    return at && results().some((l) => l.code === at) ? at : results()[0]?.code;
  });
  const stopAt = (set: (k: string) => void) => (e: FocusEvent) => {
    const k = (e.target as HTMLElement).dataset.key;
    if (k) set(k);
  };

  // The count, spoken after the viewer stops typing.
  let countTimer: ReturnType<typeof setTimeout> | undefined;
  createEffect(
    on(results, (r) => {
      clearTimeout(countTimer);
      const text = typed() ? (r.length ? found(r.length) : `No language matches "${shown().trim()}".`) : "";
      countTimer = setTimeout(() => setCount(text), 600);
    }),
  );
  onCleanup(() => clearTimeout(countTimer));

  // Back clears the query first, then does what it does today.
  let dropBack: (() => void) | undefined;
  createEffect(
    on(
      () => query().length > 0,
      (has) => {
        if (has && !dropBack) dropBack = pushBack(() => clear("back"));
        if (!has && dropBack) {
          dropBack();
          dropBack = undefined;
        }
      },
    ),
  );
  onCleanup(() => dropBack?.());

  const kbGroup = () => `${props.group}-kb`;
  const kbKey = () => root?.querySelector<HTMLElement>(`[data-f="${props.group}:kb:${lastKey()}"]`);
  const inKeyboard = () => !!document.activeElement?.closest(`[data-fgroup="${kbGroup()}"]`);
  const inList = () => !!root?.contains(document.activeElement) && !inKeyboard() && document.activeElement !== field;
  /** The focus after the list changed under it: the first suggestion or result, else Clear, else the keyboard. */
  const settle = () => {
    const el =
      root?.querySelector<HTMLElement>(`[data-fgroup="${props.group}-s"] [data-f]`) ??
      root?.querySelector<HTMLElement>(`[data-fgroup="${props.group}-r"] [data-f]`) ??
      root?.querySelector<HTMLElement>(`[data-f="${props.group}:clear"]`) ??
      kbKey();
    if (el) focusEl(el);
  };
  /** A new query. With the focus in the list, it moves before the list changes: to the keyboard's
   * remembered key (TV) or the field, then to the new list's first item once it is drawn. */
  const edit = (next: string) => {
    if (next === query()) return;
    if (inList()) {
      const hold = tv() ? kbKey() : field;
      if (hold) focusEl(hold);
      settleOnReply = true;
    }
    setQuery(next);
  };
  /** Back with a query, or Clear: the query goes. Clear's own button leaves with the empty state, so the
   * keyboard (TV) or the field takes the focus first. */
  const clear = (from: "back" | "button") => {
    if (from === "button") {
      const kb = kbKey();
      if (tv() && kb) focusEl(kb);
      else field?.focus();
      setQuery("");
      return;
    }
    edit("");
  };
  const key = (k: Key) => edit(press(query(), k));

  // Keys from a hardware keyboard. TV: letters, digits and Space type and Backspace deletes wherever the
  // focus is (outside a real text field), and a Space never clicks. Desktop and the phone, inside the
  // picker: letters and digits typed outside the field go to it, Space and Enter keep their button
  // meaning, "/" focuses the field, and Up and Down walk the directory's columns in A to Z order.
  // Backspace in the picker is never Back (focus/dom.ts `isBackKey`).
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey || e.defaultPrevented) return;
      const t = e.target as HTMLElement | null;
      const printable = e.key.length === 1 && /[\p{L}\p{N} ]/u.test(e.key);
      const take = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (tv()) {
        if (editable(t)) return;
        if (e.key === "Backspace") {
          take();
          edit(press(query(), "delete"));
        } else if (e.key === " ") {
          take();
          edit(press(query(), "space"));
        } else if (printable) {
          take();
          edit(query() + e.key.toLowerCase());
        }
        return;
      }
      if (!(root?.contains(t) || t === document.body) || t === field) return;
      if ((e.key === "ArrowDown" || e.key === "ArrowUp") && t?.closest(".lp-dir")) {
        const rows = [...(root?.querySelectorAll<HTMLElement>(".lp-dir [data-f]") ?? [])];
        const next = rows[rows.indexOf(t) + (e.key === "ArrowDown" ? 1 : -1)];
        if (next) {
          take();
          focusEl(next);
        }
        return;
      }
      if (e.key === "/") {
        take();
        field?.focus();
      } else if ((printable && e.key !== " ") || e.key === "Backspace") {
        take();
        field?.focus();
        setQuery(e.key === "Backspace" ? query().slice(0, -1) : query() + e.key);
      }
    };
    // A button also activates on Space's keyup in some engines.
    const onKeyUp = (e: KeyboardEvent) => {
      if (tv() && e.key === " " && !editable(e.target)) e.preventDefault();
    };
    document.addEventListener("keydown", onKey, { capture: true });
    document.addEventListener("keyup", onKeyUp, { capture: true });
    onCleanup(() => {
      document.removeEventListener("keydown", onKey, { capture: true });
      document.removeEventListener("keyup", onKeyUp, { capture: true });
    });
    // The pinned head and foot: a focused row is revealed clear of them (its scroll margins).
    // Each with its offset from the scroller's edge. Older TV browsers have no ResizeObserver: once then.
    const fit = () => {
      if (!root || !head) return;
      root.style.setProperty("--lp-head", `${head.offsetHeight + (parseFloat(getComputedStyle(head).top) || 0)}px`);
      root.style.setProperty("--lp-foot", `${foot ? foot.offsetHeight + (parseFloat(getComputedStyle(foot).bottom) || 0) : 0}px`);
    };
    fit();
    if (typeof ResizeObserver !== "undefined") {
      const sizes = new ResizeObserver(fit);
      if (head) sizes.observe(head);
      if (foot) sizes.observe(foot);
      onCleanup(() => sizes.disconnect());
    }
    // The keyboard's first remembered key is "a", not the one nearest to where the focus comes from.
    remember(kbGroup(), `${props.group}:kb:a`);
    if (props.focusField && !tv()) queueMicrotask(() => field?.focus());
  });

  /** The own name where it is known and drawable, else null. */
  const own = (p: Pickable) => (p.native && p.native !== p.name && !(tv() && p.code && RARE_ON_TV.has(p.code)) ? p.native : null);
  // The spoken name is the content: the English name first (hidden), the own name in its language, then
  // the rank and what a tap does (hidden). No aria-label, so the own name keeps its `lang`.
  const said = (p: Pickable) => {
    const r = p.code ? props.rank?.(p.code) : 0;
    const words = [r ? (r === 1 ? "default" : `number ${r}`) : null, props.action?.(p.code) || null].filter(Boolean);
    return words.length ? `, ${words.join(", ")}` : "";
  };
  const mark = (p: Pickable) => (
    <Show when={props.isOn(p.code)}>
      <Show when={p.code ? props.rank?.(p.code) : 0} fallback={<Check aria-hidden class="lp-check" />}>
        {(n) => (
          <b class="lp-rank num" aria-hidden="true">
            {n() === 1 ? "Default" : n()}
          </b>
        )}
      </Show>
    </Show>
  );
  const flag = (p: Pickable, lazy: boolean) => (
    <Show when={p.code} fallback={<NoneFlag />}>
      {(c) => (c() === SPOKEN && props.spoken ? <SpokenFlag /> : <Flag language={c()} lazy={lazy} />)}
    </Show>
  );
  const why = (p: Pickable) => (p.code && p.code !== SPOKEN ? props.blocked?.(p.code) : undefined);
  const pick = (p: Pickable) => why(p) === undefined && props.onPick(p.code, p);
  const name = (p: Pickable) => (
    <>
      <Show when={own(p)}>
        <span class="sr">{p.name}, </span>
      </Show>
      <span class="lp-name" lang={own(p) && p.code ? p.code : "en"} dir="auto">
        <bdi>{own(p) ?? p.name}</bdi>
      </span>
    </>
  );

  return (
    <div class="lp" ref={root} data-lp={props.group}>
      <div class="lp-head" ref={head}>
        <label class="lp-field" classList={{ tv: tv() }}>
          <SearchIcon aria-hidden />
          <span class="sr">Search languages</span>
          <input
            ref={field}
            type="search"
            inputmode={tv() ? "none" : undefined}
            readOnly={tv()}
            tabindex={tv() ? -1 : undefined}
            placeholder={total() ? `Search ${total()} languages` : "Search languages"}
            enterkeyhint="search"
            autocomplete="off"
            autocorrect="off"
            autocapitalize="off"
            spellcheck={false}
            data-f={tv() ? undefined : `${props.group}:field`}
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              // The phone's Return puts the keyboard away; the list stays.
              if (e.key === "Enter" && form() === "phone") e.currentTarget.blur();
            }}
          />
        </label>
        <Show when={tv()}>
          {/* The query as it is typed on the drawn keyboard (a field's value is not a live region's text). */}
          <span class="sr" aria-live="polite">
            {query()}
          </span>
          <div
            class="lp-kb"
            data-fgroup={kbGroup()}
            role="group"
            aria-label="Keyboard"
            onFocusIn={(e) => {
              const k = (e.target as HTMLElement).dataset.key as Key | undefined;
              if (k) setLastKey(k);
            }}
          >
            <For each={KEYS}>
              {(k) => (
                <button
                  class="lp-key"
                  classList={{ wide: k === "space" || k === "delete" }}
                  data-f={`${props.group}:kb:${k}`}
                  data-key={k}
                  aria-label={k === "space" ? "Space" : k === "delete" ? "Delete" : k}
                  onClick={() => key(k)}
                >
                  <Show when={k === "delete"} fallback={k === "space" ? "space" : k}>
                    <Delete aria-hidden />
                  </Show>
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>
      <Show when={props.hint}>
        <p class="lp-hint">{props.hint}</p>
      </Show>
      <Show when={!loaded()}>
        <p class="lp-note" role="status">
          Loading languages
        </p>
      </Show>
      <Show when={failed()}>
        {(why) => (
          <p class="lp-note" role="alert">
            The language list could not be loaded: {why()}{" "}
            <button class="lp-retry" data-f={`${props.group}:retry`} onClick={() => ask(query())}>
              Try again
            </button>
          </p>
        )}
      </Show>
      <Show when={suggested().length > 0}>
        <div
          class="lp-sug"
          classList={{ tv: tv() }}
          role="listbox"
          aria-label="Suggested"
          aria-multiselectable={props.multi || undefined}
          data-fgroup={`${props.group}-s`}
          data-fentry
          onFocusIn={stopAt(setSugAt)}
        >
          <For each={suggested()}>
            {(p) => (
              <button
                class="lp-chip"
                classList={{ on: props.isOn(p.code) }}
                role="option"
                aria-selected={props.isOn(p.code)}
                aria-disabled={why(p) !== undefined || undefined}
                tabindex={sugStop() === keyOf(p.code) ? 0 : -1}
                data-f={`${props.group}:s:${keyOf(p.code)}`}
                data-key={keyOf(p.code)}
                onClick={() => pick(p)}
              >
                {flag(p, false)}
                <Show when={why(p)} fallback={name(p)}>
                  {(w) => (
                    <span class="lp-stack">
                      {name(p)}
                      <small class="lp-why">{w()}</small>
                    </span>
                  )}
                </Show>
                {mark(p)}
                <span class="sr">{said(p)}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
      <Show when={loaded() && !failed()}>
        <p class="lp-count" aria-live="polite">
          <span aria-hidden="true">{typed() ? (results().length ? found(results().length) : "") : "All languages"}</span>
          <span class="sr">{count()}</span>
        </p>
        <Show
          when={results().length > 0}
          fallback={
            <Show when={typed()}>
              <p class="lp-empty">
                No language matches "{shown().trim()}".{" "}
                <button class="lp-clear" data-f={`${props.group}:clear`} onClick={() => clear("button")}>
                  <X aria-hidden />
                  Clear
                </button>
              </p>
            </Show>
          }
        >
          <Results
            group={props.group}
            directory={form() === "desktop" && !typed()}
            results={results()}
            multi={props.multi}
            onFocusIn={stopAt(setRowAt)}
            row={(l) => {
              const p = pickable(l);
              return (
                <button
                  class="lp-row"
                  classList={{ on: props.isOn(p.code) }}
                  role="option"
                  aria-selected={props.isOn(p.code)}
                  aria-disabled={why(p) !== undefined || undefined}
                  tabindex={rowStop() === l.code ? 0 : -1}
                  data-f={`${props.group}:r:${l.code}`}
                  data-key={l.code}
                  onClick={() => pick(p)}
                >
                  {flag(p, true)}
                  <span class="lp-names">
                    {name(p)}
                    {/* Two lines always: a row's box never follows its words. A row that cannot be picked says why. */}
                    <Show
                      when={why(p)}
                      fallback={
                        <small class="lp-en" aria-hidden="true">
                          {own(p) ? p.name : ""}
                        </small>
                      }
                    >
                      {(w) => <small class="lp-en lp-why">{w()}</small>}
                    </Show>
                  </span>
                  {mark(p)}
                  <span class="sr">{said(p)}</span>
                </button>
              );
            }}
          />
        </Show>
      </Show>
      <Show when={props.foot}>
        <div class="lp-foot" ref={foot}>
          {props.foot}
        </div>
      </Show>
    </div>
  );
}

/** The results: a list, or on desktop with nothing typed the A to Z directory with its letters. */
function Results(props: {
  group: string;
  directory: boolean;
  results: LanguageChoice[];
  multi?: boolean;
  onFocusIn: (e: FocusEvent) => void;
  row: (l: LanguageChoice) => JSX.Element;
}) {
  const letters = createMemo(() => {
    const out: { letter: string; langs: LanguageChoice[] }[] = [];
    for (const l of props.results) {
      const letter = (l.sort || l.name).charAt(0).toUpperCase();
      const last = out[out.length - 1];
      if (last?.letter === letter) last.langs.push(l);
      else out.push({ letter, langs: [l] });
    }
    return out;
  });
  return (
    <div
      class={props.directory ? "lp-dir" : "lp-list"}
      role="listbox"
      aria-label="Languages"
      aria-multiselectable={props.multi || undefined}
      data-fgroup={`${props.group}-r`}
      data-fentry
      onFocusIn={(e) => props.onFocusIn(e)}
    >
      <Show when={props.directory} fallback={<For each={props.results}>{(l) => props.row(l)}</For>}>
        <For each={letters()}>
          {(g) => (
            <div class="lp-letter" role="group" aria-label={g.letter}>
              <h3 aria-hidden="true">{g.letter}</h3>
              <For each={g.langs}>{(l) => props.row(l)}</For>
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}

/** The picker over the page (the phone's step; Settings on every form): inside the app's canvas, so TV and
 * desktop scale it as the rest; one Back (and the browser's Back) closes it; the page and its navigation
 * are inert behind it; the control that opened it has the focus again after. */
export function LanguageOverlay(props: { title: string; done: string; onClose: () => void; children: JSX.Element }) {
  const [box, setBox] = createSignal<{ top: number; height: number } | null>(null);
  onMount(() => {
    const opener = document.activeElement as HTMLElement | null;
    const behind = [...document.querySelectorAll<HTMLElement>(".screen, .topnav, .tabs, .nav-scrim")].filter((el) => !el.hasAttribute("inert"));
    for (const el of behind) el.setAttribute("inert", "");
    // The phone's keyboard covers the bottom of the layout viewport, and iOS scrolls the visual one inside
    // it: the layer follows the visible part.
    const vv = window.visualViewport;
    const fit = () => setBox(form() === "phone" && vv ? { top: vv.offsetTop, height: vv.height } : null);
    fit();
    vv?.addEventListener("resize", fit);
    vv?.addEventListener("scroll", fit);
    onCleanup(() => {
      vv?.removeEventListener("resize", fit);
      vv?.removeEventListener("scroll", fit);
      for (const el of behind) el.removeAttribute("inert");
      queueMicrotask(() => {
        if (opener?.isConnected) autoFocus(opener);
      });
    });
    onCleanup(pushBack(() => props.onClose(), { entry: true }));
  });
  return (
    <Portal mount={document.querySelector(".app") ?? document.body}>
      <div
        class="lp-layer"
        role="dialog"
        aria-modal="true"
        aria-label={props.title}
        style={box() ? { top: `${box()?.top}px`, height: `${box()?.height}px` } : undefined}
      >
        <header class="lp-top">
          <h2>{props.title}</h2>
          <button class="lp-done" data-f="lp:done" onClick={() => props.onClose()}>
            {props.done}
          </button>
        </header>
        <div class="lp-body" data-vscroll>
          {props.children}
        </div>
      </div>
    </Portal>
  );
}
