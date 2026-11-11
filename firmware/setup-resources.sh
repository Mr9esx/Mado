#!/bin/sh
set -eu
cd "$(dirname "$0")"
mkdir -p vendor
if [ ! -e vendor/freetype ]; then
    git clone --depth 1 --branch VER-2-14-3 https://github.com/freetype/freetype.git vendor/freetype
fi
if [ ! -e vendor/brotli ]; then
    git clone --depth 1 --branch v1.1.0 https://github.com/google/brotli.git vendor/brotli
fi
