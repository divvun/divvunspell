// The parity scenario: the package against native divvun-fst on the shared
// fixtures. js/test/fixtures.mjs writes parity.json at test-build time from
// native divvun-fst, reading every fixture from its file; this module starts
// the package as it ships, loads every fixture from its bytes, runs every
// call in that file through it and compares each result with the native
// one, value for value. It is written once for every environment:
// parity-runtime.ts runs it under Node, Bun and Deno, and parity-browser.ts
// in a browser, on a page and in a Web Worker.

import type * as Fst from "../fst/index.ts";
import type { LookupResult, PartialSpellerConfig, SpellerMetadata, Suggestion } from "../fst/index.ts";

type Package = typeof Fst;

/** Where the scenario finds the package and the fixtures, as URLs the environment can read. */
export interface ParityArgs {
  module: string;
  /** URL of the directory of built fixtures, ending in a slash. */
  fixtures: string;
  /** Whether a real fixture the build left out fails the run rather than being skipped. */
  required: boolean;
}

/** Reads the bytes at a URL: with fetch in a browser, from the file system elsewhere. */
export type ReadBytes = (url: string) => Promise<Uint8Array>;

/** One kind of result compared over every fixture that has it, and every difference found. */
export interface ParityCheck {
  name: string;
  compared: number;
  differences: string[];
  /** Why the check did not run, when it is a skip rather than a pass or a failure. */
  skipped?: string;
}

/** A config as divvunspell writes it: kebab-case names, and `verbose` beside them. */
type NativeConfig = Record<string, unknown>;

/** What parity.json holds: each native result beside the call that gave it. */
interface Expected {
  defaultConfig: NativeConfig;
  configs: Record<string, PartialSpellerConfig>;
  /** The real fixtures the build could not find. */
  missing: string[];
  spellers: {
    file: string;
    metadata: SpellerMetadata | null;
    bundledConfig: NativeConfig | null;
    results: { word: string; config: string; isCorrect: boolean; suggest: Suggestion[]; analyzeOutput: Suggestion[] }[];
    analyses: { word: string; config: string; analyzeInput: Suggestion[] }[];
  }[];
  lookups: {
    file: string;
    member: string | null;
    format: string;
    word: string;
    limit: number | null;
    results: LookupResult[];
  }[];
}

/** The config name that stands for passing none. */
const NO_CONFIG = "none";

function describe(value: unknown): string {
  const text = JSON.stringify(value) ?? String(value);
  return text.length > 200 ? `${text.slice(0, 200)}...` : text;
}

// The package's results and the native ones compare exactly: the same keys,
// the same strings, the same numbers. Both sides carry each f32 weight as the
// JavaScript number that holds it exactly, so no tolerance is needed, and
// any would hide a weight the module computed differently.
/** Where `actual` first differs from `expected`, or null when they are the same. */
function difference(actual: unknown, expected: unknown, at: string): string | null {
  if (typeof expected === "string" && typeof actual === "string") {
    if (actual === expected) return null;
    let i = 0;
    while (i < actual.length && actual[i] === expected[i]) i += 1;
    const around = (s: string) => JSON.stringify(s.slice(Math.max(0, i - 30), i + 50));
    return `${at} differs at code unit ${i}: ${around(actual)}, native ${around(expected)}`;
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return `${at} is ${describe(actual)}, native ${describe(expected)}`;
    if (actual.length !== expected.length) {
      return `${at} has ${actual.length} entries, native ${expected.length}: ${describe(actual)}, native ${describe(expected)}`;
    }
    for (let i = 0; i < expected.length; i += 1) {
      const found = difference(actual[i], expected[i], `${at}[${i}]`);
      if (found !== null) return found;
    }
    return null;
  }
  if (typeof expected === "object" && expected !== null) {
    if (typeof actual !== "object" || actual === null || Array.isArray(actual)) {
      return `${at} is ${describe(actual)}, native ${describe(expected)}`;
    }
    const keys = Object.keys(expected).sort();
    const actualKeys = Object.keys(actual).sort();
    if (keys.join() !== actualKeys.join()) return `${at} has keys ${actualKeys.join()}, native ${keys.join()}`;
    for (const key of keys) {
      const found = difference(Reflect.get(actual, key), Reflect.get(expected, key), `${at}.${key}`);
      if (found !== null) return found;
    }
    return null;
  }
  return Object.is(actual, expected) ? null : `${at} is ${describe(actual)}, native ${describe(expected)}`;
}

class Check implements ParityCheck {
  readonly name: string;
  compared = 0;
  readonly differences: string[] = [];

  constructor(name: string) {
    this.name = name;
  }

  compare(what: string, actual: unknown, expected: unknown): void {
    this.compared += 1;
    const found = difference(actual, expected, "result");
    if (found !== null) this.differences.push(`${what}: ${found}`);
  }

  /** Runs `body`, counting anything it throws as a difference. */
  async run(body: () => Promise<void>): Promise<Check> {
    try {
      await body();
    } catch (error) {
      this.differences.push(`threw ${error instanceof Error ? (error.stack ?? error.message) : String(error)}`);
    }
    return this;
  }
}

/** `name` in divvunspell's kebab-case, as its config files spell it. */
function kebab(name: string): string {
  return name.replace(/[A-Z]/g, (upper) => `-${upper.toLowerCase()}`);
}

/** A config the package reads back, under the names divvunspell writes it with. */
function native(config: object): NativeConfig {
  return Object.fromEntries(
    Object.entries(config).map(([name, value]) => [
      kebab(name),
      typeof value === "object" && value !== null ? native(value) : value,
    ]),
  );
}

/** Every field name of `config`, a nested one after its parent's and a dot. */
function fieldNames(config: object, prefix = ""): string[] {
  return Object.entries(config).flatMap(([name, value]) => [
    `${prefix}${name}`,
    ...(typeof value === "object" && value !== null ? fieldNames(value, `${prefix}${name}.`) : []),
  ]);
}

function configs(pkg: Package, expected: Expected): Check {
  const check = new Check("the TypeScript SpellerConfig defaults are divvun-fst's, and the parity configs set every field");
  check.compare("defaultSpellerConfig", native(pkg.defaultSpellerConfig), expected.defaultConfig);
  const set = new Set(Object.values(expected.configs).flatMap((config) => fieldNames(config)));
  const unset = fieldNames(pkg.defaultSpellerConfig).filter((name) => !set.has(name));
  check.compare("fields no parity config sets", unset, []);
  return check;
}

function spellers(pkg: Package, fixture: (name: string) => Promise<Uint8Array>, expected: Expected): Promise<Check> {
  const check = new Check("isCorrect, suggest, analyzeOutput and analyzeInput in every speller, under every config");
  const config = (name: string): PartialSpellerConfig | undefined => {
    if (name === NO_CONFIG) return undefined;
    const found = expected.configs[name];
    if (found === undefined) throw new Error(`parity.json has no config called ${name}`);
    return found;
  };
  return check.run(async () => {
    for (const { file, metadata, bundledConfig, results, analyses } of expected.spellers) {
      const speller = pkg.readSpeller(await fixture(file));
      try {
        check.compare(`${file} metadata`, speller.metadata, metadata);
        check.compare(`${file} locale`, speller.locale, metadata?.info.locale ?? null);
        const bundled = speller.bundledConfig;
        check.compare(`${file} bundled config`, bundled === null ? null : native(bundled), bundledConfig);
        for (const result of results) {
          const what = `${file} ${JSON.stringify(result.word)} config ${result.config}`;
          const given = config(result.config);
          check.compare(`${what} isCorrect`, speller.isCorrect(result.word, given), result.isCorrect);
          check.compare(`${what} suggest`, speller.suggest(result.word, given), result.suggest);
          check.compare(`${what} analyzeOutput`, speller.analyzeOutput(result.word, given), result.analyzeOutput);
        }
        for (const analysis of analyses) {
          const what = `${file} ${JSON.stringify(analysis.word)} config ${analysis.config} analyzeInput`;
          check.compare(what, speller.analyzeInput(analysis.word, config(analysis.config)), analysis.analyzeInput);
        }
      } finally {
        speller.free();
      }
    }
  });
}

function lookups(pkg: Package, fixture: (name: string) => Promise<Uint8Array>, expected: Expected): Promise<Check> {
  const check = new Check("lookup in every transducer, with and without a limit");
  return check.run(async () => {
    const groups = new Map<string, Expected["lookups"]>();
    for (const entry of expected.lookups) {
      const key = JSON.stringify([entry.file, entry.member]);
      groups.set(key, [...(groups.get(key) ?? []), entry]);
    }
    for (const entries of groups.values()) {
      const [first] = entries;
      if (first === undefined) continue;
      const options = first.member === null ? undefined : { member: first.member };
      const transducer = pkg.readTransducer(await fixture(first.file), options);
      try {
        const name = `${first.file}${first.member === null ? "" : ` member ${first.member}`}`;
        check.compare(`${name} format`, transducer.format, first.format);
        for (const { word, limit, results } of entries) {
          const found = transducer.lookup(word, limit === null ? undefined : { limit });
          check.compare(`${name} ${JSON.stringify(word)} limit ${limit}`, found, results);
        }
      } finally {
        transducer.free();
      }
    }
  });
}

function missing(expected: Expected, required: boolean): ParityCheck[] {
  return expected.missing.map((name) => {
    const check = { name: `the real fixture ${name}`, compared: 0, differences: [] };
    return required ? { ...check, differences: ["the build left it out, and every fixture is required"] } : { ...check, skipped: "the build left it out" };
  });
}

/**
 * Starts the package at `args.module` from the `.wasm` file beside it, as
 * a caller who passes init nothing does, and compares its results for every
 * fixture in parity.json with the native ones. Throws only when the package
 * or parity.json cannot be loaded; a result that differs, or a call that
 * throws, is a difference in the check it belongs to.
 */
export async function runParity(args: ParityArgs, read: ReadBytes): Promise<ParityCheck[]> {
  const pkg: Package = await import(args.module);
  await pkg.init();
  const fixture = (name: string) => read(new URL(name, args.fixtures).href);
  const expected: Expected = JSON.parse(new TextDecoder().decode(await fixture("parity.json")));
  return [
    configs(pkg, expected),
    await spellers(pkg, fixture, expected),
    await lookups(pkg, fixture, expected),
    ...missing(expected, args.required),
  ];
}

/** The most differences a failed check prints. */
const SHOWN = 20;

/**
 * The lines that report `checks` under `label`, and whether every check
 * passed. A check that compared nothing fails: the native expectations are
 * missing, not met.
 */
export function report(label: string, checks: ParityCheck[]): { lines: string[]; passed: boolean } {
  const lines: string[] = [];
  let passed = true;
  for (const { name, compared, differences, skipped } of checks) {
    if (skipped !== undefined) {
      lines.push(`ok - ${label}: ${name} # SKIP ${skipped} (--require-all makes this a failure)`);
      continue;
    }
    if (differences.length === 0 && compared > 0) {
      lines.push(`ok - ${label}: ${name} (${compared} results the same as native)`);
      continue;
    }
    passed = false;
    lines.push(`not ok - ${label}: ${name}: ${differences.length} of ${compared} results differ from native`);
    for (const line of differences.slice(0, SHOWN)) lines.push(`  ${line}`);
    if (differences.length > SHOWN) lines.push(`  and ${differences.length - SHOWN} more`);
  }
  return { lines, passed };
}
