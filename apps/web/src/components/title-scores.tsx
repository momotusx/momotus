/** The title page's ratings row (plans/title-page-ratings-genres-similar.md, design/DECISION.md): each
 * number after its service's own mark, IMDb as x/10 (the addon's score), TMDB and Trakt as percentages.
 * A slot still pending keeps its width so nothing moves when it lands; a slot with no number takes no
 * space. Each item is one element with one spoken name; the marks are hidden from assistive tech. */

import { For, Show } from "solid-js";
import type { TitleScores as Scores } from "../core/generated/TitleScores";
import { type Mark, markSrc } from "./marks";

export type ScoreItem = { mark: Mark; text: string | null; spoken: string; href: string | null };

const NAMES: Record<Mark, string> = { imdb: "IMDb", tmdb: "TMDB", trakt: "Trakt" };
const HOSTS = new Set(["imdb.com", "www.imdb.com", "themoviedb.org", "www.themoviedb.org", "trakt.tv"]);

/** A badge's link, only an https page on IMDb, TMDB or Trakt (review-trakt rev 7 P2); else none. */
export function outLink(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const u = new URL(url);
    return u.protocol === "https:" && HOSTS.has(u.hostname) ? u.href : null;
  } catch {
    return null;
  }
}

/** The row's items in order: IMDb, TMDB, Trakt; `text` null is a pending slot. */
export function scoreItems(imdb: string | null | undefined, scores: Scores | null | undefined, imdbUrl?: string | null): ScoreItem[] {
  const items: ScoreItem[] = [];
  if (imdb) items.push({ mark: "imdb", text: `${imdb}/10`, spoken: `IMDb ${imdb} out of 10`, href: outLink(imdbUrl) });
  const slot = (mark: Mark, s: Scores["tmdb"] | undefined) => {
    if (s?.state === "shown") items.push({ mark, text: s.text, spoken: `${NAMES[mark]} ${s.text.replace("%", " percent")}`, href: outLink(s.url) });
    else if (s?.state === "pending") items.push({ mark, text: null, spoken: "", href: null });
  };
  slot("tmdb", scores?.tmdb);
  slot("trakt", scores?.trakt);
  return items;
}

/** `links` false on TV (no browser there): every badge is plain. A kids profile gets no URLs from the core. */
export function TitleScores(props: { imdb: string | null | undefined; scores: Scores | null | undefined; imdbUrl?: string | null; links: boolean }) {
  const items = () => scoreItems(props.imdb, props.scores, props.imdbUrl);
  const body = (item: ScoreItem) => (
    <>
      <img class="mark" src={markSrc(item.mark)} alt="" aria-hidden="true" />
      <span class="num" aria-hidden="true">{item.text ?? "00%"}</span>
    </>
  );
  return (
    <Show when={items().length}>
      <span class="scores" role="list" aria-label="Ratings">
        <For each={items()}>
          {(item) => (
            <Show
              when={props.links && item.href}
              fallback={
                <span class="score" classList={{ pending: item.text === null }} role="listitem" aria-label={item.spoken || undefined} aria-hidden={item.text === null || undefined}>
                  {body(item)}
                </span>
              }
            >
              {(href) => (
                <a class="score" role="listitem" href={href()} target="_blank" rel="noopener noreferrer" data-f={`title:${item.mark}`} aria-label={`${item.spoken}, on ${NAMES[item.mark]}`}>
                  {body(item)}
                </a>
              )}
            </Show>
          )}
        </For>
      </span>
    </Show>
  );
}
