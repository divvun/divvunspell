// The parity scenario's runs beyond run.mjs's own runtimes: under Deno, and
// in a headless Chromium, on a page and in a Web Worker. Node and Bun run
// parity-runtime.ts from run.mjs's scenario list. A runtime that is not
// installed is skipped with a message, so a machine without Deno or Chromium
// still runs the rest; --require-all, or FST_JS_TEST_REQUIRE_ALL=1, makes
// every one of the four required, and a skip a failure.

import { spawn, spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { findBrowser, runInBrowser } from "./browser.mjs";

const here = dirname(fileURLToPath(import.meta.url));

/** The runtimes only the parity scenario runs in. */
export const PARITY_ONLY = ["deno", "chromium"];

/** The environments native-parity names: Node, Bun, Deno and a browser engine. */
const ALL = ["node", "bun", ...PARITY_ONLY];

export function requireAll(argv, env = process.env) {
  return argv.includes("--require-all") || env.FST_JS_TEST_REQUIRE_ALL === "1";
}

function hasDeno() {
  const probe = spawnSync("deno", ["--version"], { stdio: "ignore" });
  return probe.error === undefined && probe.status === 0;
}

/**
 * Runs parity-runtime.ts under Deno with read access to `readable` and
 * nothing else: the scenario reads the package and its .wasm file, and the
 * fixtures, and needs no network, environment or write access. With no
 * config file Deno reads no import map and writes no lock file.
 */
function runDeno(args, readable) {
  if (readable.some((path) => path.includes(","))) {
    throw new Error(`deno takes --allow-read as a comma-separated list, so it cannot name ${readable.join(" or ")}`);
  }
  const flags = ["run", "--no-config", "--no-prompt", `--allow-read=${readable.join(",")}`];
  const entry = join(here, "parity-runtime.ts");
  return new Promise((resolve) => {
    const child = spawn("deno", [...flags, entry, JSON.stringify(args)], { stdio: ["ignore", "inherit", "inherit"] });
    child.on("error", (error) => {
      console.log(`not ok - deno ${args.label}: cannot start deno: ${error.message}`);
      resolve(false);
    });
    child.on("close", (code) => resolve(code === 0));
  });
}

/**
 * Runs the parity scenario in each of PARITY_ONLY that `runtimes` names,
 * against the package `pkg` (as run.mjs locates it), with the fixtures and
 * compiled scenario under `testBuild`. When `required`, a runtime of the four
 * that `runtimes` leaves out, or that is not installed, fails the run.
 * Returns how many runs there were and how many failed.
 */
export async function runParityElsewhere({ runtimes, required, pkg, testBuild }) {
  const fixtures = join(testBuild, "fixtures");
  const packageDir = dirname(pkg.release.module);
  let runs = 0;
  let failures = 0;
  const unavailable = (runtime, label, why) => {
    if (required) {
      console.log(`not ok - ${runtime} ${label}: ${why}, and every runtime is required`);
      runs += 1;
      failures += 1;
    } else {
      console.log(`ok - ${runtime} ${label} # SKIP ${why} (--require-all makes this a failure)`);
    }
  };
  for (const runtime of ALL.filter((name) => !runtimes.includes(name))) {
    unavailable(runtime, `${pkg.name} parity`, "--runtime leaves it out");
  }

  if (runtimes.includes("deno")) {
    const label = `${pkg.name} parity-runtime.ts main`;
    if (hasDeno()) {
      const args = {
        module: pathToFileURL(pkg.release.module).href,
        fixtures: `${pathToFileURL(fixtures).href}/`,
        label,
        required,
      };
      runs += 1;
      if (!(await runDeno(args, [packageDir, fixtures]))) failures += 1;
    } else {
      unavailable("deno", label, "deno is not on PATH");
    }
  }

  if (runtimes.includes("chromium")) {
    const variants = ["main-thread", "worker"];
    const label = (variant) => `${pkg.name} parity-browser.ts ${variant}`;
    const browser = findBrowser();
    if (browser === null) {
      for (const variant of variants) unavailable("chromium", label(variant), "no Chromium or Chrome is installed");
    } else {
      console.log(`# chromium: ${browser}`);
      const compiled = join(testBuild, "browser", "test");
      const named = (variant) => `chromium ${label(variant)}`;
      const outcome = await runInBrowser(browser, { packageDir, compiled, fixtures, label: named, variants, required });
      runs += outcome.runs;
      failures += outcome.failures;
    }
  }
  return { runs, failures };
}
