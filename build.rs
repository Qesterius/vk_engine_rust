use anyhow::Result;
use shaderc;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=src/rendering/shaders/");

    let compiler = shaderc::Compiler::new().expect("Failed to find a shader compiler");
    let shader_dir = PathBuf::from("src/rendering/shaders");

    for entry in fs::read_dir(shader_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip if it's not a shader source file (we only want .vert, .frag, .glsl, etc.)
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("vert") | Some("frag") | Some("glsl")) {
            continue;
        }

        let shader_kind = match extension {
            Some("vert") => shaderc::ShaderKind::Vertex,
            Some("frag") => shaderc::ShaderKind::Fragment,
            _ => shaderc::ShaderKind::InferFromSource,
        };

        let source = fs::read_to_string(&path)?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap();

        let mut options = shaderc::CompileOptions::new().expect("Failed to create compile options");
        options.set_target_env(
            shaderc::TargetEnv::Vulkan,
            shaderc::EnvVersion::Vulkan1_0 as u32,
        ); // default is opengl which does not support descriptor binding of sampler and texture seperately

        let binary_result =
            compiler.compile_into_spirv(&source, shader_kind, file_name, "main", Some(&options))?;

        let spv_name = format!("{}.spv", extension.unwrap());
        let output_path = path.parent().unwrap().join(spv_name);
        fs::write(output_path, binary_result.as_binary_u8())?;

        println!("cargo:warning=Compiled shader: {}", file_name);
    }

    Ok(())
}
