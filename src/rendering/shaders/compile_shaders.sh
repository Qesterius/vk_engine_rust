#!/bin/bash
GLS_LANG_BIN="glslangValidator"

$GLS_LANG_BIN -V src/rendering/shaders/shader.vert -o src/rendering/shaders/vert.spv
$GLS_LANG_BIN -V src/rendering/shaders/shader.frag -o src/rendering/shaders/frag.spv

echo "Shaders compiled successfully!"