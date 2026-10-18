import { A } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Info from "lucide-solid/icons/info";
import Play from "lucide-solid/icons/play";
import Puzzle from "lucide-solid/icons/puzzle";
import Star from "lucide-solid/icons/star";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Wrench from "lucide-solid/icons/wrench";
import { createSignal, For, Show } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { CONTINUE_ROW, Row } from "../components/row";
import { homeView } from "../core/client";
import { form } from "../form";
import type { Tile } from "../core/generated/Tile";
import { playPath, titlePath } from "../lib/format";
import { kindLabel } from "../lib/words";
import "./home.css";

const pageScroll = { top: 0 };

/** Where a hero or tile's Play goes: a known video, or the movie itself. */
export function playHref(t: Tile): string | null {
  if (t.videoId) return playPath(t.kind, t.id, t.videoId);
  if (t.kind === "movie") return playPath(t.kind, t.id, t.id);
  return null;
}

export default function Home() {
  const home = homeView();
  const [page, setPage] = createSignal<HTMLElement | null>(null);
  const hasContinue = () => home.rows[0]?.rowId === CONTINUE_ROW;
  const empty = () => home.status.state === "open" && home.rows.length === 0;
  const hero = () => home.hero;

  const attach = (el: HTMLElement) => {
    setPage(el);
    if (pageScroll.top) queueMicrotask(() => el.scrollTo({ top: pageScroll.top }));
  };

  return (
    <div class="home">
      <main class="page" data-vscroll ref={attach} onScroll={(e) => (pageScroll.top = e.currentTarget.scrollTop)}>
        <section class="hero" data-reveal-top aria-label="Featured">
          <Backdrop src={hero()?.tile.background ?? null} class="hero-bd" />
          <Show when={form() === "phone"}>
            <header class="ph-top">
              <span class="brand">Marquee</span>
            </header>
          </Show>
          <div class="hero-body">
          <Show
            when={hero()}
            fallback={
              <Show when={!empty() && home.status.state !== "failed"}>
                <div class="hero-skel" aria-hidden="true">
                  <div class="skel" />
                  <div class="skel" />
                </div>
              </Show>
            }
          >
            {(h) => (
              <>
                <p class="reason eyebrow">{h().reason}</p>
                <Logo src={h().tile.logo} name={h().tile.name} class="hero-logo" />
                <p class="meta">
                  <Show when={h().tile.rating}>
                    <span class="rate num">
                      <Star aria-hidden />
                      {h().tile.rating}
                    </span>
                    <b aria-hidden="true">·</b>
                  </Show>
                  <Show when={h().tile.releaseInfo} fallback={kindLabel(h().tile.kind)}>
                    <span class="num">{h().tile.releaseInfo}</span>
                  </Show>
                  <Show when={h().runtime}>
                    <b aria-hidden="true">·</b>
                    <span class="num">{h().runtime}</span>
                  </Show>
                  <Show when={h().genres.length}>
                    <b aria-hidden="true">·</b>
                    <span>{h().genres.join(", ")}</span>
                  </Show>
                </p>
                <Show when={h().description}>
                  <p class="hero-syn">{h().description}</p>
                </Show>
                <div class="acts">
                  <Show when={playHref(h().tile)}>
                    {(href) => (
                      <A class="btn primary" href={href()} data-f="hero:play" data-autofocus={hasContinue() ? undefined : ""}>
                        <Play aria-hidden class="fill" />
                        Play
                      </A>
                    )}
                  </Show>
                  <A
                    class="rbtn"
                    href={titlePath(h().tile.kind, h().tile.id)}
                    data-f="hero:details"
                    data-autofocus={hasContinue() || playHref(h().tile) ? undefined : ""}
                  >
                    <span class="ring">
                      <Info aria-hidden />
                    </span>
                    <span class="lbl">Details</span>
                  </A>
                </div>
              </>
            )}
          </Show>
          </div>
        </section>

        <Show when={home.status.state === "failed" && home.status}>
          {(s) => (
            <section class="problem home-problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>Marquee cannot reach its core</h2>
              <p>{"reason" in s() ? (s() as { reason: string }).reason : ""}</p>
              <button class="btn primary" data-f="home:retry" data-autofocus onClick={() => location.reload()}>
                <RefreshCw aria-hidden />
                Try again
              </button>
            </section>
          )}
        </Show>

        <Show when={empty()}>
          <section class="problem home-problem">
            <h2>Home fills up from your addons</h2>
            <p>None of the addons you have on shows a catalog here. Add one, or turn rows back on in Settings.</p>
            <div class="acts">
              <A class="btn primary" href="/welcome" data-f="home:setup" data-autofocus>
                <Wrench aria-hidden />
                Set up Marquee
              </A>
              <A class="btn" href="/addons" data-f="home:addons">
                <Puzzle aria-hidden />
                Addons
              </A>
            </div>
          </section>
        </Show>

        <For each={home.rows}>{(row, i) => <Row row={row} autofocus={i() === 0 && row.rowId === CONTINUE_ROW} root={page} />}</For>
      </main>
    </div>
  );
}
