import { A, useNavigate, useParams } from "@solidjs/router";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import Check from "lucide-solid/icons/check";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import CircleCheck from "lucide-solid/icons/circle-check";
import Eye from "lucide-solid/icons/eye";
import EyeOff from "lucide-solid/icons/eye-off";
import KeyRound from "lucide-solid/icons/key-round";
import MonitorCheck from "lucide-solid/icons/monitor-check";
import OctagonAlert from "lucide-solid/icons/octagon-alert";
import Play from "lucide-solid/icons/play";
import Puzzle from "lucide-solid/icons/puzzle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Sparkles from "lucide-solid/icons/sparkles";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import Undo2 from "lucide-solid/icons/undo-2";
import UserRoundX from "lucide-solid/icons/user-round-x";
import { createEffect, createMemo, createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { Dynamic } from "solid-js/web";
import { Backdrop } from "../components/art";
import { KeyedPage } from "../components/keyed-page";
import { Why } from "../components/why";
import { op } from "../core/client";
import type { Group } from "../core/generated/Group";
import type { ReasonLine } from "../core/generated/ReasonLine";
import type { VersionRow } from "../core/generated/VersionRow";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { videoContext } from "../core/video";
import { goBack } from "../focus/dom";
import { form } from "../form";
import { bytes, decoded, playPath, playState, releaseParts, titlePath } from "../lib/format";
import { entryFrom } from "../history";
import { onProfileReset } from "../core/profiles";
import {
  blockedWords,
  choiceWords,
  conditionsHeader,
  counted,
  groupLabel,
  groupOrder,
  hiddenSummary,
  languageName,
  learnedNote,
  lineWords,
  nextVersion,
  noVersionWords,
  qualityCell,
  qualityText,
  reasonText,
  slowWords,
  statusText,
} from "../lib/words";
import "./versions.css";

const SHOWN = 3;
// The groups the viewer opened ("Show all"), per video: Back from a play
// finds the row it opened still listed.
const expandedFor = new Map<string, Set<Group>>();
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

/** The core's `nobodySharing`: every version is a swarm with one seeder or
 * none right now, said before Play on the card and over the list. */
function NobodySharing(props: { class: string; status?: boolean }) {
  return (
    <p class={props.class} role={props.status ? "status" : undefined}>
      <UserRoundX aria-hidden />
      <span>
        <b>{choiceWords.nobodySharing.text}</b>
        <small>{choiceWords.nobodySharing.details}</small>
      </span>
    </p>
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

function StatusWord(props: { row: VersionRow }) {
  const st = () => statusText(props.row.status);
  return (
    <span class={`st ${st().tone}`}>
      <i class="dot" />
      {st().text}
    </span>
  );
}

const toneIcon = { ok: CircleCheck, warn: TriangleAlert, bad: OctagonAlert };

/** The row's line: a tone icon (its shape, never color alone) and the plain
 * words. The details show on focus (TV), hover (desktop) or tap (phone). */
function LineWord(props: { line: ReasonLine }) {
  const w = () => lineWords(props.line);
  return (
    <span class={`st line ${w().tone}`}>
      <Dynamic component={toneIcon[w().tone]} aria-hidden class="tone" />
      <span class="lt">{w().text}</span>
      <Show when={w().details}>
        <span class="tip" aria-hidden="true">
          {w().details}
        </span>
      </Show>
    </span>
  );
}

const languages = (r: VersionRow) => r.languages.map(languageName).join(", ");
const subLine = (r: VersionRow) => [languages(r) || null, r.failoverCount > 0 ? `${r.failoverCount + 1} sources` : null].filter(Boolean).join(", ") || "Language not stated";

function VersionsPage(props: { kind: string; id: string; videoId: string; retry: () => void }) {
  const navigate = useNavigate();
  const ctx = videoContext(props.kind, props.id, props.videoId);
  createEffect(() => {
    const name = ctx.label() ?? ctx.details()?.name;
    if (name) document.title = `Versions · ${name} · Marquee`;
  });
  const snap = (): VersionsSnapshot | null => ctx.versions()?.state.data ?? null;
  // Phone: a tap opens the row (its details and Play); elsewhere it plays.
  const [opened, setOpened] = createSignal<string | null>(null);
  const tapRow = (r: VersionRow) => (form() === "phone" ? setOpened(opened() === r.key ? null : r.key) : play(r.key));
  // Blocked (3.11): "Show them" lists the rows as they would rank once fixed.
  const [showBlocked, setShowBlocked] = createSignal(false);
  const blocked = () => snap()?.blocked ?? null;
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
  const video = `${props.kind}/${props.id}/${props.videoId}`;
  const [open, setOpen] = createSignal<Set<Group>>(expandedFor.get(video) ?? new Set());
  // The row the remote is on: a collapsed group shows at least down to it,
  // so a better row arriving above never pushes it off the page.
  const [focusedRow, setFocusedRow] = createSignal<string | null>(null);
  // A play opened here knows whether Versions came from the title page, so
  // its Episodes goes back to that one title entry (player.tsx).
  const play = (key: string) =>
    navigate(playPath(props.kind, props.id, props.videoId), { state: { ...playState(key), viaTitle: entryFrom() === titlePath(props.kind, props.id) } });

  // The pick under the cursor never changes once the user starts moving.
  const freeze = () => {
    const v = ctx.versions();
    if (v && snap() && !snap()?.frozen) void v.command({ type: "freeze" });
  };
  onMount(() => {
    const onKey = (e: KeyboardEvent) => e.key.startsWith("Arrow") && freeze();
    window.addEventListener("keydown", onKey, { capture: true });
    window.addEventListener("pointerdown", freeze, { capture: true });
    onCleanup(() => {
      window.removeEventListener("keydown", onKey, { capture: true });
      window.removeEventListener("pointerdown", freeze, { capture: true });
    });
  });

  // The rows listed now, and one group's of them. Sections come from the fixed
  // group order, so a late addon or a re-rank never rebuilds them: the
  // focused row keeps its element.
  const listed = () => (showBlocked() ? blocked()?.rows : snap()?.rows) ?? [];
  const rowsOf = (g: Group) => listed().filter((r) => r.group === g);
  const toggle = (g: Group) => {
    const next = new Set(open());
    if (next.has(g)) next.delete(g);
    else next.add(g);
    setOpen(next);
    expandedFor.set(video, next);
  };
  const progress = () => {
    const s = snap();
    if (ctx.failure()) return { tone: "bad", text: "Could not be opened" };
    if (!s) return { tone: "idle", text: "Opening" };
    if (s.pendingAddons.length) return { tone: "warn live", text: `Asking ${s.pendingAddons.join(", ")}` };
    if (s.slowAddons.length) return { tone: "warn", text: slowWords(s.slowAddons) };
    if (s.failedAddons.length) return { tone: "bad", text: `${s.failedAddons.join(", ")} did not answer` };
    // Blocked: what was found, none of it listed (the card says why).
    if (s.blocked) return { tone: "warn", text: counted("versions", s.blocked.count) };
    return { tone: "ok", text: `${s.rows.length + (s.recommended ? 1 : 0)} versions` };
  };

  return (
    <div class="versions-scr">
      <Backdrop src={ctx.details()?.background} class="versions-bd" dim />
      <header class="vhead">
        {/* Where it sits under its title, wherever the viewer came from
            (Home, Search or Library). */}
        <nav class="crumbs" aria-label="Breadcrumb">
          <span>{ctx.details()?.name ?? "Title"}</span>
          <ChevronRight aria-hidden />
          <span class="here">Versions</span>
        </nav>
        <div class="vhead-row">
          <h1>
            Versions
            <Show when={ctx.label() ?? ctx.details()?.name}>{(l) => <span>for {l()}</span>}</Show>
          </h1>
          <p class="vstate">
            <span class={`st ${progress().tone}`}>
              <i class="dot" />
              {progress().text}
            </span>
            <Show when={snap()?.conditions}>
              {(c) => (
                <span class="device">
                  <MonitorCheck aria-hidden />
                  {conditionsHeader(form(), c())}
                </span>
              )}
            </Show>
          </p>
        </div>
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
                <button class="btn" data-f="versions:back" onClick={goBack}>
                  <ArrowLeft aria-hidden />
                  Back
                </button>
              </div>
            </section>
          )}
        </Match>
        <Match when={snap()?.done && !pick() && (snap()?.rows.length ?? 0) === 0 && !blocked()}>
          <section class="vempty problem">
            <h2>{noVersionWords.title}</h2>
            <p>
              <Show when={(snap()?.hidden.count ?? 0) > 0} fallback={noVersionWords.detail}>
                Every result was hidden: {hiddenSummary(snap()?.hidden ?? { count: 0, byReason: [] })}.
              </Show>
            </p>
            <A class="btn primary" href="/addons" data-f="versions:addons" data-autofocus>
              <Puzzle aria-hidden />
              Addons
            </A>
          </section>
        </Match>
        <Match when={true}>
          <section class="pick" aria-label="Recommended version">
            <Show
              when={pick()}
              fallback={
                <Show
                  when={blocked()}
                  fallback={
                    <div class="pick-wait" aria-busy="true">
                      <p class="kicker">Recommended for this screen</p>
                      <div class="skel" />
                      <div class="skel" />
                      <div class="skel" />
                    </div>
                  }
                >
                  {(b) => {
                    const w = () => blockedWords(b().reason, b().count);
                    return (
                      <div class="blocked">
                        <h2>{noVersionWords.title}</h2>
                        <p>{w().text}</p>
                        <Show
                          when={b().reason === "needsAccount"}
                          fallback={
                            <button class="btn primary" data-f="versions:show" data-autofocus aria-expanded={showBlocked()} onClick={() => setShowBlocked(!showBlocked())}>
                              <Eye aria-hidden />
                              {w().action}
                            </button>
                          }
                        >
                          <div class="acts">
                            <A class="btn primary" href="/debrid" data-f="versions:account" data-autofocus>
                              <KeyRound aria-hidden />
                              {w().action}
                            </A>
                            {/* The versions as they would rank with an account. */}
                            <button class="btn" data-f="versions:show" aria-expanded={showBlocked()} onClick={() => setShowBlocked(!showBlocked())}>
                              <Eye aria-hidden />
                              {choiceWords.blocked.showRows}
                            </button>
                          </div>
                        </Show>
                      </div>
                    );
                  }}
                </Show>
              }
            >
              {(p) => (
                <>
                  <p class="kicker">Recommended for this screen</p>
                  <h2>
                    {qualityText(p().quality)}
                    <Show when={p().features[0]}>{(f) => <> {f()}</>}</Show>
                  </h2>
                  <dl class="facts-grid">
                    <dt>Size</dt>
                    <dd class="num">{p().sizeBytes ? bytes(p().sizeBytes as number) : "Unknown"}</dd>
                    <dt>Status</dt>
                    <dd>
                      <StatusWord row={p()} />
                    </dd>
                    <dt>Via</dt>
                    <dd>{p().via}</dd>
                    <Show when={p().features.length > 1}>
                      <dt>Features</dt>
                      <dd>{p().features.join(", ")}</dd>
                    </Show>
                    <Show when={p().languages.length}>
                      <dt>Languages</dt>
                      <dd>{languages(p())}</dd>
                    </Show>
                  </dl>
                  <ul class="why">
                    <For each={p().reasons.slice(0, 3)}>
                      {(r) => (
                        <li>
                          <Check aria-hidden />
                          {reasonText(r, p().status)}
                        </li>
                      )}
                    </For>
                  </ul>
                  <p class="rel">
                    <Release name={p().releaseName} />
                  </p>
                  <Show when={snap()?.nobodySharing}>
                    <NobodySharing class="nobody" status />
                  </Show>
                  <button class="btn primary" data-f="versions:play" data-autofocus onClick={() => play(p().key)}>
                    <Play aria-hidden class="fill" />
                    Play this version
                  </button>
                  <p class="fail">
                    <RefreshCw aria-hidden />
                    {p().failoverCount > 0
                      ? `${p().failoverCount + 1} sources. If one fails, the next starts on its own.`
                      : "One source for this release."}
                  </p>
                </>
              )}
            </Show>
          </section>

          <section class="alts" aria-label="All versions">
            {/* Once per screen: on the recommended card when there is one. */}
            <Show when={snap()?.nobodySharing && !pick()}>
              <NobodySharing class="lnote nobody" status />
            </Show>
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
            <div class="thead" aria-hidden="true">
              <span>Quality</span>
              <span>Features</span>
              <span>Size</span>
              <span>Status</span>
              <span>Via</span>
            </div>
            <div
              class="alist"
              data-vscroll
              data-fgroup="alts"
              onFocusIn={(e) => {
                const f = (e.target as HTMLElement).dataset.f;
                setFocusedRow(f?.startsWith("v:") ? f.slice(2) : null);
              }}
              onFocusOut={(e) => {
                if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusedRow(null);
              }}
            >
              <For each={groupOrder}>
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
                    <section class="vgroup" data-row>
                      <h3>
                        {groupLabel[g]}
                        <span class="num">{rows().length}</span>
                      </h3>
                      <For each={shown()}>
                        {(r) => (
                          <>
                            <button
                              class="vrow"
                              classList={{ open: opened() === r.key }}
                              data-f={`v:${r.key}`}
                              aria-expanded={form() === "phone" ? opened() === r.key : undefined}
                              onClick={() => tapRow(r)}
                            >
                              <span class="q" classList={{ unknown: r.quality === "Unknown" }}>
                                {qualityCell(r.quality)}
                              </span>
                              <span class="chips">
                                <For each={r.features}>{(f) => <span>{f}</span>}</For>
                              </span>
                              <span class="size num">{r.sizeBytes ? bytes(r.sizeBytes) : ""}</span>
                              <LineWord line={r.line} />
                              <span class="via">{r.via}</span>
                              <span class="more">
                                <Show when={lineWords(r.line).details}>
                                  {(d) => <span class="detail">{d()}</span>}
                                </Show>
                                <span class="rel">
                                  <Release name={r.releaseName} />
                                </span>
                                <span class="sub">{subLine(r)}</span>
                              </span>
                            </button>
                            <Show when={form() === "phone" && opened() === r.key}>
                              <div class="vrow-open">
                                <Show when={lineWords(r.line).details}>
                                  {(d) => <p class="detail">{d()}</p>}
                                </Show>
                                <p class="rel">
                                  <Release name={r.releaseName} />
                                </p>
                                <p class="sub">{subLine(r)}</p>
                                <button class="btn" data-f={`vplay:${r.key}`} onClick={() => play(r.key)}>
                                  <Play aria-hidden class="fill" />
                                  Play this version
                                </button>
                              </div>
                            </Show>
                          </>
                        )}
                      </For>
                      <Show when={rows().length > SHOWN}>
                        <button class="fold" data-f={`fold:${g}`} aria-expanded={expanded()} onClick={() => toggle(g)}>
                          <ChevronDown aria-hidden />
                          {expanded() ? `Show the best ${SHOWN}` : `Show all ${rows().length}`}
                        </button>
                      </Show>
                    </section>
                    </Show>
                  );
                }}
              </For>
              <Show when={(snap()?.hidden.count ?? 0) > 0}>
                <p class="hidden-row">
                  <EyeOff aria-hidden />
                  <b>Hidden ({snap()?.hidden.count})</b>
                  <span>{hiddenSummary(snap()?.hidden ?? { count: 0, byReason: [] })}</span>
                </p>
              </Show>
              <Show when={snap() && !snap()?.done && listed().length === 0}>
                <div class="alist-wait" aria-hidden="true">
                  <For each={[0, 1, 2, 3, 4]}>{() => <div class="skel" />}</For>
                </div>
              </Show>
            </div>
          </section>
        </Match>
      </Switch>
    </div>
  );
}
