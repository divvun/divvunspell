// The API contract against the real module of a test build: errors from the
// library and the wrapper, object lifetime, logging fed by the library's
// tracing, and the config a call runs with.

import type * as Testing from "../fst/testing.ts";
import {
  assert,
  assertEqual,
  collectUntil,
  readBytes,
  runTests,
  scenarioArgs,
  test,
  thrownCode,
  withConsole,
} from "./harness.ts";

const args = scenarioArgs();
const pkg: typeof Testing = await import(args.module);
const { testing } = pkg;
await pkg.init(await readBytes(args.wasm));
const fixture = (name: string) => readBytes(new URL(name, args.fixtures).href);

type LogEvent = Parameters<Parameters<typeof pkg.setLogger>[0] & {}>[0];

/** Sends events at `level` and above to the returned array. */
function capture(level: Parameters<typeof pkg.setLogLevel>[0]): LogEvent[] {
  const events: LogEvent[] = [];
  pkg.setLogger((event) => events.push(event));
  pkg.setLogLevel(level);
  return events;
}

function reset(): void {
  pkg.setLogger(null);
  pkg.setLogLevel("warn");
}

function dropped(events: LogEvent[], label: string): number {
  return events.filter((event) => event.message === `dropped ${label}`).length;
}

function thrown(f: () => unknown): unknown {
  try {
    f();
  } catch (error) {
    return error;
  }
  throw new Error("nothing was thrown");
}

/** `value` and every object in it, the way `JSON.stringify` writes them, keys sorted. */
function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, inner: unknown) =>
    typeof inner === "object" && inner !== null && !Array.isArray(inner)
      ? Object.fromEntries(Object.entries(inner).sort(([a], [b]) => (a < b ? -1 : 1)))
      : inner,
  );
}

test("a library error is thrown as an FstError with its code and the library's message", () => {
  const error = thrown(() => pkg.readSpeller(new Uint8Array([0, 1, 2, 3, 4])));
  assert(error instanceof pkg.FstError, "an FstError");
  assertEqual(error.name, "FstError", "name");
  assertEqual(error.code, "unsupported-format", "code");
  assertEqual(
    error.message,
    "'speller' is neither a BHFST nor a ZHFST archive: it starts with the bytes 00 01 02 03",
    "message",
  );
  assertEqual(testing.createObject("after").label(), "after", "the module carries on");
});

test("broken bytes in a format the package reads throw invalid-format, with the reason", async () => {
  const small = await fixture("small.bhfst");
  const truncated = small.slice(0, small.length - 40);
  assertEqual(thrownCode(pkg.FstError, () => pkg.readSpeller(truncated)), "invalid-format", "a truncated BHFST");
  const lexicon = await fixture("small-acceptor.dhfst");
  const error = thrown(() => pkg.readTransducer(lexicon.slice(0, 40)));
  assert(error instanceof pkg.FstError && error.code === "invalid-format", `a truncated DHFST: ${String(error)}`);
  assertEqual(thrownCode(pkg.FstError, () => pkg.readTransducer(new Uint8Array(0))), "invalid-format", "nothing");
});

test("a box of several transducers needs a member, and names them", async () => {
  const bytes = await fixture("two-lexicons.box");
  const error = thrown(() => pkg.readTransducer(bytes));
  assert(error instanceof pkg.FstError, "an FstError");
  assertEqual(error.code, "invalid-argument", "code");
  assert(error.message.includes("acceptor.default.thfst, second.thfst"), error.message);
  const missing = thrown(() => pkg.readTransducer(bytes, { member: "third.thfst" }));
  assert(missing instanceof pkg.FstError && missing.code === "invalid-argument", "a member it does not hold");
  const chosen = pkg.readTransducer(bytes, { member: "second.thfst" });
  assertEqual(chosen.format, "thfst", "the member chosen");
  chosen.free();
});

test("the wrapper refuses arguments of the wrong type before entering the module", async () => {
  const speller = pkg.readSpeller(await fixture("small.bhfst"));
  const lexicon = pkg.readTransducer(await fixture("lexicon.box"));
  const calls: [string, () => unknown][] = [
    ["readSpeller(string)", () => Reflect.apply(pkg.readSpeller, undefined, ["small.bhfst"])],
    ["readTransducer(member: 3)", () => Reflect.apply(pkg.readTransducer, undefined, [new Uint8Array(8), { member: 3 }])],
    ["suggest(3)", () => Reflect.apply(speller.suggest, speller, [3])],
    ["isCorrect(config: [])", () => Reflect.apply(speller.isCorrect, speller, ["cat", []])],
    ["suggest(config: null)", () => Reflect.apply(speller.suggest, speller, ["cat", null])],
    ["lookup(limit: -1)", () => lexicon.lookup("cat", { limit: -1 })],
    ["lookup(limit: 1.5)", () => lexicon.lookup("cat", { limit: 1.5 })],
  ];
  for (const [name, call] of calls) assertEqual(thrownCode(pkg.FstError, call), "invalid-argument", name);
  speller.free();
  lexicon.free();
});

test("a config with a field divvun-fst does not have, or a value of the wrong type, is refused", async () => {
  const speller = pkg.readSpeller(await fixture("small.bhfst"));
  const bad: [string, unknown][] = [
    ["a misspelt field", { nbest: 3 }],
    ["a kebab-case field", { "n-best": 3 }],
    ["a misspelt penalty", { reweight: { start: 1 } }],
    ["a null boolean", { recase: null }],
    ["a fraction for a count", { nBest: 2.5 }],
    ["a negative count", { searchBudget: -1 }],
    ["a string for a weight", { maxWeight: "10" }],
    ["an infinite weight", { maxWeight: Infinity }],
    ["NaN", { beam: Number.NaN }],
    ["a bigint", { nBest: 3n }],
  ];
  for (const [name, config] of bad) {
    const error = thrown(() => Reflect.apply(speller.suggest, speller, ["kat", config]));
    assert(error instanceof pkg.FstError, `${name}: an FstError, not ${String(error)}`);
    assertEqual(error.code, "invalid-argument", name);
    assert(error.message.startsWith("a speller config: "), `${name}: ${error.message}`);
  }
  const unknown = thrown(() => Reflect.apply(speller.suggest, speller, ["kat", { nbest: 3 }]));
  assert(unknown instanceof Error && unknown.message.includes("nBest"), "the error lists the fields there are");
  speller.free();
});

test("the TypeScript defaults are the module's SpellerConfig::default(), field for field", () => {
  assertEqual(canonical(pkg.defaultSpellerConfig), canonical(testing.defaultConfig()), "defaults");
  assert(Object.isFrozen(pkg.defaultSpellerConfig) && Object.isFrozen(pkg.defaultSpellerConfig.reweight), "frozen");
});

test("every config field the wrapper takes reaches divvun-fst as given", () => {
  const every = {
    nBest: 3,
    maxWeight: 40.5,
    beam: 15,
    reweight: { startPenalty: 2, endPenalty: 3, midPenalty: 1.25, curve: 5 },
    recase: false,
    completionMarker: "+",
    astarLookahead: true,
    searchDedup: false,
    mutatorSubsets: false,
    searchBudget: 5000,
    wordSplitWeight: 20,
    boundaryEditWeight: 15,
    initialCaseWeight: 8.5,
    verbose: true,
  } satisfies Required<Testing.SpellerConfig>;
  assertEqual(canonical(testing.resolveConfig(every)), canonical(every), "every field set");
  assertEqual(canonical(Object.keys(every).sort()), canonical(Object.keys(pkg.defaultSpellerConfig).sort()), "fields");
});

test("a field left out takes the built-in default, and null means none", () => {
  const resolved = testing.resolveConfig({ nBest: null, reweight: { midPenalty: 1 }, maxWeight: undefined });
  assertEqual(resolved.nBest, null, "nBest");
  assertEqual(resolved.maxWeight, pkg.defaultSpellerConfig.maxWeight, "maxWeight left undefined");
  assertEqual(canonical(resolved.reweight), canonical({ ...pkg.defaultSpellerConfig.reweight, midPenalty: 1 }), "reweight");
  assertEqual(testing.resolveConfig({ reweight: null }).reweight, null, "reweight null");
  assertEqual(canonical(testing.resolveConfig({})), canonical(pkg.defaultSpellerConfig), "an empty config");
});

test("a speller reads its archive's metadata and bundled config once, and frozen", async () => {
  const speller = pkg.readSpeller(await fixture("small.bhfst"));
  assertEqual(speller.locale, "zxx", "locale");
  assertEqual(speller.metadata?.info.title[1]?.lang, "se", "a title's language");
  const bundled = speller.bundledConfig;
  assert(bundled !== null && Object.isFrozen(bundled) && Object.isFrozen(bundled.reweight), "a frozen config");
  assertEqual(bundled.nBest, 3, "the bundled n-best");
  assert(speller.metadata === speller.metadata, "read once");
  speller.free();
  assertEqual(thrownCode(pkg.FstError, () => speller.metadata), "freed", "metadata after free");
  const bare = pkg.readSpeller(await fixture("small-bare.bhfst"));
  assertEqual(bare.metadata, null, "no meta.json");
  assertEqual(bare.locale, null, "no locale");
  assertEqual(bare.bundledConfig, null, "no bundled config");
  bare.free();
});

test("free releases the object's memory once, and a call after it throws freed", async () => {
  const events = capture("debug");
  const object = testing.createObject("a");
  assertEqual(object.label(), "a", "label");
  object.free();
  object.free();
  assertEqual(dropped(events, "a"), 1, "times the module dropped it");
  assertEqual(thrownCode(pkg.FstError, () => object.label()), "freed", "a call after free");
  const speller = pkg.readSpeller(await fixture("small.bhfst"));
  speller.free();
  speller.free();
  assertEqual(thrownCode(pkg.FstError, () => speller.suggest("kat")), "freed", "a speller after free");
  const transducer = pkg.readTransducer(await fixture("lexicon.box"));
  transducer[Symbol.dispose]();
  assertEqual(thrownCode(pkg.FstError, () => transducer.lookup("cat")), "freed", "a transducer after dispose");
  reset();
});

test("a using declaration and Symbol.dispose free the object", () => {
  const events = capture("debug");
  {
    using scoped = testing.createObject("scoped");
    assertEqual(scoped.label(), "scoped", "label inside the block");
  }
  assertEqual(dropped(events, "scoped"), 1, "dropped at the end of the block");
  const disposed = testing.createObject("disposed");
  disposed[Symbol.dispose]();
  assertEqual(dropped(events, "disposed"), 1, "dropped by Symbol.dispose");
  reset();
});

test("an object dropped without free is released after garbage collection", async () => {
  const events = capture("debug");
  (() => {
    testing.createObject("collected");
  })();
  assert(await collectUntil(() => dropped(events, "collected") === 1), "the module dropped it");
  reset();
});

test("init again keeps the instance and the objects already made in it", async () => {
  const object = testing.createObject("before");
  await pkg.init();
  assertEqual(object.label(), "before", "the object made before the second init");
  object.free();
});

test("by default warnings and errors from the library reach the console, and lower levels do not", async () => {
  reset();
  await withConsole((lines) => {
    for (const level of ["error", "warn", "info", "debug", "trace"] as const) {
      testing.log(level, `at ${level}`);
    }
    const seen = lines.map(({ method, text }) => `${method} ${text}`);
    assertEqual(
      seen.join("; "),
      "error divvun_fst_wasm::test_hooks: at error; warn divvun_fst_wasm::test_hooks: at warn",
      "console lines",
    );
  });
});

test("a replaced logger gets each event's level, target and message", () => {
  const events = capture("info");
  testing.log("info", "hello");
  assertEqual(
    JSON.stringify(events),
    '[{"level":"info","target":"divvun_fst_wasm::test_hooks","message":"hello"}]',
    "events",
  );
  reset();
});

test("the level set is the level the module sends, and off sends nothing", () => {
  const events = capture("info");
  for (const level of ["error", "warn", "info", "debug", "trace"] as const) testing.log(level, "x");
  pkg.setLogLevel("trace");
  testing.log("trace", "y");
  pkg.setLogLevel("off");
  testing.log("error", "z");
  assertEqual(events.map((event) => event.level).join(" "), "error warn info trace", "levels");
  reset();
});

test("the library's own events reach the logger with its module as the target", async () => {
  const events = capture("debug");
  const speller = pkg.readSpeller(await fixture("small.bhfst"));
  assert(speller.isCorrect("cat"), "cat is a word");
  speller.free();
  assert(
    events.some((event) => event.level === "debug" && event.target.startsWith("divvun_fst::")),
    `a debug event from divvun-fst: ${JSON.stringify(events.slice(0, 3))}`,
  );
  reset();
});

test("a logger that throws is reported on the console and the call goes on", async () => {
  pkg.setLogger(() => {
    throw new Error("logger broke");
  });
  pkg.setLogLevel("debug");
  await withConsole((lines) => {
    testing.log("warn", "unheard");
    assert(lines.some(({ text }) => text.includes("logger broke")), "the console names the failure");
  });
  assertEqual(testing.createObject("still").label(), "still", "the module carries on");
  reset();
});

await runTests(args.label);
