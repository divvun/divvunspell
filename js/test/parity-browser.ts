// The parity scenario (parity.ts) in a browser: on the page's main thread, or
// in a module Web Worker the page starts, as the page's query says. The test
// build compiles this to target/js-test/browser, and browser.mjs serves it
// beside the package and the fixtures. The browser posts its report back to
// browser.mjs, which has no other way to learn the outcome.

import { report, runParity } from "./parity.ts";
import type { ParityArgs } from "./parity.ts";

/** What browser.mjs puts in the page's query, as JSON under `args`. */
interface BrowserArgs extends ParityArgs {
  label: string;
  variant: "main-thread" | "worker";
  /** URL to post the report to. */
  report: string;
}

// A worker's location is its script's URL, which the page gives the page's
// query, so both read their arguments the same way.
const args: BrowserArgs = JSON.parse(new URLSearchParams(location.search).get("args") ?? "null");

async function send(lines: string[], passed: boolean): Promise<void> {
  await fetch(args.report, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ lines, passed }),
  });
}

async function fetchBytes(url: string): Promise<Uint8Array> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} answered HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

/** Where this script runs: a worker has no document. */
const place = typeof document === "undefined" ? "worker" : "main-thread";

async function run(): Promise<void> {
  if (place !== args.variant) {
    await send([`not ok - ${args.label}: asked to run as ${args.variant}, it ran as ${place}`], false);
    return;
  }
  try {
    const { lines, passed } = report(args.label, await runParity(args, fetchBytes));
    await send(lines, passed);
  } catch (error) {
    const reason = error instanceof Error ? (error.stack ?? error.message) : String(error);
    await send([`not ok - ${args.label}: the scenario did not run`, reason], false);
  }
}

if (place === "worker" || args.variant === "main-thread") {
  await run();
} else {
  const script = new URL(import.meta.url);
  script.search = location.search;
  const worker = new Worker(script, { type: "module" });
  worker.addEventListener("error", (event) => {
    void send([`not ok - ${args.label}: the worker did not start: ${event.message}`], false);
  });
}
