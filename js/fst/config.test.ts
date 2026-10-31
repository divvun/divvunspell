// Unit tests for the config the wrapper hands the module. They need no build.

import { assert, assertEqual, runTests, test, thrownCode } from "../test/harness.ts";
import { configArg, defaultSpellerConfig, parseConfig } from "./config.ts";
import { FstError } from "./errors.ts";

test("no config is no argument, and fields set to undefined are left out", () => {
  assertEqual(configArg(undefined), undefined, "no config");
  assertEqual(configArg({}), "{}", "an empty config");
  assertEqual(configArg({ nBest: 3, beam: undefined, reweight: null }), '{"nBest":3,"reweight":null}', "fields");
});

test("a config that is not an object, or holds a number JSON cannot carry, is refused", () => {
  const bad: unknown[] = [null, 3, "nBest", [], { maxWeight: Infinity }, { beam: -Infinity }, { reweight: { curve: Number.NaN } }, { nBest: 3n }];
  for (const config of bad) {
    assertEqual(thrownCode(FstError, () => configArg(config)), "invalid-argument", String(config));
  }
});

test("a config read back is frozen, its reweight too", () => {
  const config = parseConfig(JSON.stringify(defaultSpellerConfig));
  assert(Object.isFrozen(config) && Object.isFrozen(config.reweight), "frozen");
  assertEqual(JSON.stringify(config), JSON.stringify(defaultSpellerConfig), "the same fields");
  const none = parseConfig(JSON.stringify({ ...defaultSpellerConfig, reweight: null }));
  assertEqual(none.reweight, null, "no reweight");
});

await runTests("fst/config");
