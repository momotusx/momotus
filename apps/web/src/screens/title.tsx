import { A, useParams } from "@solidjs/router";
import BookmarkCheck from "lucide-solid/icons/bookmark-check";
import BookmarkPlus from "lucide-solid/icons/bookmark-plus";
import Check from "lucide-solid/icons/check";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Layers from "lucide-solid/icons/layers";
import Play from "lucide-solid/icons/play";
import { createEffect, createMemo, For, type JSX, Match, Show, Switch } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { type TitleView, titleView, versionsView } from "../core/client";
import type { Episode } from "../core/generated/Episode";
import type { VersionsSnapshot } from "../core/generated/VersionsSnapshot";
import { remember } from "../focus/dom";
import { bytes, clock, decoded, episodeLabel, pct, playPath, runtimeMinutes, seasonName, versionsPath } from "../lib/format";
import { kindLabel, qualityText, statusText } from "../lib/words";
import "./title.css";

export default function Title() {
  const params = useParams();
  return (
    <Show when={`${params.kind}/${params.id}`} keyed>
      <TitlePage kind={decoded(params.kind)} id={decoded(params.id)} />
    </Show>
  );
}

const released = (e: Episode) => !e.released || new Date(e.released).getTime() <= Date.now();

function TitlePage(props: { kind: string; id: string }) {
  const view = titleView(props.kind, props.id);
  const header = () => view.state.header;
  const d = () => header()?.details ?? null;
  const target = () => header()?.playTarget ?? null;
  const series = () => props.kind !== "movie" && (header()?.seasons.length ?? 0) > 0;

  // Open Versions for the play target as soon as the name is known: the
  // line under Play shows the pick, and the core warms it before Play.
  const versions = createMemo(() => {
    const t = target();
    const name = d()?.name;
    if (!t || !name) return null;
    const ref = /^(.+):(\d+):(\d+)$/.exec(t.videoId);
    return versionsView({
      kind: props.kind,
      videoId: t.videoId,
      title: name,
      season: ref ? Number(ref[2]) : undefined,
      episode: ref ? Number(ref[3]) : undefined,
    });
  });

  const metaLine = () => {
    const x = d();
    if (!x) return [];
    const seasons = header()?.seasons.filter((s) => s > 0).length ?? 0;
    return [
      x.rating ? `${x.rating} IMDb` : null,
      x.releaseInfo,
      x.genres.slice(0, 3).join(", ") || null,
      seasons ? `${seasons} ${seasons === 1 ? "season" : "seasons"}` : null,
      props.kind === "movie" ? x.runtime : null,
    ].filter(Boolean) as string[];
  };

  return (
    <div class="title-scr" classList={{ movie: !series() }}>
      <Backdrop src={d()?.background} class="title-bd" />
      <Switch>
        <Match when={view.state.status.state === "failed" || header()?.state.state === "failed"}>
          <section class="info">
            <div class="problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>This title could not be opened</h2>
              <p>{failure(view)}</p>
              <A class="btn primary" href="/addons" data-f="title:addons" data-autofocus>
                Check addons
              </A>
            </div>
          </section>
        </Match>
        <Match when={d()}>
          {(x) => (
            <>
              <section class="info" aria-label={x().name}>
                <Logo src={x().logo} name={x().name} class="title-logo" />
                <p class="meta">
                  <For each={metaLine()}>
                    {(part, i) => (
                      <>
                        <Show when={i() > 0}>
                          <b aria-hidden="true"> · </b>
                        </Show>
                        <span class="num">{part}</span>
                      </>
                    )}
                  </For>
                </p>
                <Show when={x().description}>
                  <button class="syn" data-f="title:synopsis" aria-label="Synopsis">
                    {x().description}
                  </button>
                </Show>
                <div class="play">
                  <Show when={target()}>
                    {(t) => (
                      <A class="btn primary big" href={playPath(props.kind, props.id, t().videoId)} data-f="title:play" data-autofocus>
                        <Play aria-hidden class="fill" />
                        {t().label}
                        <Show when={t().resumeMs}>
                          {(ms) => <small class="num">from {clock(ms() / 1000)}</small>}
                        </Show>
                      </A>
                    )}
                  </Show>
                  <VersionLine snap={versions()?.state.data ?? null} />
                </div>
                <div class="acts">
                  <Show when={target()}>
                    {(t) => (
                      <A class="btn" href={versionsPath(props.kind, props.id, t().videoId)} data-f="title:versions">
                        <Layers aria-hidden />
                        Versions
                        <Show when={versionCount(versions()?.state.data ?? null)}>
                          {(n) => <span class="num">({n()})</span>}
                        </Show>
                      </A>
                    )}
                  </Show>
                  <button
                    class="btn"
                    data-f="title:library"
                    aria-pressed={header()?.inLibrary ?? false}
                    onClick={() => view.command({ type: "setInLibrary", value: !header()?.inLibrary })}
                  >
                    <Show when={header()?.inLibrary} fallback={<BookmarkPlus aria-hidden />}>
                      <BookmarkCheck aria-hidden />
                    </Show>
                    {header()?.inLibrary ? "In Library" : "Add to Library"}
                  </button>
                </div>
                <dl class="facts">
                  <Fact label="Starring" value={x().cast.slice(0, 3).join(", ")} />
                  <Fact label="Directed by" value={x().directors.slice(0, 2).join(", ")} />
                  <Fact label="Runtime" value={x().runtime ? `${x().runtime}${series() ? " an episode" : ""}` : ""} />
                  <Fact label="Details from" value={x().source} />
                </dl>
              </section>
              <Show when={series()}>
                <Episodes view={view} kind={props.kind} id={props.id} runtime={runtimeMinutes(x().runtime)} />
              </Show>
            </>
          )}
        </Match>
        <Match when={true}>
          <section class="info" aria-busy="true">
            <div class="title-skel" aria-hidden="true">
              <div class="skel" />
              <div class="skel" />
              <div class="skel" />
            </div>
          </section>
        </Match>
      </Switch>
    </div>
  );
}

function failure(view: TitleView): string {
  const s = view.state.status;
  if (s.state === "failed") return s.reason;
  const h = view.state.header?.state;
  return h?.state === "failed" ? h.reason : "";
}

function Fact(props: { label: string; value: string }): JSX.Element {
  return (
    <Show when={props.value}>
      <div>
        <dt class="kicker">{props.label}</dt>
        <dd>{props.value}</dd>
      </div>
    </Show>
  );
}

function versionCount(s: VersionsSnapshot | null): number | null {
  if (!s) return null;
  const n = s.rows.length + (s.recommended ? 1 : 0);
  return n || null;
}

/** The chosen version in words, under Play. */
function VersionLine(props: { snap: VersionsSnapshot | null }) {
  const pick = () => props.snap?.recommended ?? null;
  const waiting = () => props.snap && !props.snap.done && !pick();
  return (
    <Switch fallback={<p class="vline muted">Finding versions</p>}>
      <Match when={pick()}>
        {(p) => {
          const st = () => statusText(p().status);
          return (
            <p class="vline">
              <span>{qualityText(p().quality)}</span>
              <For each={p().features}>{(f) => <span>{f}</span>}</For>
              <Show when={p().sizeBytes}>{(b) => <span class="num">{bytes(b())}</span>}</Show>
              <span class={`st ${st().tone}`}>
                <i class="dot" />
                {st().text}
              </span>
              <span class="muted">via {p().via}</span>
            </p>
          );
        }}
      </Match>
      <Match when={waiting()}>
        <p class="vline muted">
          <span class="st">
            <i class="dot" />
            Asking {props.snap?.pendingAddons.join(", ") || "your addons"} for versions
          </span>
        </p>
      </Match>
      <Match when={props.snap?.done && !pick()}>
        <p class="vline muted">
          <span class="st warn">
            <i class="dot" />
            No playable version yet. Add a source addon in Settings, Addons.
          </span>
        </p>
      </Match>
    </Switch>
  );
}

function Episodes(props: { view: TitleView; kind: string; id: string; runtime: number | null }) {
  const header = () => props.view.state.header;
  const seasons = () => {
    const all = header()?.seasons ?? [];
    return [...all.filter((s) => s > 0), ...all.filter((s) => s === 0)];
  };
  const compact = () => seasons().length > 5;
  createEffect(() => {
    const s = header()?.selectedSeason;
    if (s !== null && s !== undefined) remember("seasons", `season:${s}`);
  });
  const select = (s: number) => props.view.command({ type: "selectSeason", season: s });
  const left = (e: Episode) => {
    if (e.watched) return "Watched";
    if (e.progress !== null) return props.runtime ? `${Math.max(1, Math.round(props.runtime * (1 - e.progress)))} min left` : `${pct(e.progress)} watched`;
    if (!released(e)) return `Airs ${new Date(e.released as string).toLocaleDateString("en", { month: "short", day: "numeric", year: "numeric" })}`;
    return props.runtime ? `${props.runtime} min` : "";
  };
  return (
    <section class="eps" aria-label="Episodes">
      <header class="eps-head">
        <h2>Episodes</h2>
        <div class="seasons" role="tablist" aria-label="Seasons" data-fgroup="seasons" data-hscroll="80" classList={{ compact: compact() }}>
          <Show when={compact()}>
            <span class="kicker">Season</span>
          </Show>
          <For each={seasons()}>
            {(s) => (
              <button
                class="tab"
                role="tab"
                aria-selected={header()?.selectedSeason === s}
                classList={{ on: header()?.selectedSeason === s }}
                data-f={`season:${s}`}
                onClick={() => select(s)}
              >
                {compact() ? (s === 0 ? "Specials" : String(s)) : seasonName(s)}
              </button>
            )}
          </For>
        </div>
      </header>
      <ol class="toc" data-vscroll data-fgroup="episodes">
        <For each={props.view.state.episodes}>
          {(e) => (
            <li class="ep" classList={{ seen: e.watched, now: e.progress !== null, later: !released(e) }}>
              <A class="ep-main" href={playPath(props.kind, props.id, e.videoId)} data-f={`ep:${e.videoId}`}>
                <span class="ep-line">
                  <b class="ep-n num">{e.episode}</b>
                  <span class="ep-t">{e.title || episodeLabel(e.season, e.episode)}</span>
                  <i class="dots" aria-hidden="true" />
                  <span class="ep-r num">
                    <Show when={e.watched}>
                      <Check aria-hidden />
                    </Show>
                    {left(e)}
                  </span>
                </span>
                <Show when={e.overview}>
                  <span class="ep-ov">{e.overview}</span>
                </Show>
                <Show when={e.progress !== null}>
                  <span class="meter">
                    <i style={{ width: pct(e.progress ?? 0) }} />
                  </span>
                </Show>
              </A>
              <button
                class="ep-mark"
                data-f={`mark:${e.videoId}`}
                aria-pressed={e.watched}
                onClick={() => props.view.command({ type: "setWatched", videoId: e.videoId, value: !e.watched })}
              >
                <Check aria-hidden />
                {e.watched ? "Watched" : "Mark watched"}
              </button>
            </li>
          )}
        </For>
      </ol>
    </section>
  );
}
