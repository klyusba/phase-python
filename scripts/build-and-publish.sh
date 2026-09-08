#!/usr/bin/env bash
# Build phase-python wheels for macOS arm64 and Linux (amd64 + arm64), then
# publish them to PyPI.
#
# macOS wheels cannot be produced inside Linux Docker images, so
# aarch64-apple-darwin is built on the host. linux/amd64 and linux/arm64 are
# built with the official maturin manylinux images via Docker.
#
# Usage:
#   export $(cat .env | xargs) &&
#   ./scripts/build-and-publish.sh
#
# Options:
#   --build-only     Build wheels, do not upload
#   --publish-only   Upload existing files in dist/ (skip builds)
#   --skip-macos     Skip the native aarch64-apple-darwin wheel
#   --skip-linux     Skip Docker Linux wheels
#   --test-pypi      Upload to TestPyPI instead of PyPI
#   -h, --help       Show this help
#
# Environment:
#   PYPI_TOKEN / UV_PUBLISH_TOKEN / MATURIN_PYPI_TOKEN
#                     PyPI API token (required unless --build-only)
#   MATURIN_IMAGE     Docker image (default: ghcr.io/pyo3/maturin:v1.15.0)
#   CARGO_BUILD_JOBS  Parallel rustc jobs inside Docker (default: 1)
#   INTERPRETERS      Extra maturin interpreter args (default: --find-interpreter)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${ROOT}/dist"
MATURIN_IMAGE="${MATURIN_IMAGE:-ghcr.io/pyo3/maturin:v1.15.0}"
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
INTERPRETERS=""
# INTERPRETERS="${INTERPRETERS:---find-interpreter}"

BUILD=1
PUBLISH=1
SKIP_MACOS=0
SKIP_LINUX=0
TEST_PYPI=0

usage() {
  sed -n '2,28p' "$0" | sed 's/^# \?//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-only) BUILD=1; PUBLISH=0 ;;
    --publish-only) BUILD=0; PUBLISH=1 ;;
    --skip-macos) SKIP_MACOS=1 ;;
    --skip-linux) SKIP_LINUX=1 ;;
    --test-pypi) TEST_PYPI=1 ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

pypi_token() {
  printf '%s' "${PYPI_TOKEN:-${UV_PUBLISH_TOKEN:-${MATURIN_PYPI_TOKEN:-}}}"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

host_maturin() {
  if command -v uv >/dev/null 2>&1; then
    uvx --from 'maturin>=1.8,<2' maturin "$@"
  else
    require_cmd maturin
    maturin "$@"
  fi
}

linux_wheel() {
  local platform="$1"
  echo "==> Linux wheel (${platform}) via ${MATURIN_IMAGE}"

  # Keep Cargo's target dir inside the container so macOS and Linux artifacts
  # never share a target/ tree. Named volumes cache crates.io / git deps.
  docker run --rm \
    --platform "${platform}" \
    -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS}" \
    -e CARGO_TARGET_DIR=/tmp/phase-python-target \
    -e RUST_MIN_STACK=16777216 \
    -v "${ROOT}:/io" \
    -v phase-python-cargo-registry:/root/.cargo/registry \
    -v phase-python-cargo-git:/root/.cargo/git \
    -w /io \
    "${MATURIN_IMAGE}" \
    build --release --out /io/dist ${INTERPRETERS}
}

if [[ "${PUBLISH}" -eq 1 && -z "$(pypi_token)" ]]; then
  echo "set PYPI_TOKEN (or UV_PUBLISH_TOKEN / MATURIN_PYPI_TOKEN) before publishing" >&2
  echo "or pass --build-only" >&2
  exit 1
fi

if [[ "${BUILD}" -eq 1 ]]; then
  if [[ "${SKIP_MACOS}" -eq 0 ]]; then
    require_cmd rustup
    echo "==> macOS wheel (aarch64-apple-darwin) on host"
    rustup target add aarch64-apple-darwin
    mkdir -p "${DIST}"
    (
      cd "${ROOT}"
      host_maturin build --release --target aarch64-apple-darwin --out dist ${INTERPRETERS}
    )
  fi

  if [[ "${SKIP_LINUX}" -eq 0 ]]; then
    require_cmd docker
    if ! docker info >/dev/null 2>&1; then
      echo "docker is not running" >&2
      exit 1
    fi
    mkdir -p "${DIST}"
    linux_wheel linux/arm64
    linux_wheel linux/amd64
  fi

  echo "==> sdist"
  (
    cd "${ROOT}"
    host_maturin sdist --out dist
  )

  echo "==> artifacts in ${DIST}"
  ls -lh "${DIST}"
fi

if [[ "${PUBLISH}" -eq 0 ]]; then
  exit 0
fi

if [[ ! -d "${DIST}" ]] || [[ -z "$(ls -A "${DIST}" 2>/dev/null)" ]]; then
  echo "nothing to publish in ${DIST}" >&2
  exit 1
fi

export UV_PUBLISH_TOKEN
UV_PUBLISH_TOKEN="$(pypi_token)"

if [[ "${TEST_PYPI}" -eq 1 ]]; then
  uv publish --index testpypi "${DIST}"/*
else
  uv publish "${DIST}"/*
fi

echo "done"
