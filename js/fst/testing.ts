// The entry module of a test build: the package's API and its test hooks.
// js/build.mjs compiles it only with --test, into target/js-test, so the
// published package never contains it.

import { core } from "./binding.ts";
import { configArg, type PartialSpellerConfig, parseConfig, type SpellerConfig } from "./config.ts";
import type { Core } from "./core.ts";
import type { GlueObject } from "./glue.ts";
import { Handle } from "./handle.ts";
import { levelNumber, type LogEvent } from "./logging.ts";
import * as glue from "./pkg/divvun_fst_wasm.js";

export * from "./index.ts";

interface TestObjectGlue extends GlueObject {
  label(): string;
  panicWhileBorrowed(message: string): void;
}

/** The exports of `crates/divvun-fst-wasm/src/test_hooks.rs`. */
interface TestHooksGlue {
  TestObject: new (label: string) => TestObjectGlue;
  testPanic(message: string): void;
  testLog(level: number, message: string): void;
  testResolveConfig(json: string): string;
  testDefaultConfig(): string;
}

/** A small object that owns module memory. Freeing it logs `dropped <label>` at debug level. */
export class TestObject extends Handle<TestObjectGlue> {
  constructor(core: Core, glue: TestObjectGlue) {
    super(core, glue);
  }

  label(): string {
    return this.use((glue) => glue.label());
  }

  /** Panics inside a method, while the module holds a borrow of the object. */
  panicWhileBorrowed(message: string): void {
    this.use((glue) => glue.panicWhileBorrowed(message));
  }
}

export interface TestHooks {
  createObject(label: string): TestObject;
  /** Panics in the module, which traps it. */
  panic(message: string): void;
  /** Logs `message` from the module at `level`, through the library's tracing. */
  log(level: LogEvent["level"], message: string): void;
  /** The whole config a speller call passed `config` runs with, as the module reads it. */
  resolveConfig(config: PartialSpellerConfig): Readonly<SpellerConfig>;
  /** divvun-fst's built-in config, as the module reads it. */
  defaultConfig(): Readonly<SpellerConfig>;
}

function hasTestHooks(glue: object): glue is TestHooksGlue {
  return ["TestObject", "testPanic", "testLog", "testResolveConfig", "testDefaultConfig"].every(
    (name) => name in glue,
  );
}

function testHooks(core: Core, glue: object): TestHooks {
  if (!hasTestHooks(glue)) {
    throw new Error("this build has no test hooks: build it with node js/build.mjs --test");
  }
  return {
    createObject: (label) => new TestObject(core, core.call(() => new glue.TestObject(label))),
    panic: (message) => core.call(() => glue.testPanic(message)),
    log: (level, message) => core.call(() => glue.testLog(levelNumber(level), message)),
    resolveConfig: (config) => {
      const json = configArg(config) ?? "{}";
      return parseConfig(core.call(() => glue.testResolveConfig(json)));
    },
    defaultConfig: () => parseConfig(core.call(() => glue.testDefaultConfig())),
  };
}

export const testing: TestHooks = testHooks(core, glue);
