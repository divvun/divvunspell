// The fixtures of the parity suite (js/test/parity.ts) and what native
// divvun-fst gives for them, written at test-build time to
// target/js-test/fixtures with parity.json beside them. js/build.mjs --test
// calls this, and the Python package's tests (python/tests) run it on its
// own to write the same files elsewhere:
//
//   node js/test/fixtures.mjs <out>
//
// Small spellers and transducers are made from the repository's own
// tests/fixtures. Real spellers, analysers and generators are read from the
// paths below, or from the path in each one's environment variable; one that
// is not there is left out, and parity.json names it, so that the suite can
// report it skipped, or failed under --require-all.
//
// The crate example crates/divvun-fst-wasm/examples/js_fixtures.rs writes the
// derived fixtures and the native expectations; dhfst-tools writes the DHFST
// copy of the ZHFST speller, as a speller's build does.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** Real fixtures by name: the environment variable that can name each, and where to look otherwise. */
const SOURCES = {
  "sma.bhfst": ["FST_JS_SMA_BHFST", "git/divvun/divvunspell/sma.bhfst"],
  "se.bhfst": ["FST_JS_SE_BHFST", "git/divvun/divvunspell/se.bhfst"],
  "ga.zhfst": ["FST_JS_GA_ZHFST", "git/divvun/lang-gle/tools/spellcheckers/ga.zhfst"],
  "sma-analyser.hfstol": ["FST_JS_SMA_ANALYSER", "git/divvun/lang-sma/src/fst/analyser-gt-desc.hfstol"],
  "sma-generator.hfstol": ["FST_JS_SMA_GENERATOR", "git/divvun/lang-sma/src/fst/generator-gt-norm.hfstol"],
};

/**
 * The configs the spellers are run under, by name, as the package takes them
 * (`js`) and as divvunspell reads a config file (`native`). Each is written
 * out twice by hand so that the package's camelCase mirror is checked against
 * divvunspell's own field names. `none` passes no config.
 */
const CONFIGS = {
  defaults: { js: {}, native: {} },
  nbest3: { js: { nBest: 3 }, native: { "n-best": 3 } },
  wide: {
    js: { nBest: null, maxWeight: 60, beam: 20 },
    native: { "n-best": null, "max-weight": 60, beam: 20 },
  },
  plain: { js: { reweight: null, recase: false }, native: { reweight: null, recase: false } },
  curve: {
    js: { reweight: { startPenalty: 2, midPenalty: 1.5, endPenalty: 3, curve: 5 } },
    native: { reweight: { "start-penalty": 2, "mid-penalty": 1.5, "end-penalty": 3, curve: 5 } },
  },
  partialReweight: { js: { reweight: { midPenalty: 0.25 } }, native: { reweight: { "mid-penalty": 0.25 } } },
  verbose: { js: { verbose: true, nBest: 5 }, native: { verbose: true, "n-best": 5 } },
  budget: { js: { searchBudget: 2000 }, native: { "search-budget": 2000 } },
  extras: {
    js: { wordSplitWeight: 20, boundaryEditWeight: 15, initialCaseWeight: 8 },
    native: { "word-split-weight": 20, "boundary-edit-weight": 15, "initial-case-weight": 8 },
  },
  search: {
    js: { astarLookahead: true, searchDedup: false, mutatorSubsets: false, completionMarker: "+" },
    native: { "astar-lookahead": true, "search-dedup": false, "mutator-subsets": false, "completion-marker": "+" },
  },
};

const ALL_CONFIGS = ["none", ...Object.keys(CONFIGS)];

const SMALL_WORDS = ["cat", "car", "cart", "care", "kat", "cet", "cad", "kare", "card", "ked", "Kat", "KAT", "xyz", "", "c", "cat car", "123"];
const SMA_WORDS = ["gïele", "gïelee", "saemien", "saemein", "gåetie", "gaetie", "maanah", "jïh", "jih", "Saemien", "GÏELE", "", ",", "123", "😀"];
const SE_WORDS = ["sámi", "sami", "samegiella", "giella", "gilla", "boahta", "dál", "callit", "Sámi", "SÁMI", "girjji"];
const GA_WORDS = ["agus", "agsu", "Gaeilge", "Gaelige", "teanga", "tenga", "scoil", "bfuil", "teangaa", "AGUS"];
const ANALYSER_WORDS = ["gïele", "saemien", "gåetie", "jïh", "maanah", "gïelem", "Saemien", "xyzzy", "", "1"];

/** Which fixtures are run, under which configs, and with which words. */
function spellerRequests(have) {
  const speller = (file, words, configs, analyse = ["none"]) => ({ file, words, configs, analyse });
  return [
    speller("small.bhfst", SMALL_WORDS, ALL_CONFIGS, ["none", "plain"]),
    speller("small-bare.bhfst", SMALL_WORDS, ["none", "defaults", "nbest3", "verbose"]),
    speller("small-dhfst.bhfst", SMALL_WORDS, ["none", "nbest3", "verbose", "search"]),
    ...(have("sma.bhfst")
      ? [
          speller("sma.bhfst", SMA_WORDS, ["none", "defaults", "nbest3", "wide", "plain", "curve", "verbose", "budget", "extras"], [
            "none",
            "plain",
          ]),
          speller("sma.v1.bhfst", SMA_WORDS, ["none", "nbest3"]),
        ]
      : []),
    ...(have("se.bhfst") ? [speller("se.bhfst", SE_WORDS, ["none", "nbest3", "budget", "extras"])] : []),
    ...(have("ga.zhfst")
      ? [
          speller("ga.zhfst", GA_WORDS, ["none", "nbest3", "curve", "verbose"]),
          speller("ga.stored.zhfst", GA_WORDS, ["none"]),
          speller("ga.dhfst.bhfst", GA_WORDS, ["none", "nbest3"]),
        ]
      : []),
  ];
}

function lookupRequests(have) {
  const lookup = (file, words, { member = null, nativeMember = member, limits = [null] } = {}) => ({
    file,
    member,
    nativeMember,
    words,
    limits,
  });
  const analyser = lookup("sma-analyser.hfstol", ANALYSER_WORDS, { limits: [null, 1] });
  return [
    lookup("lexicon.box", SMALL_WORDS, { nativeMember: "", limits: [null, 0, 1] }),
    lookup("two-lexicons.box", SMALL_WORDS, { member: "second.thfst" }),
    lookup("small-acceptor.dhfst", SMALL_WORDS),
    lookup("small-errmodel.dhfst", SMALL_WORDS, { limits: [null, 2] }),
    lookup("small-dhfst.bhfst", SMALL_WORDS, { member: "acceptor.default.dhfst" }),
    ...(have("sma-analyser.hfstol")
      ? [
          analyser,
          lookup("sma-analyser.box", ANALYSER_WORDS, { nativeMember: "analyser.thfst" }),
        ]
      : []),
    ...(have("sma-analyser.hfstol") && have("sma-generator.hfstol")
      ? [{ ...lookup("sma-generator.hfstol", []), wordsFrom: analyser }]
      : []),
    ...(have("sma.bhfst") ? [lookup("sma.bhfst", SMA_WORDS, { member: "acceptor.default.thfst" })] : []),
    ...(have("ga.zhfst")
      ? [
          lookup("ga.acceptor.default.hfst", GA_WORDS),
          lookup("ga.acceptor.dhfst", GA_WORDS),
          lookup("ga.dhfst.bhfst", GA_WORDS, { member: "acceptor.default.dhfst" }),
        ]
      : []),
  ];
}

/** Each real fixture that is there, by name, and the names of those that are not. */
function sources(env = process.env) {
  const found = {};
  const missing = [];
  for (const [name, [variable, path]] of Object.entries(SOURCES)) {
    const at = env[variable] || join(homedir(), path);
    if (existsSync(at)) found[name] = at;
    else missing.push(`${name} (not at ${at}; set ${variable})`);
  }
  return { found, missing };
}

/**
 * Writes the fixtures and `out/parity.json`. `run(command, args)` runs a
 * command in the repository root `root`.
 */
export function buildFixtures(root, out, run) {
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  const { found, missing } = sources();
  for (const name of missing) console.log(`# fixtures: leaving out ${name}`);
  const have = (name) => name in found;

  const sourcesFile = join(out, "fixture-sources.json");
  writeFileSync(sourcesFile, JSON.stringify(found));
  const example = ["run", "--quiet", "--release", "-p", "divvun-fst-wasm", "--example", "js_fixtures", "--"];
  run("cargo", [...example, "derive", sourcesFile, out]);
  rmSync(sourcesFile);

  if (have("ga.zhfst")) {
    const dhfst = ["run", "--quiet", "--release", "-p", "dhfst-tools", "--"];
    const at = (name) => join(out, name);
    run("cargo", [...dhfst, "write", at("ga.errmodel.default.hfst"), at("ga.errmodel.dhfst")]);
    run("cargo", [...dhfst, "acceptor", at("ga.acceptor.default.hfst"), at("ga.acceptor.dhfst")]);
    run("cargo", [...dhfst, "bhfst", at("ga.zhfst"), at("ga.errmodel.dhfst"), at("ga.dhfst-errmodel.bhfst")]);
    run("cargo", [
      ...dhfst,
      "pack",
      at("ga.dhfst-errmodel.bhfst"),
      at("ga.dhfst.bhfst"),
      "--acceptor",
      at("ga.acceptor.dhfst"),
    ]);
  }

  const request = {
    configs: Object.fromEntries(Object.entries(CONFIGS).map(([name, { native }]) => [name, native])),
    spellers: spellerRequests(have),
    lookups: lookupRequests(have),
  };
  const requestFile = join(out, "parity-request.json");
  const nativeFile = join(out, "parity-native.json");
  writeFileSync(requestFile, JSON.stringify(request));
  run("cargo", [...example, "expect", out, requestFile, nativeFile]);
  const native = JSON.parse(readFileSync(nativeFile, "utf8"));
  rmSync(requestFile);
  rmSync(nativeFile);

  const configs = Object.fromEntries(Object.entries(CONFIGS).map(([name, { js }]) => [name, js]));
  writeFileSync(join(out, "parity.json"), JSON.stringify({ ...native, configs, missing }));
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [out] = process.argv.slice(2);
  if (out === undefined) throw new Error("usage: node js/test/fixtures.mjs <out>");
  const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
  buildFixtures(root, resolve(out), (command, args) => {
    console.log(`$ ${command} ${args.join(" ")}`);
    execFileSync(command, args, { cwd: root, stdio: "inherit" });
  });
}
