use shaderc;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;

fn main() -> Result<()> {
    // 1. Tell Cargo to rerun this script if any file in the shaders folder changes
    println!("cargo:rerun-if-changed=src/rendering/shaders/");

    let compiler = shaderc::Compiler::new().expect("Failed to find a shader compiler");
    
    // 2. Define our paths
    let shader_dir = PathBuf::from("src/rendering/shaders");
    
    // 3. Iterate over files in the shader directory
    for entry in fs::read_dir(shader_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        // Skip if it's not a shader source file (we only want .vert, .frag, .glsl, etc.)
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("vert") | Some("frag") | Some("glsl")) {
            continue;
        }

        // 4. Determine shader kind
        let shader_kind = match extension {
            Some("vert") => shaderc::ShaderKind::Vertex,
            Some("frag") => shaderc::ShaderKind::Fragment,
            _ => shaderc::ShaderKind::InferFromSource,
        };

        // 5. Read source and compile
        let source = fs::read_to_string(&path)?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap();
        
        let binary_result = compiler.compile_into_spirv(
            &source,
            shader_kind,
            file_name,
            "main",
            None,
        )?;

        // 6. Save the .spv file in the same directory (or a target dir)
        let output_path = path.with_extension("spv");
        fs::write(output_path, binary_result.as_binary_u8())?;
        
        println!("cargo:warning=Compiled shader: {}", file_name);
    }

    Ok(())
}