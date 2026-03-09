#!/usr/bin/env bash
set -euo pipefail

version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
tag="v${version}"

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "Error: tag $tag already exists"
  exit 1
fi

echo "Releasing $tag ..."
git tag "$tag"
git push origin main "$tag"
echo "Done! GitHub Actions will build and publish the release."
