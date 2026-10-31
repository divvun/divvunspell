// Starting the package: what importing does, the init sources, and calls made
// too early. `variant` names the source this process starts the module from.

import {
  assert,
  assertEqual,
  loadPackage,
  readBytes,
  rejectedCode,
  runTests,
  scenarioArgs,
  test,
  thrownCode,
} from "./harness.ts";

const args = scenarioArgs();

let fetches = 0;
let compiles = 0;
const realFetch = globalThis.fetch;
globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  fetches += 1;
  return realFetch(input, init);
}) as typeof fetch;
for (const name of ["compile", "compileStreaming", "instantiate", "instantiateStreaming"]) {
  const real: unknown = Reflect.get(WebAssembly, name);
  assert(typeof real === "function", `WebAssembly.${name} is a function`);
  Reflect.set(WebAssembly, name, (...rest: unknown[]) => {
    compiles += 1;
    return Reflect.apply(real, WebAssembly, rest);
  });
}

const pkg = await loadPackage(args);
let started: Promise<void> | undefined;

async function source(variant: string): Promise<Parameters<typeof pkg.init>[0]> {
  switch (variant) {
    case "bytes":
    case "retry":
      return await readBytes(args.wasm);
    case "file-url":
      return new URL(args.wasm);
    case "http-url":
      return args.http;
    case "response":
      return await realFetch(args.http);
    case "module":
      return await WebAssembly.compile(await readBytes(args.wasm));
    case "default":
      return undefined;
    default:
      throw new Error(`no init source called ${variant}`);
  }
}

test("importing the package fetches and compiles nothing", () => {
  assertEqual(fetches, 0, "fetches during import");
  assertEqual(compiles, 0, "compiles during import");
});

test("a call before init throws not-initialised", () => {
  assertEqual(thrownCode(pkg.FstError, () => pkg.version()), "not-initialised", "version() before init");
});

if (args.variant === "retry") {
  test("a failed init rejects with init-failed, and the next init starts afresh", async () => {
    const failing = pkg.init(new Uint8Array([0, 0x61, 0x73, 0x6d, 9, 9, 9, 9]));
    assertEqual(await rejectedCode(pkg.FstError, failing), "init-failed", "init with bad bytes");
    assertEqual(thrownCode(pkg.FstError, () => pkg.version()), "not-initialised", "version() after it");
  });
}

test(`init from ${args.variant} starts the package`, async () => {
  started = pkg.init(await source(args.variant));
  assertEqual(thrownCode(pkg.FstError, () => pkg.version()), "not-initialised", "a call while init runs");
  await started;
  assertEqual(pkg.version(), args.version, "version()");
});

test("init again returns the first call's promise and instance", async () => {
  const again = pkg.init(new Uint8Array(0));
  assert(again === started, "a second init returns the same promise");
  await again;
  assertEqual(pkg.version(), args.version, "version() after the second init");
});

await runTests(args.label);
