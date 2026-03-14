glslangValidator.exe -V shaders/shader.vert -o shaders/vert.spv
glslangValidator.exe -V shaders/shader.frag -o shaders/frag.spv
Write-Host "Shaders compiled!" -ForegroundColor Green