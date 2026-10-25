import { A, type RouteSectionProps, Route, Router, useLocation, useNavigate } from "@solidjs/router";
import CalendarDays from "lucide-solid/icons/calendar-days";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Compass from "lucide-solid/icons/compass";
import House from "lucide-solid/icons/house";
import LibraryBig from "lucide-solid/icons/library-big";
import SearchIcon from "lucide-solid/icons/search";
import Settings from "lucide-solid/icons/settings";
import { type Component, createEffect, createMemo, createSignal, For, on, onCleanup, Show, untrack } from "solid-js";
import { ProfileButton } from "./components/avatar";
import { bridgeUrl, link } from "./core/client";
// The debrid accounts follow the core from the start, whatever screen is open.
import "./core/debrid";
import "./core/trakt";
// A sign-out with removal that a reload cut short goes on (D13).
import "./core/wipe";
import { gateOpen, profiles, profilesFailed } from "./core/profiles";
import { focusRail, installFocus, move, restoreSoon } from "./focus/dom";
import { form } from "./form";
import { entryOwner, historyBase, historyDepth, markBack, resetHistoryBase, stampEntriesWith, takeAfterBack, takeBack } from "./history";
import Addons from "./screens/addons";
import FindAddons from "./screens/find-addons";
import CalendarScreen from "./screens/calendar";
import Debrid from "./screens/debrid";
import Discover from "./screens/discover";
import Home from "./screens/home";
import HomeRows from "./screens/home-rows";
import StreamChoice from "./screens/stream-choice";
import Import from "./screens/import";
import Library from "./screens/library";
import Onboarding, { RecommendedAddons } from "./screens/onboarding";
import Pairing from "./screens/pairing";
import ProfilesScreen, { EditProfile, ManageProfiles, Picker } from "./screens/profiles";
import Player from "./screens/player";
import Search, { leaveSearch, openSearch, SearchOverlay, searchOpen } from "./screens/search";
import SettingsScreen from "./screens/settings";
import SignInScreen, { Found, NewAccount, OtherWays, OwnServer } from "./screens/sign-in";
import SyncScreen, { AddDevice, ForkReview, MakeKit, RecoverAccount, ReportLost, StartAccount } from "./screens/sync";
import { Stub } from "./screens/stub";
import TraktScreen from "./screens/trakt";
import Title from "./screens/title";
import Versions from "./screens/versions";

type Destination = { path: string; label: string; icon: Component<{ "aria-hidden"?: boolean }>; tool?: boolean };
const destinations: Destination[] = [
  { path: "/", label: "Home", icon: House },
  { path: "/discover", label: "Discover", icon: Compass },
  { path: "/library", label: "Library", icon: LibraryBig },
  { path: "/calendar", label: "Calendar", icon: CalendarDays },
  { path: "/search", label: "Search", icon: SearchIcon, tool: true },
  { path: "/settings", label: "Settings", icon: Settings, tool: true },
];
// Settings sub-pages keep the rail and light up Settings.
const settingsPages = ["/settings", "/addons", "/recommended-addons", "/find-addons", "/import", "/pair", "/home-rows", "/stream-choice", "/debrid", "/sync", "/trakt"];
const isSettingsPage = (path: string) => settingsPages.includes(path) || path.startsWith("/profiles/") || path.startsWith("/sync/");
const section = (path: string) => (isSettingsPage(path) ? "/settings" : path);
const hasNav = (path: string) => destinations.some((d) => d.path === section(path));
// The phone has five tabs: Calendar is a row at the top of Library, lit as Library (design/DECISION.md, Navigation).
const phoneTabs = destinations.filter((d) => d.path !== "/calendar");
const lit = (path: string) => (form() === "phone" && path === "/calendar" ? "/library" : section(path));

/** Where Back goes when there is no in-app history (a deep link or reload). */
export function parentOf(path: string): string | null {
  const parts = path.split("/").filter(Boolean);
  if (parts[0] === "title" && parts[3] === "versions") return `/${parts.slice(0, 3).join("/")}`;
  if (parts[0] === "play" && parts[2]) return `/title/${parts[1]}/${parts[2]}`;
  if (path.startsWith("/profiles/edit/") || path === "/profiles/new") return "/profiles/manage";
  if (path.startsWith("/sync/") || path === "/pair" || path === "/sign-in") return "/sync";
  if (path.startsWith("/sign-in/")) return "/sign-in";
  if (isSettingsPage(path) && path !== "/settings") return "/settings";
  return path === "/" ? null : "/";
}


/** How long the focus rests on a TV tab before its place shows: the androidx.tv TabRow's debounce. Longer than a
 * held arrow's repeat, so a sweep across the bar opens no place on the way. */
const TAB_SETTLE_MS = 250;

/** Search opens as a palette over the screen you are on; its route stays for
 * deep links (and is where the palette lives when you are already there). */
const paletteClick = (path: string) => (e: MouseEvent) => {
  if (path !== "/search" || window.location.pathname === "/search") return;
  e.preventDefault();
  openSearch();
};

/** TV and desktop (design/DECISION.md, Navigation), the wordmark at the left. TV: a solid tab
 * bar centered at the top, every tab with its word (Search and Settings with their icon too),
 * the profile at its end. Desktop: tv.apple.com's black header, text tabs in the middle, Search
 * and the profile at the right. */
function TopNav() {
  const location = useLocation();
  const navigate = useNavigate();
  const tv = () => form() === "tv";
  // TV: a tab that keeps the focus for TAB_SETTLE_MS shows its place, as Apple TV's tab bar does; the focus stays
  // on it. A replace, as a tab bar switches: Back from any place still goes Home (parentOf).
  let settle: ReturnType<typeof setTimeout> | undefined;
  const rest = (path: string) => {
    clearTimeout(settle);
    if (!tv() || section(location.pathname) === path) return;
    settle = setTimeout(() => navigate(path, { replace: true }), TAB_SETTLE_MS);
  };
  onCleanup(() => clearTimeout(settle));
  // TV: Select on the place shown goes into it, as on Apple TV.
  const select = (path: string) => (e: MouseEvent) => {
    if (tv() && section(location.pathname) === path) {
      e.preventDefault();
      move("down");
      return;
    }
    paletteClick(path)(e);
  };
  const link = (d: Destination) => (
    <A
      href={d.path}
      end
      class="nav"
      classList={{ on: section(location.pathname) === d.path }}
      data-f={`nav:${d.path}`}
      on:click={select(d.path)}
      on:focus={() => rest(d.path)}
      on:blur={() => clearTimeout(settle)}
    >
      <Show when={d.tool && tv()}>
        <d.icon aria-hidden />
      </Show>
      <span>{d.label}</span>
    </A>
  );
  // Desktop: Search sits at the right in the header field's place; it opens the palette.
  const search = destinations.find((d) => d.path === "/search") as Destination;
  return (
    <nav class="topnav" aria-label="Main" data-fgroup="rail" inert={searchOpen() || undefined}>
      <span class="brand">Marquee</span>
      <div class="tabbar">
        <For each={tv() ? destinations : destinations.filter((d) => d !== search)}>{link}</For>
        <Show when={tv()}>
          <ProfileButton />
        </Show>
      </div>
      <Show when={!tv()}>
        <div class="topnav-end">
          <A href={search.path} end class="nav nav-field" classList={{ on: section(location.pathname) === search.path }} data-f="nav:/search" on:click={paletteClick(search.path)}>
            <search.icon aria-hidden />
            <span>{search.label}</span>
          </A>
          <ProfileButton />
        </div>
      </Show>
    </nav>
  );
}

/** The phone: the iPhone's floating glass capsule, five tabs, the selected one in the key blue. */
function Tabs() {
  const location = useLocation();
  return (
    <nav class="tabs" aria-label="Main" inert={searchOpen() || undefined}>
      <For each={phoneTabs}>
        {(d) => (
          <A href={d.path} end classList={{ on: lit(location.pathname) === d.path }} on:click={paletteClick(d.path)}>
            <d.icon aria-hidden />
            <span>{d.label}</span>
          </A>
        )}
      </For>
    </nav>
  );
}

let focusInstalled = false;

function Shell(props: RouteSectionProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const back = () => {
    // TV: Back in a place's content puts the focus on its tab first, as Menu does on Apple TV; Back on the bar goes on.
    // Home keeps its own (the platform's exit).
    const place = location.pathname !== "/" && destinations.some((d) => d.path === location.pathname);
    if (form() === "tv" && place && focusRail()) return;
    if (historyDepth() > historyBase()) {
      markBack();
      history.back();
      return;
    }
    const parent = parentOf(location.pathname);
    if (!parent) return; // Home is the root: the platform shell owns exit.
    markBack();
    navigate(parent, { replace: true });
  };
  if (!focusInstalled) {
    focusInstalled = true;
    installFocus(back);
  }
  // A route belongs to the profile its history entry was opened under
  // (`entryOwner`, written with the entry itself). An entry of another profile
  // never renders: after a switch or a reload when another tab switched, Home
  // opens as a new entry above the old ones; reached through the browser's own
  // Back or Forward, Home replaces it. Either way the app's Back stops there.
  // The session's profile, locked or not: an entry opened behind the locked
  // picker belongs to the profile that was locked, so unlocking it opens the
  // asked page and choosing another opens Home. Entries of another profile
  // reached with the browser's Back or Forward become Home in history.ts.
  const session = () => profiles()?.active;
  stampEntriesWith(session, gateOpen);
  const foreign = createMemo(() => {
    const active = session();
    const owner = entryOwner();
    return gateOpen() && !!active && !!owner && owner !== active;
  });
  // The session changed (a switch, another profile after the locked picker,
  // a reload after another tab switched): Home opens as a new entry above the
  // old profile's ones. No navigation is under way here, so history is settled.
  createEffect(
    on([session, gateOpen], ([active, isOpen]) => {
      if (!active) return;
      const owner = untrack(entryOwner);
      // A fresh tab's first entry, or one written before the core said who runs.
      if (!owner) return void history.replaceState(history.state, "");
      if (!isOpen || owner === active) return;
      if (window.location.pathname === "/") {
        resetHistoryBase();
        history.replaceState(history.state, "");
      } else {
        resetHistoryBase(historyDepth() + 1);
        navigate("/");
      }
    }),
  );
  // A page scrolled away from its top slides under the floating pill: the
  // scrim behind the pill shows then. Scroll does not bubble, so listen in
  // the capture phase; a new page starts at its top until it scrolls.
  const [scrolled, setScrolled] = createSignal(false);
  const watchScroll = (app: HTMLDivElement) =>
    app.addEventListener(
      "scroll",
      (e) => {
        const page = e.target;
        if (page instanceof HTMLElement && page.hasAttribute("data-vscroll")) setScrolled(page.scrollTop > 0);
      },
      { capture: true, passive: true },
    );
  createEffect(
    on(
      () => location.pathname,
      () => {
        const then = takeAfterBack();
        if (then) return navigate(then, { replace: true });
        setScrolled(false);
        // The tab and history name the screen; a title, Versions and the
        // player name their title themselves once it is known.
        const path = location.pathname;
        // Full screen is for the play: the next episode or version keeps it,
        // any other screen ends it.
        if (!path.startsWith("/play/") && document.fullscreenElement) void document.exitFullscreen().catch(() => undefined);
        if (!path.startsWith("/title/") && !path.startsWith("/play/")) {
          const named = destinations.find((d) => d.path === section(path));
          document.title = named ? `${named.label} · Marquee` : "Marquee";
        }
        // A new screen (a result was opened, or Back) leaves the palette.
        leaveSearch(location.pathname + location.search);
        const wasBack = takeBack();
        // Touch has no cursor to place; remotes and keyboards do.
        if (form() !== "phone") restoreSoon(wasBack);
      },
    ),
  );
  const nav = () => hasNav(location.pathname);
  // Nothing of a profile shows before the core says which one runs and
  // whether it is locked (plans/m11-profiles.md, P1 apps v2 5).
  return (
    <div class="app" ref={watchScroll}>
      {/* A lost core takes the profile with it: nothing shows until a reload starts the next one. */}
      <Show when={link() !== "closed"}>
        <Show
          when={profiles()}
          fallback={
            <Show when={profilesFailed()} fallback={<div class="boot brand" role="status" aria-label="Marquee is starting">Marquee</div>}>
              {(r) => <p class="banner bad">{r()}</p>}
            </Show>
          }
        >
          <Show when={gateOpen()} fallback={<Picker mode={profiles()?.locked ? "locked" : "start"} />}>
            <Show when={!foreign()}>
            <Show when={nav() && form() !== "phone"}>
              <div class="nav-scrim" classList={{ on: scrolled() }} aria-hidden="true" />
              <TopNav />
            </Show>
            <div class="screen" inert={searchOpen() || undefined}>
              {props.children}
            </div>
            <Show when={nav() && form() === "phone"}>
              <Tabs />
            </Show>
            <Show when={searchOpen()}>
              <SearchOverlay />
            </Show>
            </Show>
          </Show>
        </Show>
      </Show>
      <Show when={link() === "closed"}>
        <p class="banner bad" role="status">
          <CircleAlert aria-hidden />
          Lost connection to the core at {bridgeUrl}. Restart it, then reload.
        </p>
      </Show>
    </div>
  );
}

const NotFound = () => <Stub title="Not found" text="This page does not exist in Marquee." />;

export function App() {
  return (
    <Router root={Shell}>
      <Route path="/" component={Home} />
      <Route path="/title/:kind/:id" component={Title} />
      <Route path="/title/:kind/:id/versions/:videoId" component={Versions} />
      <Route path="/play/:kind/:id/:videoId" component={Player} />
      <Route path="/welcome" component={Onboarding} />
      <Route path="/sign-in" component={SignInScreen} />
      <Route path="/sign-in/other" component={OtherWays} />
      <Route path="/sign-in/server" component={OwnServer} />
      <Route path="/sign-in/new" component={NewAccount} />
      <Route path="/sign-in/found" component={Found} />
      <Route path="/settings" component={SettingsScreen} />
      <Route path="/search" component={Search} />
      <Route path="/discover" component={Discover} />
      <Route path="/library" component={Library} />
      <Route path="/calendar" component={CalendarScreen} />
      <Route path="/addons" component={Addons} />
      <Route path="/recommended-addons" component={RecommendedAddons} />
      <Route path="/find-addons" component={FindAddons} />
      <Route path="/import" component={Import} />
      <Route path="/pair" component={Pairing} />
      <Route path="/sync" component={SyncScreen} />
      <Route path="/sync/start" component={StartAccount} />
      <Route path="/sync/recover" component={RecoverAccount} />
      {/* The web approves no device (plans/accounts.md 4.2): "Use your phone to sign in other
          devices". The approve page stays in development builds, for the e2e peers. */}
      <Route path="/sync/add" component={import.meta.env.DEV ? AddDevice : NotFound} />
      <Route path="/sync/kit" component={MakeKit} />
      <Route path="/sync/lost/:id" component={ReportLost} />
      <Route path="/sync/fork" component={ForkReview} />
      <Route path="/home-rows" component={HomeRows} />
      <Route path="/stream-choice" component={StreamChoice} />
      <Route path="/debrid" component={Debrid} />
      <Route path="/trakt" component={TraktScreen} />
      <Route path="/profiles" component={ProfilesScreen} />
      <Route path="/profiles/manage" component={ManageProfiles} />
      <Route path="/profiles/new" component={EditProfile} />
      <Route path="/profiles/edit/:id" component={EditProfile} />
      <Route path="*" component={NotFound} />
    </Router>
  );
}
