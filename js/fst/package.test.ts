import { entryOf, packageTests } from "../test/packages.ts";
import { assertEqual, runTests, test } from "../test/harness.ts";

const dir = new URL("./", import.meta.url);
packageTests(dir, "@divvun/fst");

test("the entry module exports the API", async () => {
  const names = Object.keys(await entryOf(dir)).sort();
  assertEqual(
    names.join(" "),
    "FstError Speller Transducer defaultSpellerConfig init readSpeller readTransducer setLogLevel setLogger version",
    "exports",
  );
});

await runTests("fst/package");
