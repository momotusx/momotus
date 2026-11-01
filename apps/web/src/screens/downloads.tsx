/** This device's downloads (plans/offline-downloads.md 3.4), reached from Library: the storage line
 * over its bar, what is under way with Pause all, then the kept titles, newest first. A film's row
 * plays it; a series' row opens its page, the episodes by season, with Delete all. A kids profile
 * sees its allowed titles with Play only. */

import { useNavigate, useParams } from "@solidjs/router";
import ArrowDownToLine from "lucide-solid/icons/arrow-down-to-line";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Trash from "lucide-solid/icons/trash";
import { createEffect, createSignal, For, type JSX, Match, Show, Switch } from "solid-js";
import { awayFromTv, controlAria, downloadAct, DownloadMark, DownloadMenu, itemEntries, kidsProfile, type MenuAsk } from "../components/download";
import { Why } from "../components/why";
import { liveView } from "../core/client";
import type { DownloadRow } from "../core/generated/DownloadRow";
import type { DownloadsSnapshot } from "../core/generated/DownloadsSnapshot";
import type { DownloadStorage } from "../core/generated/DownloadStorage";
import { decoded, episodeLabel, playPath, seasonName, versionsPath } from "../lib/format";
import { badgeOf, deleteSeason, downloadControl, type DownloadItem, downloadWords, episodeLine, episodeName, progressLine, shownDownloads, storageLine, subtitlesLine, titleLine } from "../lib/download";
import "./browse.css";
import "./settings.css";
import { setTitle } from "../incognito";
import { RouteA } from "../components/route-link";

export const DOWNLOADS = "/library/downloads";
/** After an inline confirmation opens or closes, the focus goes to its other side. */
export const focusOn = (key: string) => queueMicrotask(() => document.querySelector<HTMLElement>(`[data-f="${key}"]`)?.focus());
export const downloadedSeriesPath = (itemId: string) => `${DOWNLOADS}/${encodeURIComponent(itemId)}`;

/** "Downloads use 12.4 GB · 48 GB free on this device" over a thin bar: downloads white, free space the track. */
export function StorageLine(props: { storage: DownloadStorage }) {
  // ponytail: the browser knows no disk total, so the bar is downloads against downloads plus free space;
  // the "other used" segment comes when the core sends the disk's size.
  const share = () => {
    const free = props.storage.freeBytes;
    const used = props.storage.downloadsBytes;
    return free === null || used + free === 0 ? null : used / (used + free);
  };
  return (
    <div class="dl-storage">
      <p class="dl-storage-line num">{storageLine(props.storage)}</p>
      <Show when={share() !== null}>
        <span class="dl-bar" aria-hidden="true">
          <i style={{ width: `${Math.max(share() ?? 0, (share() ?? 0) > 0 ? 0.005 : 0) * 100}%` }} />
        </span>
      </Show>
    </div>
  );
}

/** What a row's control and its menu do: the ops, Play, and a version to download. */
function useRowMenu() {
  const navigate = useNavigate();
  const [menu, setMenu] = createSignal<MenuAsk | null>(null);
  const go = (r: DownloadRow) => (item: DownloadItem) => {
    if (item === "play") navigate(playPath(r.kind, r.itemId, r.videoId));
    else if (item === "another" || item === "choose") navigate(`${versionsPath(r.kind, r.itemId, r.videoId)}?purpose=download`);
  };
  const open = (r: DownloadRow, from: string) => {
    const badge = badgeOf(r);
    const c = downloadControl(badge);
    const title = r.episode === null ? r.name : `${r.name} ${episodeLabel(r.season, r.episode)}`;
    setMenu({ title, detail: c.detail, entries: itemEntries(c.items, badge, go(r)), from });
  };
  return { menu, setMenu, open };
}

/** A Settings list row (V2 rollout section 8): the art and two lines play it (or open the series), the
 * control and the trash button trail it. */
function DlRow(props: { fkey: string; href: string; art: string | null; wide?: boolean; title: string; line: string; extra?: string | null; end?: JSX.Element; label?: string }) {
  const [broken, setBroken] = createSignal(false);
  return (
    <div class="dlrow" data-row>
      <RouteA class="dlrow-main" href={props.href} data-f={props.fkey} aria-label={props.label}>
        <span class="dlrow-art" classList={{ wide: props.wide }} aria-hidden="true">
          <Show when={!broken() && props.art}>{(src) => <img src={src()} alt="" loading="lazy" decoding="async" onError={() => setBroken(true)} />}</Show>
        </span>
        <span class="dlrow-text">
          <b>{props.title}</b>
          <small class="num">{props.line}</small>
          <Show when={props.extra}>{(x) => <small class="dlrow-subs">{x()}</small>}</Show>
        </span>
      </RouteA>
      <Show when={props.end}>
        <span class="dlrow-acts">{props.end}</span>
      </Show>
    </div>
  );
}

function RowControl(props: { row: DownloadRow; onOpen: (r: DownloadRow, from: string) => void }) {
  const c = () => downloadControl(badgeOf(props.row));
  const key = `dlc:${props.row.id}`;
  return (
    <button class="dl-btn" data-f={key} {...controlAria(c(), rowName(props.row))} onClick={() => props.onOpen(props.row, key)}>
      <DownloadMark control={c()} />
    </button>
  );
}

function TrashButton(props: { row: DownloadRow; name: string }) {
  return (
    <button class="dl-btn" data-f={`dlx:${props.row.id}`} aria-label={`Delete ${props.name}`} onClick={() => downloadAct("downloadRemove", { ids: [props.row.id] })}>
      <Trash aria-hidden />
    </button>
  );
}

const rowName = (r: DownloadRow) => (r.episode === null ? r.name : `${r.name} · ${episodeLabel(r.season, r.episode)}`);

export default function Downloads() {
  const view = liveView<DownloadsSnapshot>("downloads");
  const s = () => shownDownloads(view.state.data);
  const kids = kidsProfile;
  awayFromTv();
  const { menu, setMenu, open } = useRowMenu();
  const empty = () => !!s() && s()?.downloading.length === 0 && s()?.titles.length === 0;
  const allPaused = () => s()?.downloading.every((r) => r.state === "paused") ?? false;
  return (
    <>
      <Show when={menu()}>{(m) => <DownloadMenu ask={m()} onClose={() => setMenu(null)} />}</Show>
      <main class="browse page settings-page column downloads-page" data-vscroll inert={!!menu() || undefined}>
        <h1>Downloads</h1>
        <Switch>
          <Match when={view.state.status.state === "failed"}>
            <section class="problem">
              <h2>Downloads could not open</h2>
              <Why reason={(view.state.status as { reason: string }).reason} raw={(view.state.status as { raw?: string }).raw} />
            </section>
          </Match>
          <Match when={s()}>
            {(snap) => (
              <>
                <Show when={!kids()}>
                  <StorageLine storage={snap().storage} />
                </Show>
                <Show when={empty()}>
                  <section class="problem dl-empty">
                    <ArrowDownToLine aria-hidden class="problem-icon" />
                    <h2>{downloadWords.emptyTitle}</h2>
                    <p>{downloadWords.emptyBody}</p>
                  </section>
                </Show>
                <Show when={snap().downloading.length > 0}>
                  <section class="set dl-sec" aria-labelledby="dl-running">
                    <header class="dl-sec-head">
                      <h2 id="dl-running">Downloading</h2>
                      <Show when={!kids()}>
                        <button class="btn" data-f="downloads:all" onClick={() => downloadAct(allPaused() ? "downloadResume" : "downloadPause", { all: true })}>
                          {allPaused() ? "Resume all" : "Pause all"}
                        </button>
                      </Show>
                    </header>
                    <div class="links" data-fgroup="dl-running">
                      <For each={snap().downloading}>
                        {(r) => (
                          <DlRow
                            fkey={`dl:${r.id}`}
                            href={playPath(r.kind, r.itemId, r.videoId)}
                            art={r.still ?? r.poster}
                            wide={!!r.still}
                            title={rowName(r)}
                            label={`Play ${rowName(r)}`}
                            line={progressLine(r)}
                            end={kids() ? undefined : <RowControl row={r} onOpen={open} />}
                          />
                        )}
                      </For>
                    </div>
                  </section>
                </Show>
                <Show when={snap().titles.length > 0}>
                  <section class="set dl-sec" aria-labelledby="dl-kept">
                    <header class="dl-sec-head">
                      <h2 id="dl-kept">Downloaded</h2>
                    </header>
                    <div class="links" data-fgroup="dl-kept">
                      <For each={snap().titles}>
                        {(t) => (
                          <Show
                            when={t.kind === "movie" && t.rows[0]}
                            fallback={
                              <DlRow
                                fkey={`dlt:${t.itemId}`}
                                href={downloadedSeriesPath(t.itemId)}
                                art={t.poster}
                                title={t.name}
                                line={titleLine(t)}
                                end={<ChevronRight aria-hidden class="chev" />}
                              />
                            }
                          >
                            {(r) => (
                              <DlRow
                                fkey={`dl:${r().id}`}
                                href={playPath(r().kind, r().itemId, r().videoId)}
                                art={t.poster}
                                title={t.name}
                                label={`Play ${t.name}`}
                                line={titleLine(t)}
                                extra={subtitlesLine(r().subtitles)}
                                end={
                                  kids() ? undefined : (
                                    <>
                                      <RowControl row={r()} onOpen={open} />
                                      <TrashButton row={r()} name={t.name} />
                                    </>
                                  )
                                }
                              />
                            )}
                          </Show>
                        )}
                      </For>
                    </div>
                  </section>
                </Show>
              </>
            )}
          </Match>
        </Switch>
      </main>
    </>
  );
}

/** One series' downloaded episodes by season, with Delete all and its inline confirmation. */
export function DownloadedSeries() {
  const params = useParams();
  const itemId = () => decoded(params.itemId);
  const view = liveView<DownloadsSnapshot>("downloads");
  const navigate = useNavigate();
  const kids = kidsProfile;
  awayFromTv();
  const { menu, setMenu, open } = useRowMenu();
  const [confirm, setConfirm] = createSignal(false);
  const shown = () => shownDownloads(view.state.data);
  const title = () => shown()?.titles.find((t) => t.itemId === itemId()) ?? null;
  const rows = () => [...(title()?.rows ?? []), ...(shown()?.downloading.filter((r) => r.itemId === itemId()) ?? [])];
  const name = () => title()?.name ?? rows()[0]?.name ?? "";
  // Counts what is downloaded, not the queued and running ones it also cancels (code review r3 m4).
  const ask = () => deleteSeason(null, rows().map(badgeOf), name());
  const seasons = () => {
    const by = new Map<number, DownloadRow[]>();
    for (const r of [...rows()].sort((a, b) => (a.season ?? 0) - (b.season ?? 0) || (a.episode ?? 0) - (b.episode ?? 0))) {
      const s = r.season ?? 0;
      by.set(s, [...(by.get(s) ?? []), r]);
    }
    return [...by.entries()];
  };
  // Nothing of it left (the last episode deleted): back to Downloads.
  createEffect(() => {
    if (shown() && rows().length === 0) navigate(DOWNLOADS, { replace: true });
  });
  createEffect(() => {
    if (name()) setTitle(`${name()} · Downloads · Momotus`);
  });
  return (
    <>
      <Show when={menu()}>{(m) => <DownloadMenu ask={m()} onClose={() => setMenu(null)} />}</Show>
      <main class="browse page settings-page column downloads-page" data-vscroll inert={!!menu() || undefined}>
        <h1>{name()}</h1>
        <Show when={!kids() && rows().length > 0}>
          <div class="dl-confirm" role="group" aria-label={confirm() ? ask() : undefined}>
            <Show
              when={confirm()}
              fallback={
                <button
                  class="btn"
                  data-f="downloads:delete-all"
                  onClick={() => {
                    setConfirm(true);
                    focusOn("downloads:delete-all-cancel");
                  }}
                >
                  <Trash aria-hidden />
                  Delete all
                </button>
              }
            >
              {/* DECISION's Confirmation pattern, inline: no destructive red, Cancel beside it. */}
              <p>{ask()}</p>
              <button
                class="btn"
                data-f="downloads:delete-all-confirm"
                onClick={() => {
                  setConfirm(false);
                  downloadAct("downloadRemove", { itemId: itemId() });
                }}
              >
                Delete
              </button>
              <button
                class="btn"
                data-f="downloads:delete-all-cancel"
                onClick={() => {
                  setConfirm(false);
                  focusOn("downloads:delete-all");
                }}
              >
                Cancel
              </button>
            </Show>
          </div>
        </Show>
        <For each={seasons()}>
          {([season, eps]) => (
            <section class="set dl-sec" aria-labelledby={`dl-season-${season}`}>
              <header class="dl-sec-head">
                <h2 id={`dl-season-${season}`}>{seasonName(season)}</h2>
              </header>
              <div class="links" data-fgroup={`dl-season-${season}`}>
                <For each={eps}>
                  {(r) => (
                    <DlRow
                      fkey={`dl:${r.id}`}
                      href={playPath(r.kind, r.itemId, r.videoId)}
                      art={r.still ?? r.poster}
                      wide={!!r.still}
                      title={episodeName(r)}
                      label={`Play ${episodeName(r)}`}
                      line={r.state === "done" ? episodeLine(r) : progressLine(r)}
                      extra={subtitlesLine(r.subtitles)}
                      end={
                        kids() ? undefined : (
                          <>
                            <RowControl row={r} onOpen={open} />
                            <TrashButton row={r} name={episodeName(r)} />
                          </>
                        )
                      }
                    />
                  )}
                </For>
              </div>
            </section>
          )}
        </For>
      </main>
    </>
  );
}
