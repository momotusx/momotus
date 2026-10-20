/** Loaded before every unit test (bunfig.toml): Solid's reactive build, as
 * the page runs it. Bun would pick Solid's server build, whose effects never
 * run, so a module that reacts to a signal could not be tested as it works. */

import { mock } from "bun:test";

const reactive = "../node_modules/solid-js/dist/solid.js";
const solid = (await import(reactive)) as typeof import("solid-js");
mock.module("solid-js", () => solid);
// The store too: the server build's reconcile replaces in place of merging.
const reactiveStore = "../node_modules/solid-js/store/dist/store.js";
const store = (await import(reactiveStore)) as typeof import("solid-js/store");
mock.module("solid-js/store", () => store);
