// Checks the npm package must pass, run against its built package directory.

import { assert, assertEqual, readBytes, test } from "./harness.ts";

/** The fields of package.json these checks read. */
interface Manifest {
  name: string;
  version: string;
  description: string;
  license: string;
  type: string;
  files: string[];
  repository: { url: string; directory: string };
  exports: { ".": { types: string; default: string } };
}

interface NodeFs {
  readdir(path: URL, options: { recursive: true }): Promise<string[]>;
}

async function readManifest(dir: URL): Promise<Manifest> {
  const bytes = await readBytes(new URL("package.json", dir).href);
  return JSON.parse(new TextDecoder().decode(bytes));
}

async function listFiles(dir: URL): Promise<string[]> {
  const specifier = "node:fs/promises";
  const fs: NodeFs = await import(specifier);
  return await fs.readdir(dir, { recursive: true });
}

/** The entry module of the built package in `dir`. */
export async function entryOf(dir: URL): Promise<Record<string, unknown>> {
  const manifest = await readManifest(dir);
  return await import(new URL(manifest.exports["."].default, dir).href);
}

/** Registers the checks for the package in `dir`, which must be called `name`. */
export function packageTests(dir: URL, name: string): void {
  test("package.json names the package, its version, licence and repository", async () => {
    const manifest = await readManifest(dir);
    assertEqual(manifest.name, name, "name");
    assert(/^\d+\.\d+\.\d+$/.test(manifest.version), `a release version: ${manifest.version}`);
    assertEqual(manifest.license, "MIT OR Apache-2.0", "license");
    assertEqual(manifest.type, "module", "type");
    assertEqual(manifest.repository.url, "git+https://github.com/divvun/divvunspell.git", "repository");
    assertEqual(manifest.repository.directory, "js/fst", "repository directory");
    assert(manifest.description.includes("divvunspell"), "the description names divvunspell");
    assertEqual(manifest.files.join(" "), "dist", "files");
  });

  test("the entry module and its declarations are where the exports map says", async () => {
    const manifest = await readManifest(dir);
    const files = await listFiles(dir);
    const entry = manifest.exports["."];
    for (const path of [entry.default, entry.types]) {
      assert(files.includes(path.replace(/^\.\//, "")), `${path} exists`);
    }
  });

  test("one .wasm file ships, beside the entry module, and the licences with it", async () => {
    const manifest = await readManifest(dir);
    const shipped = await listFiles(new URL("dist/", dir));
    const wasm = shipped.filter((file) => file.endsWith(".wasm"));
    assertEqual(wasm.length, 1, ".wasm files");
    const entryDir = manifest.exports["."].default.replace(/^\.\/dist\//, "").replace(/[^/]*$/, "");
    assert(wasm[0]?.startsWith(entryDir) === true, `${String(wasm[0])} sits under ${entryDir}`);
    for (const licence of ["LICENSE-MIT", "LICENSE-APACHE"]) {
      assert(shipped.includes(licence), `dist/${licence} ships`);
    }
  });
}
