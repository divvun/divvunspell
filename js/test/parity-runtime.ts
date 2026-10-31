// The parity scenario (parity.ts) under Node, Bun and Deno, in a process of
// its own. run.mjs passes the scenario arguments as JSON: Node and Bun run
// this from its scenario list, and Deno with read access to the package and
// the fixtures alone, which is all the scenario needs.

import { readBytes } from "./harness.ts";
import { report, runParity } from "./parity.ts";
import type { ParityArgs } from "./parity.ts";

const deno: unknown = Reflect.get(globalThis, "Deno");

/** The runtime this process is, by the global each one alone defines. */
const runtime = deno !== undefined ? "deno" : Reflect.has(globalThis, "Bun") ? "bun" : "node";

/** Deno hands a script its own arguments; Node and Bun put the runtime and script paths first. */
function scenarioArgs(): ParityArgs & { label: string } {
  const argv: unknown =
    typeof deno === "object" && deno !== null
      ? Reflect.get(deno, "args")
      : Reflect.get(Reflect.get(globalThis, "process"), "argv");
  const own = runtime === "deno" ? argv : Array.isArray(argv) ? argv.slice(2) : undefined;
  if (!Array.isArray(own) || typeof own[0] !== "string") {
    throw new Error("the parity scenario takes its arguments as JSON in its first argument");
  }
  return JSON.parse(own[0]);
}

const args = scenarioArgs();
const label = `${runtime} ${args.label}`;
const { lines, passed } = report(label, await runParity(args, readBytes));
for (const line of lines) console.log(line);
if (!passed) throw new Error(`${label}: the package's results differ from native divvun-fst's`);
