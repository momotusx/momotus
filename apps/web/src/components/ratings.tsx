/** The ratings row (design/DECISION.md, Status and ratings; D10): IMDb and Rotten Tomatoes side
 * by side as plain text ("IMDb 8.4 · RT Fresh"), the source's name at 70 percent and the score in
 * white, no star and no logos; a score the core left out has no slot. A screen reader hears each
 * score's spoken twin, never "R T". `sep` is the line's own separator, laid between the scores
 * as their sibling; `imdb` links IMDb's score to its page. */

import { For, type JSX, Show } from "solid-js";
import type { Ratings as Scores } from "../core/generated/Ratings";
import { type RatingPart, ratingParts } from "../lib/format";

const Score = (props: { part: RatingPart }) => (
  <span aria-hidden="true">
    <span class="rate-src">{props.part.source}</span> {props.part.score}
  </span>
);

export function Ratings(props: { ratings: Scores | null | undefined; sep: () => JSX.Element; imdb?: { href: string; f: string } | null }) {
  return (
    <For each={ratingParts(props.ratings)}>
      {(p, i) => (
        <>
          <Show when={i() > 0}>{props.sep()}</Show>
          <Show
            when={p.imdb && props.imdb}
            fallback={
              <span class="rate num">
                <Score part={p} />
                <span class="sr">{p.spoken}</span>
              </span>
            }
          >
            {(link) => (
              <a class="rate num" href={link().href} target="_blank" rel="noopener noreferrer" data-f={link().f} aria-label={`${p.spoken}, on IMDb`}>
                <Score part={p} />
              </a>
            )}
          </Show>
        </>
      )}
    </For>
  );
}
