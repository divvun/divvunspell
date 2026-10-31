// Runs the parity scenario in a headless Chromium, with no driver package: a
// small HTTP server serves the package as it ships, the compiled scenario
// (target/js-test/browser) and the fixtures, the browser opens a page that
// runs the scenario, and the page posts its report back to the server. Only
// js/test/parity-runner.mjs calls this.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { homedir, tmpdir } from "node:os";
import { delimiter, extname, join, normalize, sep } from "node:path";

const TIMEOUT_MS = 300_000;

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
};

/** Executable names Playwright's Chromium builds and Chrome go by. */
const NAMES = ["chrome-headless-shell", "headless_shell", "Google Chrome for Testing", "chrome", "Chromium"];

function isExecutable(path) {
  try {
    const stat = statSync(path);
    return stat.isFile() && (stat.mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

/** An executable called one of NAMES at most `depth` directories below `dir`. */
function findIn(dir, depth) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return null;
  }
  for (const entry of entries) {
    if (NAMES.includes(entry.name) && isExecutable(join(dir, entry.name))) return join(dir, entry.name);
  }
  if (depth === 0) return null;
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const found = findIn(join(dir, entry.name), depth - 1);
    if (found !== null) return found;
  }
  return null;
}

function playwrightCache(env) {
  if (env.PLAYWRIGHT_BROWSERS_PATH) return env.PLAYWRIGHT_BROWSERS_PATH;
  if (process.platform === "darwin") return join(homedir(), "Library", "Caches", "ms-playwright");
  if (process.platform === "win32") return join(env.LOCALAPPDATA ?? homedir(), "ms-playwright");
  return join(homedir(), ".cache", "ms-playwright");
}

/**
 * The Chromium to run, or null when there is none: `FST_JS_TEST_BROWSER` if
 * set, else the newest headless shell in Playwright's cache, its newest full
 * Chromium, Google Chrome, or a Chromium or Chrome on PATH.
 */
export function findBrowser(env = process.env) {
  if (env.FST_JS_TEST_BROWSER) return env.FST_JS_TEST_BROWSER;
  const cache = playwrightCache(env);
  const builds = (prefix) => {
    let names = [];
    try {
      names = readdirSync(cache).filter((name) => new RegExp(`^${prefix}-\\d+$`).test(name));
    } catch {
      return [];
    }
    return names.sort((a, b) => Number(b.split("-")[1]) - Number(a.split("-")[1])).map((name) => join(cache, name));
  };
  for (const build of [...builds("chromium_headless_shell"), ...builds("chromium")]) {
    const found = findIn(build, 5);
    if (found !== null) return found;
  }
  const chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
  if (process.platform === "darwin" && isExecutable(chrome)) return chrome;
  for (const dir of (env.PATH ?? "").split(delimiter)) {
    for (const name of ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]) {
      if (dir !== "" && isExecutable(join(dir, name))) return join(dir, name);
    }
  }
  return null;
}

/** The file under `root` that `path` names, or null when it names none or leaves `root`. */
function fileUnder(root, path) {
  const file = normalize(join(root, decodeURIComponent(path)));
  if (!file.startsWith(root + sep) || !existsSync(file) || !statSync(file).isFile()) return null;
  return file;
}

/**
 * The page that runs the compiled scenario, which reads its arguments from
 * the page's query. A script that fails to load fires no error on the window,
 * only on its element, so the page listens in the capture phase and reports
 * the failure rather than leaving the run to time out.
 */
const PAGE = `<!doctype html>
<meta charset="utf-8">
<title>@divvun/fst parity</title>
<script>
addEventListener("error", (event) => {
  const args = JSON.parse(new URLSearchParams(location.search).get("args"));
  const what = event.message || (event.target && event.target.src) || "an error";
  fetch(args.report, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ lines: ["not ok - " + args.label + ": " + what], passed: false }),
  });
}, true);
</script>
<script type="module" src="/test/parity-browser.js"></script>
`;

/**
 * Serves `roots` (URL prefix to directory) and the scenario page, and
 * resolves each report the page posts to `/report/<variant>`.
 */
async function serve(roots) {
  const reports = new Map();
  const reported = (variant) =>
    new Promise((resolve) => {
      reports.set(variant, resolve);
    });
  const server = createServer((request, response) => {
    const url = new URL(request.url, "http://localhost");
    if (request.method === "POST" && url.pathname.startsWith("/report/")) {
      const chunks = [];
      request.on("data", (chunk) => chunks.push(chunk));
      request.on("end", () => {
        response.writeHead(204).end();
        reports.get(url.pathname.slice("/report/".length))?.(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      });
      return;
    }
    if (url.pathname === "/parity.html") {
      response.writeHead(200, { "content-type": TYPES[".html"] }).end(PAGE);
      return;
    }
    for (const [prefix, root] of Object.entries(roots)) {
      if (!url.pathname.startsWith(prefix)) continue;
      const file = fileUnder(root, url.pathname.slice(prefix.length));
      if (file === null) break;
      response.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
      response.end(readFileSync(file));
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, reported, base: `http://127.0.0.1:${server.address().port}` };
}

/**
 * Starts `browser` on `url` with a profile of its own. `exited` resolves
 * with what it wrote to stderr once it exits; `stop` ends it, removes the
 * profile and resolves with the same.
 */
function launch(browser, url) {
  const profile = mkdtempSync(join(tmpdir(), "fst-parity-browser-"));
  const flags = [
    "--headless",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-gpu",
    "--disable-extensions",
    `--user-data-dir=${profile}`,
  ];
  const child = spawn(browser, [...flags, url], { stdio: ["ignore", "ignore", "pipe"] });
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const exited = new Promise((resolve) => {
    child.on("error", (error) => resolve(`cannot start ${browser}: ${error.message}`));
    child.on("close", () => resolve(stderr));
  });
  const stop = async () => {
    child.kill();
    const output = await exited;
    rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
    return output;
  };
  return { exited, stop };
}

/**
 * Runs the scenario in `browser` once for each of `variants` ("main-thread",
 * "worker"), each in a browser of its own, and prints what each reports.
 * `compiled` is the directory of the compiled js/test modules,
 * `label(variant)` names the run in the report, and `required` says whether
 * a fixture the build left out fails the run. Returns how many ran and how
 * many failed.
 */
export async function runInBrowser(browser, { packageDir, compiled, fixtures, label, variants, required }) {
  const served = await serve({ "/package/": packageDir, "/test/": compiled, "/fixtures/": fixtures });
  const { base } = served;
  let failures = 0;
  try {
    for (const variant of variants) {
      const args = {
        module: `${base}/package/index.js`,
        fixtures: `${base}/fixtures/`,
        label: label(variant),
        variant,
        required,
        report: `${base}/report/${variant}`,
      };
      const reported = served.reported(variant);
      const browserRun = launch(browser, `${base}/parity.html?args=${encodeURIComponent(JSON.stringify(args))}`);
      let timer;
      const outcome = await Promise.race([
        reported,
        browserRun.exited.then(() => ({ exited: true })),
        new Promise((resolve) => {
          timer = setTimeout(() => resolve({ timedOut: true }), TIMEOUT_MS);
        }),
      ]);
      clearTimeout(timer);
      const stderr = await browserRun.stop();
      if (outcome.lines !== undefined) {
        for (const line of outcome.lines) console.log(line);
        if (outcome.passed) continue;
      } else {
        const why = outcome.timedOut ? `no report within ${TIMEOUT_MS / 1000} s` : "the browser exited without a report";
        console.log(`not ok - ${label(variant)}: ${why}`);
        console.log(stderr);
      }
      failures += 1;
    }
  } finally {
    served.server.close();
  }
  return { runs: variants.length, failures };
}
