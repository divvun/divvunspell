#!/usr/bin/env bash
# Build the accuracy viewer and print what to copy into jekyll-theme-giellalt.
#
# Deployment is manual: run this, then copy the listed paths into a checkout
# of giellalt/jekyll-theme-giellalt at assets/typosreport/ (overwriting what's
# there), and commit. See README.md for details.
set -euo pipefail
cd "$(dirname "$0")"

trunk build --release

echo
echo "Built. Copy these into jekyll-theme-giellalt's assets/typosreport/ (replacing what's there):"
echo
for f in accuracy-viewer.js accuracy-viewer_bg.wasm styles.css global.css; do
	echo "  dist/$f"
done
echo "  dist/snippets/   (whole directory — accuracy-viewer.js imports these relative to itself)"
echo
echo "dist/index.html and dist/speller-accuracy.json (if present) are local-dev-only; don't copy those."
