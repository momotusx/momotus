import { useNavigate, useParams } from "@solidjs/router";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import Ban from "lucide-solid/icons/ban";
import ChevronDown from "lucide-solid/icons/chevron-down";
import CircleCheck from "lucide-solid/icons/circle-check";
import Lock from "lucide-solid/icons/lock";
import OctagonAlert from "lucide-solid/icons/octagon-alert";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Sparkles from "lucide-solid/icons/sparkles";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import Undo2 from "lucide-solid/icons/undo-2";
import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { Dynamic } from "solid-js/web";
import { Backdrop } from "../components/art";
import { EmptyActs } from "../components/empty-acts";
import { KeyedPage } from "../components/keyed-page";
import { Why } from "../components/why";
import { op } from "../core/client";
import type { SectionQuality } from "../core/generated/SectionQuality";
import type { ReasonLine } from "../core/generated/ReasonLine";
import type { VersionRow } from "../core/generated/VersionRow";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { videoContext } from "../core/video";
import { focusEl, goBack } from "../focus/dom";
import { form } from "../form";
import { bytes, decoded, playPath, playState, releaseParts, titlePath } from "../lib/format";
import { entryFrom } from "../history";
import { onProfileReset } from "../core/profiles";
import {
  emptyWords,
  choiceWords,
  counted,
  languageName,
  learnedNote,
  lineWords,
  nextVersion,
  qualityCell,
  qualityText,
  versionsWords,
} from "../lib/words";
import "./versions.css";

const SHOWN = 3;
// The sections the viewer opened ("Show all"), per video: Back from a play
// finds the row it opened still listed.
// The sections in their fixed order; "found" is the rows listed dimmed when none can play (no heading).
type Sec = SectionQuality | "found";
const ORDER: Sec[] = ["uhd", "fhd", "hd", "other", "found"];
const expandedFor = new Map<string, Set<Sec>>();
onProfileReset(() => expandedFor.clear());

export default function Versions() {
  const params = useParams();
  // Try again on a title or a list that could not be loaded: the page anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <KeyedPage
      address={`${params.kind}/${params.id}/${params.videoId}/${attempt()}`}
      read={() => ({ kind: decoded(params.kind), id: decoded(params.id), videoId: decoded(params.videoId), retry: () => setAttempt((n) => n + 1) })}
      page={VersionsPage}
    />
  );
}

export function Release(props: { name: string }) {
  return (
    <For each={releaseParts(props.name)}>
      {(part) => (
        <>
          {part}
          <wbr />
        </>
      )}
    </For>
  );
}

const toneIcon = { ok: CircleCheck, warn: TriangleAlert, bad: OctagonAlert };

/** The row's line: a tone icon (its shape, never color alone) and the plain
 * words, one line. Its details sit in the row's reserved details line. */
function LineWord(props: { line: ReasonLine }) {
  const w = () => lineWords(props.line);
  return (
    <span class={`st line ${w().tone}`}>
      <Dynamic component={toneIcon[w().tone]} aria-hidden class="tone" />
      <span class="lt">{w().text}</span>
    </span>
  );
}

/** A row's details on one line: the line's details, the release name, the languages and sources. */
const detailsLine = (r: VersionRow) =>
  [!r.onDevice ? lineWords(r.line).details : null, r.releaseName, subLine(r)].filter((p): p is string => !!p).join("  ·  ");

/** The card's one why line: the core's line and its details joined by a colon ("On this device" where set). */
const whyLine = (r: VersionRow) => {
  if (r.onDevice) return r.onDevice;
  const w = lineWords(r.line);
  return w.details ? `${w.text}: ${w.details}` : w.text;
};
/** "1080p HDR, 2.1 GB": a version's name for a screen reader. */
const rowName = (r: VersionRow) => [`${qualityText(r.quality)}${r.features.length ? ` ${r.features.join(" ")}` : ""}`, r.sizeBytes ? bytes(r.sizeBytes) : null].filter(Boolean).join(", ");

/** The element that scrolls `el`: the list on TV and desktop, the page on the phone. */
function scrollerOf(el: HTMLElement): HTMLElement {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if ((y === "auto" || y === "scroll") && p.scrollHeight > p.clientHeight) return p;
  }
  return document.scrollingElement as HTMLElement;
}

const languages = (r: VersionRow) => r.languages.map(languageName).join(", ");
const subLine = (r: VersionRow) => [languages(r) || null, r.failoverCount > 0 ? `${r.failoverCount + 1} sources` : null].filter(Boolean).join(", ");

/** The player's layer: `onPlay` takes the choice (it replaces the play under it), `onBack` closes it, `hold`
 * keeps the order the rows had when it opened. Else a choice opens a new play and Back leaves. */
export function VersionsPage(props: { kind: string; id: string; videoId: string; retry: () => void; binge?: string; onPlay?: (key: string) => void; onBack?: () => void; hold?: boolean }) {
  const navigate = useNavigate();
  // The player's layer shares the play's versions view (its binge group), never a second one.
  const ctx = videoContext(props.kind, props.id, props.videoId, props.binge);
  createEffect(() => {
    const name = ctx.label() ?? ctx.details()?.name;
    if (name) document.title = `Versions · ${name} · Marquee`;
  });
  const snap = (): VersionsSnapshot | null => ctx.versions()?.state.data ?? null;
  // Phone: a tap opens the row (its details and Play); elsewhere it plays.
  const [opened, setOpened] = createSignal<string | null>(null);
  const tapRow = (r: VersionRow) => (form() === "phone" ? setOpened(opened() === r.key ? null : r.key) : play(r.key));
  // Why nothing can play (plans/play-every-version.md, the shared Step 3 table): the rows found, if
  // any, listed dimmed as they would rank; an account's rows cannot be started here, the others are an attempt.
  const empty = () => snap()?.empty ?? null;
  const dimmed = () => !!empty()?.rows.length;
  const startable = () => empty()?.kind !== "needsAccount";
  // A dimmed row says why in place of its own line: a lock for an account, a ban for this device.
  const dimWord = () => {
    const e = empty();
    return e && dimmed() ? ((choiceWords.empty[e.kind] as { row?: string }).row ?? null) : null;
  };
  // The one-time learning note (3.9), until Undo.
  const [undone, setUndone] = createSignal(false);
  const note = () => {
    const n = snap()?.note;
    return n && !undone() ? { ...n, text: learnedNote(n.choice, n.up, form(), n.prefs) } : null;
  };
  const undo = () => {
    const n = snap()?.note;
    if (!n) return;
    setUndone(true);
    void op("undoLearned", { screenClass: n.screenClass, choice: n.choice }).catch(() => setUndone(false));
  };
  const pick = () => snap()?.recommended ?? null;
  const shortcut = (id: "smaller" | "sharper") => {
    const r = snap()?.[id];
    return r && r.key !== pick()?.key ? r : null;
  };
  const video = `${props.kind}/${props.id}/${props.videoId}`;
  const [open, setOpen] = createSignal<Set<Sec>>(expandedFor.get(video) ?? new Set());
  // The row the remote is on: a collapsed group shows at least down to it,
  // so a better row arriving above never pushes it off the page.
  const [focusedRow, setFocusedRow] = createSignal<string | null>(null);
  // A play opened here knows whether Versions came from the title page, so
  // its Episodes goes back to that one title entry (player.tsx).
  const play = (key: string) =>
    props.onPlay ? props.onPlay(key) : navigate(playPath(props.kind, props.id, props.videoId), { state: { ...playState(key), viaTitle: entryFrom() === titlePath(props.kind, props.id) } });

  // The pick under the cursor never changes once the user starts moving.
  const freeze = () => {
    const v = ctx.versions();
    if (v && snap() && !snap()?.frozen) void v.command({ type: "freeze" });
  };
  onMount(() => {
    const onKey = (e: KeyboardEvent) => (e.key.startsWith("Arrow") || e.key === "Tab") && freeze();
    window.addEventListener("keydown", onKey, { capture: true });
    window.addEventListener("pointerdown", freeze, { capture: true });
    onCleanup(() => {
      window.removeEventListener("keydown", onKey, { capture: true });
      window.removeEventListener("pointerdown", freeze, { capture: true });
    });
  });

  // The rows listed now, and one section's of them (the core's sections, by key). Sections come from
  // the fixed order, so a late addon or a re-rank never rebuilds them: the focused row keeps its element.
  const listed = () => (dimmed() ? empty()?.rows : snap()?.rows) ?? [];
  const byKey = createMemo(() => new Map((snap()?.rows ?? []).map((r) => [r.key, r])));
  // Over the player (`hold`), each section keeps the order it showed when the layer opened; keys
  // that arrive later go after it, so nothing moves under the viewer while the attempt resolves.
  const held = new Map<Sec, string[]>();
  const keysOf = (g: Sec): string[] => {
    if (dimmed()) return g === "found" ? listed().map((r) => r.key) : [];
    const keys = snap()?.sections.find((s) => s.quality === g)?.keys ?? [];
    if (!props.hold) return keys;
    const kept = (held.get(g) ?? []).filter((k) => keys.includes(k));
    const order = [...kept, ...keys.filter((k) => !kept.includes(k))];
    held.set(g, order);
    return order;
  };
  const rowsOf = (g: Sec): VersionRow[] => {
    const rows = dimmed() ? new Map(listed().map((r) => [r.key, r])) : byKey();
    return keysOf(g)
      .map((k) => rows.get(k))
      .filter((r): r is VersionRow => !!r);
  };
  const label = (g: Sec) => (g === "found" ? null : versionsWords.sections[g]);
  const toggle = (g: Sec) => {
    const next = new Set(open());
    if (next.has(g)) next.delete(g);
    else next.add(g);
    setOpen(next);
    expandedFor.set(video, next);
  };
  const at = (f: string) => document.querySelector<HTMLElement>(`[data-f="${CSS.escape(f)}"]`);
  // Show all: the section opens and the focus goes to its first row that was hidden.
  const unfold = (g: Sec) => {
    toggle(g);
    const row = at(`v:${rowsOf(g)[SHOWN]?.key ?? ""}`);
    if (row) focusEl(row);
  };
  // Show fewer: the section folds and the focus goes to its bottom toggle, with no jump: the bottom
  // toggle stays where it was pressed, or, pressed from the pinned heading, the section's top stays
  // where its heading was, so the toggle shows under its rows. The scroll is set here (Safari has no
  // scroll anchoring), from the scroller found before the list shrinks.
  const foldAt = (g: Sec, from: HTMLElement) => {
    const head = from.dataset.f?.startsWith("foldtop:") ? from.closest<HTMLElement>(".vghead") : null;
    const scroller = scrollerOf(from);
    const top = (head ?? from).getBoundingClientRect().top;
    toggle(g);
    const t = at(`fold:${g}`);
    t?.focus({ preventScroll: true });
    // The heading's own place in its section (it may be pinned where it was drawn), else the toggle's.
    const section = head?.closest<HTMLElement>("[data-group]");
    const now = head && section ? section.getBoundingClientRect().top + Number.parseFloat(getComputedStyle(head).marginTop) : t?.getBoundingClientRect().top;
    if (now !== undefined) scroller.scrollTop += now - top;
  };
  // Left from a row of an open section goes to its heading's Show fewer, Left again to Play; Home or
  // PageUp does the same on a keyboard.
  const listKeys = (e: KeyboardEvent) => {
    const el = e.target as HTMLElement;
    const g = el.closest<HTMLElement>("[data-group]")?.dataset.group as Sec | undefined;
    const onRow = !!el.dataset.f?.startsWith("v:");
    const head = g && open().has(g) && onRow ? at(`foldtop:${g}`) : null;
    if (e.key === "ArrowLeft" && form() === "tv") {
      const to = head ?? at("versions:play");
      if (!to) return;
      e.preventDefault();
      to.focus();
    } else if ((e.key === "Home" || e.key === "PageUp") && form() === "desktop" && head) {
      e.preventDefault();
      head.focus();
    }
  };
  // The header's count, then its one reserved status line: never an addon's name.
  const count = () => {
    const s = snap();
    if (!s) return null;
    const n = s.empty ? s.empty.count : s.rows.length + (s.recommended ? 1 : 0);
    return n ? counted("versions", n) : null;
  };
  const statusLine = () => {
    const s = snap();
    if (ctx.failure()) return "";
    const e = empty();
    if (e) return emptyWords(e).text;
    // Until a row or the core's answer (once every addon is done, it may still check a kept source).
    if (!s || (!s.recommended && !s.rows.length)) return versionsWords.finding;
    if (s.pendingAddons.length || s.slowAddons.length) return versionsWords.slow;
    return "";
  };

  return (
    <div class="versions-scr">
      <Backdrop src={ctx.details()?.background} class="versions-bd" dim />
      <header class="vhead">
        <div class="vhead-row">
          <h1>
            Versions
            <Show when={ctx.label() ?? ctx.details()?.name}>{(l) => <span>for {l()}</span>}</Show>
          </h1>
          <p class="vstate">
            <Show when={count()}>{(c) => <span class="num">{c()}</span>}</Show>
          </p>
        </div>
        {/* Reserved from the first paint, so nothing below moves when it fills or empties. */}
        <p class="vstatus" role="status">
          {statusLine()}
        </p>
      </header>

      <Switch>
        {/* The title's details, or the list itself, could not be loaded:
            the reason, Try again (the page anew) and Back, never "Opening". */}
        <Match when={ctx.failure() ?? (ctx.versions()?.state.status.state === "failed" ? ((ctx.versions()?.state.status as { reason?: string }).reason ?? "") : null)}>
          {(reason) => (
            <section class="vempty problem">
              <h2>{ctx.failure() ? nextVersion.titleNotOpened : nextVersion.listNotLoaded}</h2>
              <Why reason={reason()} />
              <div class="acts">
                <button class="btn primary" data-f="versions:retry" data-autofocus onClick={() => props.retry()}>
                  <RefreshCw aria-hidden />
                  Try again
                </button>
                <button class="btn" data-f="versions:back" onClick={() => (props.onBack ?? goBack)()}>
                  <ArrowLeft aria-hidden />
                  Back
                </button>
              </div>
            </section>
          )}
        </Match>
        <Match when={true}>
          <section class="pick" aria-label="Recommended version">
            <Show
              when={pick()}
              fallback={
                <Show
                  when={empty()}
                  fallback={
                    <div class="pick-wait" aria-busy="true">
                      <p class="best">Best for this screen</p>
                      <div class="skel" />
                      <div class="skel" />
                      <div class="skel" />
                    </div>
                  }
                >
                  {(e) => {
                    const lone = () => !emptyWords(e(), true).steps.length;
                    return (
                      // The line is the header's status line; here, the state's actions (the shared table).
                      <div class="blocked acts">
                        <EmptyActs empty={e()} at="versions" onVersions retry={props.retry} play={play} />
                        <button class="btn" classList={{ primary: lone() }} data-f="versions:back" data-autofocus={lone() ? "" : undefined} onClick={() => (props.onBack ?? goBack)()}>
                          <ArrowLeft aria-hidden />
                          {versionsWords.back}
                        </button>
                      </div>
                    );
                  }}
                </Show>
              }
            >
              {(p) => (
                <>
                  <p class="best">Best for this screen</p>
                  <h2>{qualityText(p().quality)}</h2>
                  <Show when={p().features.length}>
                    <p class="badges">
                      <For each={p().features}>{(f) => <span>{f}</span>}</For>
                    </p>
                  </Show>
                  {/* One why line (the core's line and its details), clamped; the full sentence describes Play. */}
                  <p class="why1" id="vwhy">
                    {whyLine(p())}
                  </p>
                  <button
                    class="btn primary"
                    data-f="versions:play"
                    data-autofocus
                    aria-label={`Play ${rowName(p())}`}
                    aria-describedby="vwhy"
                    onClick={() => play(p().key)}
                  >
                    <Play aria-hidden class="fill" />
                    Play this version
                  </button>
                  {/* The core's shortcuts, each only when it is not the pick (it stays in its section too). */}
                  <For each={["smaller", "sharper"] as const}>
                    {(id) => (
                      <Show when={shortcut(id)}>
                        {(r) => (
                          <button
                            class="vshort"
                            data-f={`vshort:${id}`}
                            aria-label={`${versionsWords[id]}: ${qualityText(r().quality)}${r().sizeBytes ? `, ${bytes(r().sizeBytes as number)}` : ""}`}
                            onClick={() => play(r().key)}
                          >
                            <span class="w">{versionsWords[id]}</span>
                            <span class="q">{qualityCell(r().quality)}</span>
                            <span class="chips">
                              <For each={r().features}>{(f) => <span>{f}</span>}</For>
                            </span>
                            <span class="size num">{r().sizeBytes ? bytes(r().sizeBytes as number) : ""}</span>
                          </button>
                        )}
                      </Show>
                    )}
                  </For>
                </>
              )}
            </Show>
          </section>

          <section class="alts" aria-label="All versions">
            <div
              class="alist"
              classList={{ dimmed: dimmed() }}
              data-vscroll
              data-fgroup="alts"
              onFocusIn={(e) => {
                const f = (e.target as HTMLElement).dataset.f;
                setFocusedRow(f?.startsWith("v:") ? f.slice(2) : null);
              }}
              onFocusOut={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusedRow(null);
              }}
              onKeyDown={listKeys}
            >
              <For each={ORDER}>
                {(g) => {
                  const rows = createMemo(() => rowsOf(g));
                  const expanded = () => open().has(g);
                  const shown = () => {
                    if (expanded()) return rows();
                    const at = rows().findIndex((r) => r.key === focusedRow());
                    return rows().slice(0, Math.max(SHOWN, at + 1));
                  };
                  return (
                    <Show when={rows().length > 0}>
                      <section class="vgroup" data-row data-group={g}>
                        {/* An open section's heading holds Show fewer: Up from its first row, or Left
                            from any of its rows (Home on a keyboard), leaves a long list in one press. */}
                        <div class="vghead" classList={{ bare: !label(g) }}>
                          <Show when={label(g)}>
                            {(l) => (
                              <h3 id={`vg-${g}`}>
                                {l()}
                                <span class="num">{rows().length}</span>
                              </h3>
                            )}
                          </Show>
                          <Show when={expanded()}>
                            <button
                              class="fold top"
                              data-f={`foldtop:${g}`}
                              aria-expanded="true"
                              aria-controls={`vl-${g}`}
                              aria-label={`${versionsWords.showFewer} ${label(g) ?? ""} versions`.replace("  ", " ")}
                              onClick={(e) => foldAt(g, e.currentTarget)}
                            >
                              <ChevronDown aria-hidden />
                              {versionsWords.showFewer}
                            </button>
                          </Show>
                        </div>
                        <div id={`vl-${g}`} role="list" aria-labelledby={label(g) ? `vg-${g}` : undefined}>
                          <For each={shown()}>
                            {(r, i) => (
                              <div class="vslot" role="listitem">
                                <button
                                  class="vrow"
                                  aria-describedby={`vd-${g}-${i()}`}
                                  classList={{ open: opened() === r.key, dim: dimmed() }}
                                  data-f={`v:${r.key}`}
                                  aria-expanded={form() === "phone" ? opened() === r.key : undefined}
                                  aria-disabled={dimmed() && !startable() ? "true" : undefined}
                                  onClick={() => (dimmed() && !startable() ? undefined : tapRow(r))}
                                >
                                  <span class="q" classList={{ unknown: r.quality === "Unknown" }}>
                                    {qualityCell(r.quality)}
                                  </span>
                                  <span class="chips">
                                    {/* Another language than the viewer's: the chip names it. */}
                                    <Show when={r.group === "otherLanguages" && languages(r)}>{(l) => <span class="lang">{l()}</span>}</Show>
                                    <For each={r.features}>{(f) => <span>{f}</span>}</For>
                                  </span>
                                  <span class="size num">{r.sizeBytes ? bytes(r.sizeBytes) : ""}</span>
                                  {/* The core's "On this device" stands for the row's line where it is set. */}
                                  <Show
                                    when={r.onDevice}
                                    fallback={
                                      <Show when={dimWord()} fallback={<LineWord line={r.line} />}>
                                        {(w) => (
                                          <span class="st line why">
                                            <Dynamic component={empty()?.kind === "needsAccount" ? Lock : Ban} aria-hidden class="tone" />
                                            <span class="lt">{w()}</span>
                                          </span>
                                        )}
                                      </Show>
                                    }
                                  >
                                    {(w) => (
                                      <span class="st line ok">
                                        <CircleCheck aria-hidden class="tone" />
                                        <span class="lt">{w()}</span>
                                      </span>
                                    )}
                                  </Show>
                                  {/* Reserved in every row: focus or hover changes only its opacity. Seen only: the
                                      reader gets the same words as the row's description (hidden, outside its name). */}
                                  <span class="more" aria-hidden="true">
                                    {detailsLine(r)}
                                  </span>
                                </button>
                                <span id={`vd-${g}-${i()}`} hidden>
                                  {detailsLine(r)}
                                </span>
                                <Show when={form() === "phone" && opened() === r.key}>
                                  <div class="vrow-open">
                                    <Show when={!r.onDevice && lineWords(r.line).details}>
                                      {(d) => <p class="detail">{d()}</p>}
                                    </Show>
                                    <p class="rel">
                                      <Release name={r.releaseName} />
                                    </p>
                                    <Show when={subLine(r)}>{(l) => <p class="sub">{l()}</p>}</Show>
                                    <button class="btn" data-f={`vplay:${r.key}`} onClick={() => play(r.key)}>
                                      <Play aria-hidden class="fill" />
                                      Play this version
                                    </button>
                                  </div>
                                </Show>
                              </div>
                            )}
                          </For>
                        </div>
                        <Show when={rows().length > SHOWN}>
                          <button
                            class="fold"
                            data-f={`fold:${g}`}
                            aria-expanded={expanded()}
                            aria-controls={`vl-${g}`}
                            aria-label={`${expanded() ? versionsWords.showFewer : versionsWords.showAll(rows().length)} ${label(g) ?? ""} versions`.replace("  ", " ")}
                            onClick={(e) => (expanded() ? foldAt(g, e.currentTarget) : unfold(g))}
                          >
                            <ChevronDown aria-hidden />
                            {expanded() ? versionsWords.showFewer : versionsWords.showAll(rows().length)}
                          </button>
                        </Show>
                      </section>
                    </Show>
                  );
                }}
              </For>
              <Show when={snap() && !snap()?.done && listed().length === 0}>
                <div class="alist-wait" aria-hidden="true">
                  <For each={[0, 1, 2, 3, 4]}>{() => <div class="skel" />}</For>
                </div>
              </Show>
              {/* The learning note under the sections, where its arrival moves nothing above. */}
              <Show when={note()?.text}>
                {(text) => (
                  <p class="lnote" role="status">
                    <Sparkles aria-hidden />
                    <span>{text()}</span>
                    <button class="btn" data-f="versions:undo" onClick={undo}>
                      <Undo2 aria-hidden />
                      {choiceWords.learned.undo}
                    </button>
                  </p>
                )}
              </Show>
            </div>
          </section>
        </Match>
      </Switch>
    </div>
  );
}
