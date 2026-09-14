# Accuracy viewer

A web viewer for `divvunspell` accuracy reports, written in Rust with
[Dioxus](https://dioxuslabs.com/) and built to WebAssembly with
[Trunk](https://trunkrs.dev/). It is a static site — no Node toolchain — that
fetches a `speller-accuracy.json` served alongside it and renders the speller
configuration, performance/classification/suggestion statistics, and a sortable,
colour-coded results table.

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk          # or: brew install trunk
```

This crate is intentionally **outside** the main `divvunspell` Cargo workspace
(it has its own `[workspace]` table), so building it never interferes with the
native library/CLI build.

## Generate a report

```bash
# from the divvunspell repo root
cargo run -p divvunspell-cli --features accuracy -- \
    accuracy -o speller-accuracy.json typos.tsv path/to/language.bhfst
```

`typos.tsv` is a tab-separated `input<TAB>expected` list; rows with an empty
`expected` column are treated as correct words (to measure false positives).
Add `-v` to include the per-suggestion weight breakdown (lexicon / mutator /
reweight) in the report.

## Develop

```bash
trunk serve --open
```

Place the `speller-accuracy.json` to view in `dist/` (Trunk serves that directory), or copy
it there after `trunk build`. The app fetches `speller-accuracy.json` relative to the page.

## Build for deployment

```bash
trunk build --release
```

Emits stable (non-hashed — see `Trunk.toml`) filenames to `dist/`. For local
testing, copy a `speller-accuracy.json` into `dist/` and serve/publish the directory.

### Standalone (e.g. GitHub Pages)

The app has no hard dependency on jekyll-theme-giellalt: `dist/` from `trunk
build --release` is a self-contained static site (its own `index.html` and
wasm bootstrap script), and `docs_data_base()` falls back to a same-origin
relative fetch when `window.__DOCS_DATA_BASE__` isn't set. When serving from
a project subpath (e.g. `https://<org>.github.io/<repo>/`), build with a
matching public URL so asset links resolve:

```bash
trunk build --release --public-url /<repo>/
```

Then publish `dist/` (with `speller-accuracy.json` inside it) to the Pages
branch/directory.

### Deploying to jekyll-theme-giellalt

For `lang-*` repos' docs sites specifically, this app is instead deployed via
[`giellalt/jekyll-theme-giellalt`](https://github.com/giellalt/jekyll-theme-giellalt)'s
`typosreport` layout, which supplies `window.__DOCS_DATA_BASE__` (the repo's
`generated/docs-data` branch, where CI publishes `speller-accuracy.json`) and the wasm
bootstrap script.

There's no CI wiring this up — the built output is a checked-in artifact in
the theme repo, same as the old Svelte bundle it replaced. After changing
this app:

```bash
./build.sh
```

then copy the paths it prints into a checkout of jekyll-theme-giellalt at
`assets/typosreport/` (replacing what's there — this includes the whole
`snippets/` directory, which `accuracy-viewer.js` imports relative to
itself), and commit there. `index.html` and `dist/speller-accuracy.json` are for local
`trunk serve`/`trunk build` testing only — don't copy those in.
