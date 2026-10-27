#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
for name in panel backdrop; do
for stage in vert frag; do
    shader="$root/src/bin/dome-xr/shaders/$name.$stage"
    glslangValidator -V "$shader" -o "$shader.spv"
done
done
glslangValidator -V "$root/src/bin/dome-xr/shaders/particles.vert" -o "$root/src/bin/dome-xr/shaders/particles.vert.spv"
