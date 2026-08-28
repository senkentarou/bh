#!/usr/bin/env bash
set -euo pipefail

repo="senkentarou/bh"
tap_repo="senkentarou/homebrew-tap"
target="aarch64-apple-darwin"
archive="bh-${target}"

version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
tag="v${version}"

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "Error: tag $tag already exists"
  exit 1
fi

# A dirty tree would make the tag point at something other than what gets built.
if [ -n "$(git status --porcelain)" ]; then
  echo "Error: working tree is dirty"
  exit 1
fi

echo "Building $tag ..."
cargo build --release --target "$target"

# Stage inside target/ so the archive never litters the repository (/target is
# already ignored). The tarball holds a single top-level directory, which is
# what lets the Homebrew formula get away with a bare `bin.install "bh"`.
staging="target/${archive}"
tarball="target/${archive}.tar.gz"
rm -rf "$staging" "$tarball"
mkdir -p "$staging"
cp "target/${target}/release/bh" "$staging/"
cp README.md "$staging/"
tar -C target -czf "$tarball" "$archive"

echo "Publishing $tag ..."
git tag "$tag"
git push origin main "$tag"
gh release create "$tag" --title "$tag" --generate-notes "$tarball"

# The formula is generated rather than patched in place: it keeps the version,
# URL and checksum from ever drifting apart, and lets the tap start out empty.
echo "Updating $tap_repo ..."
sha=$(shasum -a 256 "$tarball" | cut -d ' ' -f 1)
tap_dir=$(mktemp -d)
trap 'rm -rf "$tap_dir"' EXIT
gh repo clone "$tap_repo" "$tap_dir" -- --depth 1
mkdir -p "${tap_dir}/Formula"
cat > "${tap_dir}/Formula/bh.rb" <<EOF
class Bh < Formula
  desc "Fast, interactive bash history search with fuzzy matching and smart ranking"
  homepage "https://github.com/${repo}"
  url "https://github.com/${repo}/releases/download/${tag}/${archive}.tar.gz"
  version "${version}"
  sha256 "${sha}"
  license "MIT"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "bh"
  end

  test do
    assert_match "bh #{version}", shell_output("#{bin}/bh --version")
  end
end
EOF
git -C "$tap_dir" add Formula/bh.rb
git -C "$tap_dir" commit -m "bh ${version}"
git -C "$tap_dir" push

echo "Done! ${tag} published and ${tap_repo} updated."
