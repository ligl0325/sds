#!/usr/bin/env bash
set -euo pipefail

REPO="${SDS_REPO:-ligl0325/sds}"
VERSION="${SDS_VERSION:-latest}"
INSTALL_DIR="${SDS_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) ASSET="sds-linux-x86_64-gnu.tar.gz" ;;
  *)
    printf 'Unsupported platform: %s/%s\n' "$(uname -s)" "$(uname -m)" >&2
    exit 1
    ;;
esac

if [[ "$VERSION" == "latest" ]]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  TAG="$VERSION"
  [[ "$TAG" == v* ]] || TAG="v${TAG}"
  BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
fi

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

curl --fail --location --silent --show-error \
  "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}"
curl --fail --location --silent --show-error \
  "${BASE_URL}/${ASSET}.sha256" -o "${TMP_DIR}/${ASSET}.sha256"

EXPECTED=$(awk '{print $1}' "${TMP_DIR}/${ASSET}.sha256")
ACTUAL=$(sha256sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')
if [[ "$EXPECTED" != "$ACTUAL" ]]; then
  printf 'Checksum mismatch\nexpected: %s\nactual:   %s\n' "$EXPECTED" "$ACTUAL" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
install -m 0755 "${TMP_DIR}/sds" "${INSTALL_DIR}/sds"
install -m 0755 "${TMP_DIR}/sds-mcp" "${INSTALL_DIR}/sds-mcp"

printf 'Installed sds and sds-mcp to %s\n' "$INSTALL_DIR"
"${INSTALL_DIR}/sds" --version
