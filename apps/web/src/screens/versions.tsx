import { useLocation, useNavigate, useParams, useSearchParams } from "@solidjs/router";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import Ban from "lucide-solid/icons/ban";
import ChevronDown from "lucide-solid/icons/chevron-down";
import CircleCheck from "lucide-solid/icons/circle-check";
import Info from "lucide-solid/icons/info";
import OctagonAlert from "lucide-solid/icons/octagon-alert";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Sparkles from "lucide-solid/icons/sparkles";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import Undo2 from "lucide-solid/icons/undo-2";
import { createEffect, createMemo, createSignal, For, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import { Dynamic } from "solid-js/web";
import { Backdrop } from "../components/art";
import { downloadAdd } from "../components/download";
import { Chips } from "../components/grid";
import { EmptyActs } from "../components/empty-acts";
import { KeyedPage } from "../components/keyed-page";
import { Why } from "../components/why";
import { askKind, op, rawOf } from "../core/client";
import type { SectionQuality } from "../core/generated/SectionQuality";
import type { SourceKind } from "../core/generated/SourceKind";
import type { VersionRow } from "../core/generated/VersionRow";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { videoContext } from "../core/video";
import { focusEl, goBack } from "../focus/dom";
import { form } from "../form";
import { bytes, decoded, playPath, playState, releaseParts, titlePath } from "../lib/format";
import { downloadWords } from "../lib/download";
import { pluginRowLine, pluginWords } from "../lib/plugins";
import { entryFrom, ownState } from "../history";
import { onProfileReset } from "../core/profiles";
import {
  bestFor,
  emptyEntry,
  emptyWords,
  choiceWords,
  counted,
  kindBest,
  kindChips,
  type KindChoice,
  languageName,
  learnedNote,
  lineWords,
  nextVersion,
  qualityCell,
  qualityText,
  rowWords,
  lowerFirst,
  sheetDetails,
  versionsWords,
} from "../lib/words";
import "./browse.css";
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
  // `?purpose=download`: the versions to keep on this device, each row downloads (plans/offline-downloads.md 3.2).
  const [query] = useSearchParams();
  const purpose = () => (query.purpose === "download" ? ("download" as const) : undefined);
  // Choosing a version to download is not for TV, even by address: Home instead.
  const navigate = useNavigate();
  createEffect(() => {
    if (purpose() && form() === "tv") navigate("/", { replace: true });
  });
  // Try again on a title or a list that could not be loaded: the page anew.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <KeyedPage
      address={`${params.kind}/${params.id}/${params.videoId}/${purpose() ?? ""}/${attempt()}`}
      read={() => ({ kind: decoded(params.kind), id: decoded(params.id), videoId: decoded(params.videoId), purpose: purpose(), retry: () => setAttempt((n) => n + 1) })}
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

/** A version's one line, as the row and its sheet show it: the size first, then where it comes from, the
 * core's warning, or why it is dimmed (`why`, with a ban). */
function RowLine(props: { row: VersionRow; why: string | null; tag?: "span" | "p" }) {
  const tone = () => (props.why ? "why" : props.row.onDevice ? "ok" : lineWords(props.row.line).tone);
  return (
    <Dynamic component={props.tag ?? "span"} class={`st line ${tone()}`}>
      <Dynamic component={props.why ? Ban : props.row.onDevice ? CircleCheck : toneIcon[lineWords(props.row.line).tone]} aria-hidden class="tone" />
      <span class="lt">{rowWords(props.row, props.why)}</span>
    </Dynamic>
  );
}

/** A version's badges, as the row and its sheet show them: its quality when the release states it, a
 * language chip when it is another language than the viewer's, then its features. */
function Badges(props: { row: VersionRow }) {
  return (
    <>
      <span class="q">{qualityCell(props.row.quality)}</span>
      <span class="chips">
        <Show when={props.row.group === "otherLanguages" && languages(props.row)}>{(l) => <span class="lang">{l()}</span>}</Show>
        <For each={props.row.features}>{(f) => <span>{f}</span>}</For>
      </span>
    </>
  );
}

/** A row's details on one line: the line's details, the release name, the languages and sources. */
const detailsLine = (r: VersionRow) =>
  [!r.onDevice ? lineWords(r.line).details : null, r.releaseName, subLine(r)].filter((p): p is string => !!p).join("  ·  ");

/** The card's one why line: the core's line and its details joined by a colon ("On this device" where set). */
const whyLine = (r: VersionRow) => {
  if (r.onDevice) return r.onDevice;
  const w = lineWords(r.line);
  // After the colon the details go on as one sentence: "May start slowly: shared by 21 people".
  return w.details ? `${w.text}: ${lowerFirst(w.details)}` : w.text;
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
export function VersionsPage(props: { kind: string; id: string; videoId: string; retry: () => void; binge?: string; onPlay?: (key: string) => void; onBack?: () => void; hold?: boolean; purpose?: "download" }) {
  const navigate = useNavigate();
  // The player's layer shares the play's versions view (its binge group), never a second one.
  const ctx = videoContext(props.kind, props.id, props.videoId, props.binge, props.purpose);
  // To download: the top line, each action and a pick say so, and a pick keeps that version, then goes back.
  const keeping = props.purpose === "download";
  const best = () => (keeping ? downloadWords.pick : bestFor(form()));
  const actWord = keeping ? downloadWords.pickAction : "Play this version";
  const ActIcon = () => (keeping ? <ArrowDownToLine aria-hidden /> : <Play aria-hidden class="fill" />);
  createEffect(() => {
    const name = ctx.label() ?? ctx.details()?.name;
    if (name) document.title = `Versions · ${name} · Momotus`;
  });
  const snap = (): VersionsSnapshot | null => ctx.versions()?.state.data ?? null;
  // Why nothing can play (plans/play-every-version.md, the shared Step 3 table): the rows found, if
  // any, listed dimmed as they would rank; an account's rows cannot be started here, the others are an attempt.
  const empty = () => snap()?.empty ?? null;
  const dimmed = () => !!empty()?.rows.length;
  const startable = () => empty()?.kind !== "needsAccount";
  // A dimmed row says why in place of its own line, with a ban.
  const dimWord = () => {
    const e = empty();
    return e && dimmed() ? (emptyEntry(e.kind).row ?? null) : null;
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
  // Over the player (`hold`) the card and its shortcuts hold too: the version first shown in each
  // place stays while the core still lists it, so nothing changes under the viewer.
  const heldKeys = new Map<string, string>();
  const holdRow = (place: string, now: VersionRow | null | undefined): VersionRow | null => {
    if (!props.hold || !now) return now ?? null;
    const s = snap();
    const kept = heldKeys.get(place);
    const found = kept ? [s?.recommended, ...(s?.rows ?? []), s?.smaller, s?.sharper].find((r) => r?.key === kept) : undefined;
    if (found) return found;
    heldKeys.set(place, now.key);
    return now;
  };
  // The kind chips (plans/plugins-and-marketplace.md 4.2): the core counts the kinds and, under one
  // (`setKind`, which freezes the pick), sends that kind's own card, rows and sections and no
  // shortcuts. The chips show from two kinds on and then stay while the page is open.
  // A chip shows as on from the press until the core lists that kind, or soon after the core answered: an answer that
  // changes nothing (the kind emptied meanwhile) puts the chips back as the core lists them.
  const [pressed, setPressed] = createSignal<KindChoice | null>(null);
  createEffect(() => pressed() === (snap()?.selectedKind ?? "all") && setPressed(null));
  const shownKind = (): KindChoice => pressed() ?? snap()?.selectedKind ?? "all";
  const [chipsShown, setChipsShown] = createSignal(false);
  createEffect(() => (snap()?.kinds.length ?? 0) >= 2 && !dimmed() && setChipsShown(true));
  const chips = () => (chipsShown() ? kindChips(snap()?.kinds ?? []) : []);
  // The chip that is on again asks nothing. The kind holds while this video's Versions page or player is open;
  // any other page lists every kind again (app.tsx).
  const chooseKind = (k: KindChoice) => {
    if (k === shownKind()) return;
    setPressed(k);
    // ponytail: a fixed wait for the snapshot after the answer; key it on a snapshot sequence if one ever shows late.
    void askKind(k === "all" ? null : k)?.finally(() => setTimeout(() => pressed() === k && setPressed(null), 300));
  };
  // The chips, from two kinds on (no line kept for them before): over the card on TV and desktop, in the
  // header's room, where Down goes back to Play as Up from Play comes to the chip that is on; under the card
  // on the phone, so a row that arrives late never moves Play under a finger.
  const kindRow = () => (
    <div
      class="vkinds"
      onKeyDown={(e) => {
        const p = e.key === "ArrowDown" ? at("versions:play") : null;
        if (!p) return;
        e.preventDefault();
        focusEl(p);
      }}
    >
      <Show when={chips().length}>
        <Chips label={choiceWords.kind.label} group="vkinds" items={chips()} selected={shownKind()} onSelect={chooseKind} />
      </Show>
    </div>
  );
  // The page lists the kind the core answered with (`selectedKind`); only the chips show a press early.
  const listedKind = (): KindChoice => snap()?.selectedKind ?? "all";
  const pick = () => holdRow(`pick:${listedKind()}`, snap()?.recommended);
  const shortcut = (id: "smaller" | "sharper") => {
    const r = holdRow(id, snap()?.[id]);
    return r && r.key !== pick()?.key ? r : null;
  };
  const video = `${props.kind}/${props.id}/${props.videoId}`;
  const [open, setOpen] = createSignal<Set<Sec>>(expandedFor.get(video) ?? new Set());
  // The row the remote is on: a collapsed group shows at least down to it,
  // so a better row arriving above never pushes it off the page.
  const [focusedRow, setFocusedRow] = createSignal<string | null>(null);
  // A play opened here knows whether Versions came from the title page, so
  // its Episodes goes back to that one title entry (player.tsx).
  const play = (key: string) => {
    if (keeping) {
      const sub = ctx.versions()?.sub();
      if (sub == null) return;
      downloadAdd(props.kind, props.id, "video", { videoId: props.videoId, pick: { versionsSub: sub, key } });
      return (props.onBack ?? goBack)();
    }
    return props.onPlay ? props.onPlay(key) : navigate(playPath(props.kind, props.id, props.videoId), { state: { ...playState(key), viaTitle: entryFrom() === titlePath(props.kind, props.id) } });
  };

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
  // Phone: a row's info opens a sheet with its details and Play, on a history entry of its own
  // (`?info`), so Back closes it first. ponytail: popstate until design-web's pushBack lands.
  const location = useLocation();
  const [sheet, setSheet] = createSignal<VersionRow | null>(null);
  const infoOpen = () => new URLSearchParams(location.search).has("info");
  const withInfo = (on: boolean) => {
    const q = new URLSearchParams(location.search);
    if (on) q.set("info", "");
    else q.delete("info");
    return `${location.pathname}${q.size ? `?${q}` : ""}`;
  };
  const openSheet = (r: VersionRow) => {
    setSheet(r);
    navigate(withInfo(true), { state: ownState(location.state) });
  };
  // A sheet's Play waits for its entry to go, so the play (or the layer's choice) follows the page's.
  let playAfter: string | null = null;
  const closeSheet = (then: string | null = null) => {
    playAfter = then;
    history.back();
  };
  // A reload or Back to the page brings no row with it: the entry gives way in place.
  onMount(() => infoOpen() && navigate(withInfo(false), { replace: true, state: ownState(location.state) }));
  createEffect(
    on(
      infoOpen,
      (open) => {
        if (open) return queueMicrotask(() => at("vsheet:play")?.focus({ preventScroll: true }));
        const r = sheet();
        const then = playAfter;
        setSheet(null);
        playAfter = null;
        if (then) return play(then);
        if (r) at(`v:${r.key}`)?.focus({ preventScroll: true });
      },
      { defer: true },
    ),
  );
  // A swipe down on the sheet closes it.
  let swipeFrom: number | null = null;
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
    // Every row listed, the plugins' own pass too (4.4).
    const n = s.empty ? s.empty.count : s.rows.length + (s.recommended ? 1 : 0) + s.plugins.rows.length;
    return n ? counted("versions", n) : null;
  };
  const doneWords = () => {
    const e = empty();
    return !snap()?.done ? "" : e ? emptyWords(e).text : (count() ?? "");
  };
  const statusLine = () => {
    const s = snap();
    if (ctx.failure()) return "";
    const e = empty();
    if (e) return emptyWords(e).text;
    // Until a row or the core's answer (once every addon is done, it may still check a kept source).
    // Only rows of the plugins' own pass: never picked, so the viewer chooses (4.4).
    if (s?.done && !s.recommended && !s.rows.length && s.plugins.rows.length) return pluginWords.choose;
    if (!s || (!s.recommended && !s.rows.length)) return versionsWords.finding;
    if (s.pendingAddons.length || s.slowAddons.length) return versionsWords.slow;
    return "";
  };

  return (
    <div class="versions-scr">
      <Backdrop src={ctx.details()?.background} class="versions-bd" dim />
      {/* The page under the sheet is inert: nothing behind it can be reached or played. */}
      <div class="vmain" inert={!!(infoOpen() && sheet()) || undefined}>
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
          <p class="vstatus">{statusLine()}</p>
          <Show when={form() !== "phone"}>{kindRow()}</Show>
          {/* Said once when the list is done (3.6): the count, or why nothing can play; never each row. */}
          <p class="sr" role="status">
            {doneWords()}
          </p>
        </header>

        <Switch>
          {/* The title's details, or the list itself, could not be loaded:
              the reason, Try again (the page anew) and Back, never "Opening". */}
          <Match when={ctx.failure() ?? (ctx.versions()?.state.status.state === "failed" ? ((ctx.versions()?.state.status as { reason?: string }).reason ?? "") : null)}>
            {(reason) => (
              <section class="vempty problem">
                <h2>{ctx.failure() ? nextVersion.titleNotOpened : nextVersion.listNotLoaded}</h2>
                <Why reason={reason()} raw={rawOf(ctx.failure() ? ctx.title.state.status : ctx.versions()?.state.status)} />
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
                      <Show when={!(snap()?.done && snap()?.plugins.rows.length)} fallback={<p class="best">{pluginWords.choose}</p>}>
                      <div class="pick-wait" aria-busy="true">
                        <p class="best">{best()}</p>
                        <div class="skel" />
                        <div class="skel" />
                        <div class="skel" />
                      </div>
                      </Show>
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
                    <p class="best">{keeping || listedKind() === "all" ? best() : kindBest(listedKind() as SourceKind, snap()?.kinds ?? [])}</p>
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
                      aria-label={`${keeping ? "Download" : "Play"} ${rowName(p())}`}
                      aria-describedby="vwhy"
                      onClick={() => play(p().key)}
                    >
                      <ActIcon />
                      {actWord}
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

            <Show when={form() === "phone"}>{kindRow()}</Show>
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
                                    classList={{ dim: dimmed() }}
                                    data-f={`v:${r.key}`}
                                    aria-disabled={dimmed() && !startable() ? "true" : undefined}
                                    onClick={() => (dimmed() && !startable() ? undefined : play(r.key))}
                                  >
                                    <Badges row={r} />
                                    <RowLine row={r} why={dimWord()} />
                                    {/* Reserved in every row: focus or hover changes only its opacity. Seen only: the
                                        reader gets the same words as the row's description (hidden, outside its name). */}
                                    <span class="more" aria-hidden="true">
                                      {detailsLine(r)}
                                    </span>
                                  </button>
                                  <span id={`vd-${g}-${i()}`} hidden>
                                    {detailsLine(r)}
                                  </span>
                                  {/* Phone: one tap plays; the info button beside the row opens its sheet. The row
                                      is one node for the reader (its details are its description). */}
                                  <Show when={form() === "phone"}>
                                    <button class="vinfo" aria-hidden="true" tabindex="-1" onClick={() => openSheet(r)}>
                                      <Info />
                                    </button>
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
                {/* "From plugins" (plans/plugins-and-marketplace.md 4.4, 7.8): their own pass, after the
                    addons' rows, never the pick; a row plays only when chosen and says where it plays from. */}
                <Show when={!dimmed() && ((snap()?.plugins.rows.length ?? 0) > 0 || snap()?.plugins.pending)}>
                  <section class="vgroup" data-row data-group="plugins">
                    <div class="vghead">
                      <h3 id="vg-plugins">
                        {pluginWords.section}
                        <span class="num">{snap()?.plugins.rows.length ?? 0}</span>
                      </h3>
                    </div>
                    <div id="vl-plugins" role="list" aria-labelledby="vg-plugins">
                      <For each={snap()?.plugins.rows ?? []}>
                        {(r) => (
                          <div class="vslot" role="listitem">
                            <button class="vrow" data-f={`v:${r.key}`} onClick={() => play(r.key)}>
                              <Badges row={r} />
                              <span class="st line">
                                <span class="lt" dir="auto">
                                  <bdi>{pluginRowLine(r)}</bdi>
                                </span>
                              </span>
                            </button>
                          </div>
                        )}
                      </For>
                    </div>
                    <Show when={snap()?.plugins.pending}>
                      <p class="vstatus" role="status">
                        {pluginWords.looking}
                      </p>
                    </Show>
                  </section>
                </Show>
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

      <Show when={infoOpen() && sheet()}>
        {(r) => (
          <div class="vsheet-scrim" onClick={(e) => e.target === e.currentTarget && closeSheet()}>
            <section
              class="vsheet"
              role="dialog"
              aria-modal="true"
              aria-label={rowName(r())}
              onPointerDown={(e) => (swipeFrom = e.clientY)}
              onPointerUp={(e) => {
                if (swipeFrom !== null && e.clientY - swipeFrom > 60) closeSheet();
                swipeFrom = null;
              }}
            >
              <div class="grab" aria-hidden="true" />
              {/* First what the row shows (its badges and its one line), then what the row keeps for
                  its details: the line's own (the seeders), the file and its languages. */}
              {/* Only when it holds a badge: a version with no stated quality, no features and the viewer's
                  language starts with its one line, with no empty band above it. */}
              <Show when={qualityCell(r().quality) || r().features.length || (r().group === "otherLanguages" && languages(r()))}>
                <div class="vsheet-badges">
                  <Badges row={r()} />
                </div>
              </Show>
              <RowLine row={r()} why={dimWord()} tag="p" />
              <Show when={sheetDetails(r(), dimWord())}>{(d) => <p class="detail">{d()}</p>}</Show>
              <p class="rel">
                <Release name={r().releaseName} />
              </p>
              <Show when={subLine(r())}>{(l) => <p class="sub">{l()}</p>}</Show>
              <div class="acts">
                <Show when={!dimmed() || startable()}>
                  <button class="btn primary" data-f="vsheet:play" onClick={() => closeSheet(r().key)}>
                    <ActIcon />
                    {actWord}
                  </button>
                </Show>
                <button class="btn" data-f="vsheet:done" onClick={() => closeSheet()}>
                  {versionsWords.done}
                </button>
              </div>
            </section>
          </div>
        )}
      </Show>
    </div>
  );
}
