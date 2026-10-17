import { A, useNavigate, useParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronRight from "lucide-solid/icons/chevron-right";
import EyeOff from "lucide-solid/icons/eye-off";
import MonitorCheck from "lucide-solid/icons/monitor-check";
import Play from "lucide-solid/icons/play";
import Puzzle from "lucide-solid/icons/puzzle";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import { createSignal, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { Backdrop } from "../components/art";
import { displayCaps } from "../core/client";
import type { Group } from "../core/generated/Group";
import type { VersionRow } from "../core/generated/VersionRow";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { videoContext } from "../core/video";
import { bytes, decoded, playPath, releaseParts } from "../lib/format";
import { groupLabel, groupOrder, hiddenSummary, languageName, qualityCell, qualityText, reasonText, statusText } from "../lib/words";
import "./versions.css";

const SHOWN = 3;

export default function Versions() {
  const params = useParams();
  return (
    <Show when={`${params.kind}/${params.id}/${params.videoId}`} keyed>
      <VersionsPage kind={decoded(params.kind)} id={decoded(params.id)} videoId={decoded(params.videoId)} />
    </Show>
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

const languages = (r: VersionRow) => r.languages.map(languageName).join(", ");

function VersionsPage(props: { kind: string; id: string; videoId: string }) {
  const navigate = useNavigate();
  const ctx = videoContext(props.kind, props.id, props.videoId);
  const snap = (): VersionsSnapshot | null => ctx.versions()?.state.data ?? null;
  const pick = () => snap()?.recommended ?? null;
  const [open, setOpen] = createSignal<Set<Group>>(new Set());
  const play = (key: string) => navigate(`${playPath(props.kind, props.id, props.videoId)}?key=${encodeURIComponent(key)}`);

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

  const groups = () => {
    const rows = snap()?.rows ?? [];
    return groupOrder.map((g) => ({ group: g, rows: rows.filter((r) => r.group === g) })).filter((g) => g.rows.length > 0);
  };
  const toggle = (g: Group) => {
    const next = new Set(open());
    if (next.has(g)) next.delete(g);
    else next.add(g);
    setOpen(next);
  };
  const caps = displayCaps();
  const codecNames: Record<string, string> = { h264: "H.264", h265: "HEVC", av1: "AV1", vp9: "VP9" };
  const device = `This screen: ${caps.displayHeight >= 2160 ? "4K" : `${caps.displayHeight}p`}${caps.hdr.length ? ", HDR" : ""}, plays ${caps.decoders.map((d) => codecNames[d] ?? d).join(", ")} in ${caps.containers.map((c) => (c === "webm" ? "WebM" : c.toUpperCase())).join(", ")}`;
  const progress = () => {
    const s = snap();
    if (!s) return { tone: "idle", text: "Opening" };
    if (s.pendingAddons.length) return { tone: "warn live", text: `Asking ${s.pendingAddons.join(", ")}` };
    if (s.slowAddons.length) return { tone: "warn", text: `${s.slowAddons.join(", ")} is slow, still waiting` };
    if (s.failedAddons.length) return { tone: "bad", text: `${s.failedAddons.join(", ")} did not answer` };
    return { tone: "ok", text: `${s.rows.length + (s.recommended ? 1 : 0)} versions` };
  };

  return (
    <div class="versions-scr">
      <Backdrop src={ctx.details()?.background} class="versions-bd" dim />
      <header class="vhead">
        <nav class="crumbs" aria-label="Breadcrumb">
          <span>Home</span>
          <ChevronRight aria-hidden />
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
            <span class="device">
              <MonitorCheck aria-hidden />
              {device}
            </span>
          </p>
        </div>
      </header>

      <Switch>
        <Match when={ctx.versions()?.state.status.state === "failed"}>
          <section class="vempty problem">
            <h2>Versions could not be loaded</h2>
            <p>{(ctx.versions()?.state.status as { reason?: string }).reason}</p>
          </section>
        </Match>
        <Match when={snap()?.done && !pick() && (snap()?.rows.length ?? 0) === 0}>
          <section class="vempty problem">
            <h2>No playable version found</h2>
            <p>
              <Show when={(snap()?.hidden.count ?? 0) > 0} fallback="None of your addons returned a stream for this video. A source addon finds playable streams; add one in Addons.">
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
                <div class="pick-wait" aria-busy="true">
                  <p class="kicker">Recommended for this screen</p>
                  <div class="skel" />
                  <div class="skel" />
                  <div class="skel" />
                </div>
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
            <div class="thead" aria-hidden="true">
              <span>Quality</span>
              <span>Features</span>
              <span>Size</span>
              <span>Status</span>
              <span>Via</span>
            </div>
            <div class="alist" data-vscroll data-fgroup="alts">
              <For each={groups()}>
                {(g) => {
                  const expanded = () => open().has(g.group);
                  const shown = () => (expanded() ? g.rows : g.rows.slice(0, SHOWN));
                  return (
                    <section class="vgroup" data-row>
                      <h3>
                        {groupLabel[g.group]}
                        <span class="num">{g.rows.length}</span>
                      </h3>
                      <For each={shown()}>
                        {(r) => (
                          <button class="vrow" data-f={`v:${r.key}`} onClick={() => play(r.key)}>
                            <span class="q" classList={{ unknown: r.quality === "Unknown" }}>
                              {qualityCell(r.quality)}
                            </span>
                            <span class="chips">
                              <For each={r.features}>{(f) => <span>{f}</span>}</For>
                            </span>
                            <span class="size num">{r.sizeBytes ? bytes(r.sizeBytes) : ""}</span>
                            <StatusWord row={r} />
                            <span class="via">{r.via}</span>
                            <span class="more">
                              <span class="rel">
                                <Release name={r.releaseName} />
                              </span>
                              <span class="sub">
                                {[languages(r) || null, r.failoverCount > 0 ? `${r.failoverCount + 1} sources` : null]
                                  .filter(Boolean)
                                  .join(", ") || "Language not stated"}
                              </span>
                            </span>
                          </button>
                        )}
                      </For>
                      <Show when={g.rows.length > SHOWN}>
                        <button class="fold" data-f={`fold:${g.group}`} aria-expanded={expanded()} onClick={() => toggle(g.group)}>
                          <ChevronDown aria-hidden />
                          {expanded() ? `Show the best ${SHOWN}` : `Show all ${g.rows.length}`}
                        </button>
                      </Show>
                    </section>
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
              <Show when={snap() && !snap()?.done && groups().length === 0}>
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
