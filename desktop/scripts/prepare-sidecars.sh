#!/usr/bin/env bash
# Prepare the sidecar binaries Tauri bundles into the .app:
#   binaries/node-<triple>       official Node.js (Agent Ask runner)
#   binaries/pdftotext-<triple>  Homebrew poppler; dylibs relocated into
#                                binaries/libs/ with install names pointing at
#                                @executable_path/../Resources/libs/
# install_name_tool invalidates code signatures and unsigned arm64 binaries
# are killed on launch, so everything touched is re-signed ad-hoc.
set -euo pipefail
cd "$(dirname "$0")/.."

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
NODE_VERSION="${NODE_VERSION:-22.17.0}"
mkdir -p binaries/libs

echo "==> node v${NODE_VERSION} (${TRIPLE})"
curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-darwin-arm64.tar.gz" \
  -o /tmp/xuewen-node.tgz
tar -xzf /tmp/xuewen-node.tgz -C /tmp
cp "/tmp/node-v${NODE_VERSION}-darwin-arm64/bin/node" "binaries/node-${TRIPLE}"

echo "==> pdftotext from $(brew --prefix poppler)"
cp "$(brew --prefix poppler)/bin/pdftotext" "binaries/pdftotext-${TRIPLE}"
chmod u+w "binaries/pdftotext-${TRIPLE}"
dylibbundler -of -b \
  -x "binaries/pdftotext-${TRIPLE}" \
  -d binaries/libs \
  -p '@executable_path/../Resources/libs/'

echo "==> ad-hoc re-sign"
codesign --force -s - "binaries/node-${TRIPLE}" "binaries/pdftotext-${TRIPLE}"
for f in binaries/libs/*.dylib; do
  codesign --force -s - "$f"
done

echo "==> done:"
ls -l binaries binaries/libs
