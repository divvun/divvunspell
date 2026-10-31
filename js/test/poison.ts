// A panic in the real module of a test build, from a free function or from a
// method that holds a borrow of its object. Each variant runs alone, since
// the instance stays poisoned.

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
} from "./harness.ts";

const args = scenarioArgs();
const pkg: typeof Testing = await import(args.module);
const { testing } = pkg;
const started = pkg.init(await readBytes(args.wasm));
await started;

const messages: string[] = [];
pkg.setLogger((event) => messages.push(event.message));
pkg.setLogLevel("debug");
const survivor = testing.createObject("survivor");
let doomed: ReturnType<typeof testing.createObject> | null = testing.createObject("doomed");
const victim = testing.createObject("victim");

function trigger(): void {
  if (args.variant === "method") victim.panicWhileBorrowed("boom");
  else testing.panic("boom");
}

test("a panic is rethrown as FstError poisoned, carrying the panic message", () => {
  let error: unknown = null;
  try {
    trigger();
  } catch (thrown) {
    error = thrown;
  }
  assert(error instanceof pkg.FstError, `an FstError, not ${String(error)}`);
  assertEqual(error.code, "poisoned", "code");
  assert(error.message.includes("boom"), `the panic message: ${error.message}`);
});

test("every later call throws poisoned without running", () => {
  const before = messages.length;
  const calls: [string, () => unknown][] = [
    ["version", () => pkg.version()],
    ["setLogLevel", () => pkg.setLogLevel("info")],
    ["setLogger", () => pkg.setLogger(null)],
    ["createObject", () => testing.createObject("late")],
    ["a method", () => survivor.label()],
    ["log", () => testing.log("error", "unheard")],
    ["the panic again", () => trigger()],
  ];
  for (const [name, call] of calls) {
    assertEqual(thrownCode(pkg.FstError, call), "poisoned", name);
  }
  assertEqual(messages.length, before, "events from the module after the trap");
});

test("freeing after the trap does nothing and never enters the module", () => {
  survivor.free();
  survivor.free();
  victim.free();
  assertEqual(thrownCode(pkg.FstError, () => survivor.label()), "poisoned", "a call after it");
  assert(!messages.some((message) => message.startsWith("dropped")), "nothing was dropped");
});

test("an object dropped after the trap is never freed through the module", async () => {
  doomed = null;
  await collectUntil(() => false);
  assertEqual(doomed, null, "the object is gone");
  assert(!messages.includes("dropped doomed"), "the module never dropped it");
});

test("init after the trap returns the same promise", () => {
  assert(pkg.init() === started, "the same promise");
});

await runTests(args.label);
