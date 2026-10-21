#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
for stage in vert frag; do
    shader="$root/src/bin/dome-xr/shaders/panel.$stage"
    glslangValidator -V "$shader" -o "$shader.spv"
done
