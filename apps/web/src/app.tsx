import { A, type RouteSectionProps, Route, Router, useLocation, useNavigate } from "@solidjs/router";
import CalendarDays from "lucide-solid/icons/calendar-days";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Compass from "lucide-solid/icons/compass";
import House from "lucide-solid/icons/house";
import LibraryBig from "lucide-solid/icons/library-big";
import SearchIcon from "lucide-solid/icons/search";
import Settings from "lucide-solid/icons/settings";
import { type Component, createEffect, createMemo, For, on, Show, untrack } from "solid-js";
import { AvatarArt } from "./components/avatar";
import { bridgeUrl, link } from "./core/client";
// The debrid accounts follow the core from the start, whatever screen is open.
import "./core/debrid";
import "./core/trakt";
import { gateOpen, profiles, profilesFailed } from "./core/profiles";
import { installFocus, restore } from "./focus/dom";
import { form } from "./form";
import { entryOwner, historyBase, historyDepth, resetHistoryBase, stampEntriesWith } from "./history";
import Addons from "./screens/addons";
import FindAddons from "./screens/find-addons";
import CalendarScreen from "./screens/calendar";
import Debrid from "./screens/debrid";
import Discover from "./screens/discover";
import Home from "./screens/home";
import HomeRows from "./screens/home-rows";
import Import from "./screens/import";
import Library from "./screens/library";
import Onboarding from "./screens/onboarding";
import Pairing from "./screens/pairing";
import ProfilesScreen, { EditProfile, ManageProfiles, Picker } from "./screens/profiles";
import Player from "./screens/player";
import Search from "./screens/search";
import SettingsScreen from "./screens/settings";
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
const settingsPages = ["/settings", "/addons", "/find-addons", "/import", "/pair", "/home-rows", "/debrid", "/sync", "/trakt"];
const isSettingsPage = (path: string) => settingsPages.includes(path) || path.startsWith("/profiles/") || path.startsWith("/sync/");
const section = (path: string) => (isSettingsPage(path) ? "/settings" : path);
const hasNav = (path: string) => destinations.some((d) => d.path === section(path));

/** Where Back goes when there is no in-app history (a deep link or reload). */
export function parentOf(path: string): string | null {
  const parts = path.split("/").filter(Boolean);
  if (parts[0] === "title" && parts[3] === "versions") return `/${parts.slice(0, 3).join("/")}`;
  if (parts[0] === "play" && parts[2]) return `/title/${parts[1]}/${parts[2]}`;
  if (path.startsWith("/profiles/edit/") || path === "/profiles/new") return "/profiles/manage";
  if (path.startsWith("/sync/") || path === "/pair") return "/sync";
  if (isSettingsPage(path) && path !== "/settings") return "/settings";
  return path === "/" ? null : "/";
}


let wentBack = false;
export const markBack = () => {
  wentBack = true;
};

/** TV and desktop: a floating glass pill at the top. Sections are words;
 * Search and Settings, after a divider, carry their icon. */
function TopNav() {
  const location = useLocation();
  const link = (d: Destination) => (
    <A href={d.path} end class="nav" classList={{ on: section(location.pathname) === d.path }} data-f={`nav:${d.path}`}>
      <Show when={d.tool}>
        <d.icon aria-hidden />
      </Show>
      <span>{d.label}</span>
    </A>
  );
  return (
    <nav class="topnav glass" aria-label="Main" data-fgroup="rail">
      <span class="brand">Marquee</span>
      <For each={destinations.filter((d) => !d.tool)}>{link}</For>
      <span class="sep" aria-hidden="true" />
      <For each={destinations.filter((d) => d.tool)}>{link}</For>
      <ProfileButton />
    </nav>
  );
}

/** The active profile's avatar at the pill's end: it opens the picker. */
function ProfileButton() {
  const me = () => profiles()?.profiles.find((p) => p.id === profiles()?.active);
  return (
    <Show when={me()}>
      {(p) => (
        <A href="/profiles" class="nav pavatar" data-f="nav:/profiles" aria-label={`Profile: ${p().name}, switch profile`}>
          <AvatarArt avatar={p().avatar} name={p().name} size="s" />
        </A>
      )}
    </Show>
  );
}

function Tabs() {
  const location = useLocation();
  return (
    <nav class="tabs glass strong" aria-label="Main">
      <For each={destinations}>
        {(d) => (
          <A href={d.path} end classList={{ on: section(location.pathname) === d.path }}>
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
  createEffect(
    on(
      () => location.pathname,
      () => {
        const wasBack = wentBack;
        wentBack = false;
        // Touch has no cursor to place; remotes and keyboards do.
        if (form() !== "phone") requestAnimationFrame(() => restore(wasBack));
      },
    ),
  );
  const nav = () => hasNav(location.pathname);
  // Nothing of a profile shows before the core says which one runs and
  // whether it is locked (plans/m11-profiles.md, P1 apps v2 5).
  return (
    <div class="app">
      {/* A lost core takes the profile with it: nothing shows until a reload starts the next one. */}
      <Show when={link() !== "closed"}>
        <Show when={profiles()} fallback={<Show when={profilesFailed()}>{(r) => <p class="banner bad">{r()}</p>}</Show>}>
          <Show when={gateOpen()} fallback={<Picker mode={profiles()?.locked ? "locked" : "start"} />}>
            <Show when={!foreign()}>
            <Show when={nav() && form() !== "phone"}>
              <TopNav />
            </Show>
            <div class="screen">{props.children}</div>
            <Show when={nav() && form() === "phone"}>
              <Tabs />
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

export function App() {
  return (
    <Router root={Shell}>
      <Route path="/" component={Home} />
      <Route path="/title/:kind/:id" component={Title} />
      <Route path="/title/:kind/:id/versions/:videoId" component={Versions} />
      <Route path="/play/:kind/:id/:videoId" component={Player} />
      <Route path="/welcome" component={Onboarding} />
      <Route path="/settings" component={SettingsScreen} />
      <Route path="/search" component={Search} />
      <Route path="/discover" component={Discover} />
      <Route path="/library" component={Library} />
      <Route path="/calendar" component={CalendarScreen} />
      <Route path="/addons" component={Addons} />
      <Route path="/find-addons" component={FindAddons} />
      <Route path="/import" component={Import} />
      <Route path="/pair" component={Pairing} />
      <Route path="/sync" component={SyncScreen} />
      <Route path="/sync/start" component={StartAccount} />
      <Route path="/sync/recover" component={RecoverAccount} />
      <Route path="/sync/add" component={AddDevice} />
      <Route path="/sync/kit" component={MakeKit} />
      <Route path="/sync/lost/:id" component={ReportLost} />
      <Route path="/sync/fork" component={ForkReview} />
      <Route path="/home-rows" component={HomeRows} />
      <Route path="/debrid" component={Debrid} />
      <Route path="/trakt" component={TraktScreen} />
      <Route path="/profiles" component={ProfilesScreen} />
      <Route path="/profiles/manage" component={ManageProfiles} />
      <Route path="/profiles/new" component={EditProfile} />
      <Route path="/profiles/edit/:id" component={EditProfile} />
      <Route path="*" component={() => <Stub title="Not found" text="This page does not exist in Marquee." />} />
    </Router>
  );
}
