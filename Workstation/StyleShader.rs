//! Compile the small, resource-free style WGSL profile on the control thread.
//! User code is parsed independently before it can join the workstation module.
//! The existing generated gem shader remains the default on every backend.
use std::path::Path;

use naga::{Module, ScalarKind, ShaderStage, Statement, TypeInner, VectorSize};

const BEGIN: &str = "// USER_STYLE_FUNCTIONS";
const END: &str = "// END_USER_STYLE_FUNCTIONS";
const MAX_SOURCE_BYTES: usize = 65_536;
const MAX_FUNCTIONS: usize = 32;
const MAX_EXPRESSIONS: usize = 4_096;
const MAX_EXPANDED_COST: usize = 8_192;
const PROBE: &str = "\n@fragment fn wwStyleCompilerProbe() {\n let c=style_color(vec2<f32>(0.5),vec3<f32>(0.0),vec2<f32>(1.0),0.0);\n let b=style_background(vec2<f32>(0.5),0.0);\n}\n";

/// Validate an author shader without accessing any renderer, file, or device.
pub(crate) fn validate(source: &str) -> Result<(), String> {
    let _ = checked_user_module(source)?;
    // Portable packages must compile to the least capable supported frontends.
    let _ = glsl_library(source, false)?;
    let _ = glsl_library(source, true)?;
    Ok(())
}

/// Read a generated frontend and replace only its delimited default style hooks.
pub(crate) fn source(root: &Path, relative: &str) -> Result<String, String> {
    let path = root.join(relative);
    let original =
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !matches!(
        relative,
        "Procedures/Shaders/workstation.wgsl"
            | "Procedures/Shaders/workstation.gl.frag"
            | "Procedures/Shaders/workstation.ios.frag"
    ) {
        return Ok(original);
    }
    let Some(kit) = super::user_style::selected()? else {
        return Ok(original);
    };
    let Some(user) = kit.shader_source.as_deref() else {
        return Ok(original);
    };
    if relative.ends_with(".wgsl") {
        checked_user_module(user)?;
        let joined = replace_hooks(&original, user)?;
        // Detect collisions with the host namespace before passing code to a GPU.
        checked_module(&joined, "combined workstation WGSL")?;
        Ok(joined)
    } else {
        let library = glsl_library(user, relative.ends_with(".ios.frag"))?;
        replace_hooks(&original, &library)
    }
}

/// Vulkan keeps its existing vertex pipeline and embedded default SPIR-V. Only
/// a selected custom fragment program is compiled here, without an external CLI.
pub(crate) fn vulkan_fragment(root: &Path) -> Result<Option<Vec<u8>>, String> {
    let Some(kit) = super::user_style::selected()? else {
        return Ok(None);
    };
    if kit.shader_source.is_none() {
        return Ok(None);
    }
    let joined = source(root, "Procedures/Shaders/workstation.wgsl")?;
    compile_vulkan_fragment(&joined).map(Some)
}

fn compile_vulkan_fragment(source: &str) -> Result<Vec<u8>, String> {
    let (mut module, _) = checked_module(source, "combined workstation WGSL")?;
    // WebGPU has appearance=0, optics=1. Native Vulkan's descriptor ABI is the
    // reverse; the existing vertex module already reads appearance at binding 1.
    for (_, variable) in module.global_variables.iter_mut() {
        if let Some(binding) = &mut variable.binding {
            if binding.group == 0 && binding.binding < 2 {
                binding.binding = 1 - binding.binding;
            }
        }
    }
    let entry = module
        .entry_points
        .iter_mut()
        .find(|entry| entry.stage == ShaderStage::Fragment && entry.name == "fs_main")
        .ok_or("workstation WGSL has no fs_main fragment entry point")?;
    entry.name = "main".into();
    let info = validate_module(&module, "Vulkan workstation WGSL")?;
    let mut options = naga::back::spv::Options::default();
    options.fake_missing_bindings = false;
    for (_, variable) in module.global_variables.iter() {
        if let Some(binding) = &variable.binding {
            options.binding_map.insert(
                *binding,
                naga::back::spv::BindingInfo {
                    descriptor_set: binding.group,
                    binding: binding.binding,
                    binding_array_size: None,
                },
            );
        }
    }
    let pipeline = naga::back::spv::PipelineOptions {
        shader_stage: ShaderStage::Fragment,
        entry_point: "main".into(),
    };
    let words = naga::back::spv::write_vec(&module, &info, &options, Some(&pipeline))
        .map_err(|e| format!("custom style Vulkan compilation: {e}"))?;
    Ok(words.into_iter().flat_map(u32::to_le_bytes).collect())
}

fn replace_hooks(original: &str, replacement: &str) -> Result<String, String> {
    let start = original
        .find(BEGIN)
        .ok_or("workstation shader lacks style start marker")?;
    let after = start + BEGIN.len();
    let end = original[after..]
        .find(END)
        .map(|offset| after + offset)
        .ok_or("workstation shader lacks style end marker")?;
    if original[after..].contains(BEGIN) || original[end + END.len()..].contains(END) {
        return Err("workstation shader has duplicate style markers".into());
    }
    Ok(format!(
        "{}\n{}\n{}",
        &original[..after],
        replacement,
        &original[end..]
    ))
}

fn checked_module(source: &str, label: &str) -> Result<(Module, naga::valid::ModuleInfo), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| format!("{label}: {}", e.emit_to_string(source)))?;
    let info = validate_module(&module, label)?;
    Ok((module, info))
}

fn validate_module(module: &Module, label: &str) -> Result<naga::valid::ModuleInfo, String> {
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(module)
    .map_err(|e| format!("{label}: {e:?}"))
}

fn checked_user_module(source: &str) -> Result<Module, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!("style WGSL exceeds {MAX_SOURCE_BYTES} bytes"));
    }
    let (module, _) = checked_module(source, "style WGSL")?;
    if !module.entry_points.is_empty()
        || !module.global_variables.is_empty()
        || !module.overrides.is_empty()
    {
        return Err(
            "style WGSL cannot declare entry points, global variables, bindings, or overrides"
                .into(),
        );
    }
    if module.functions.len() > MAX_FUNCTIONS {
        return Err(format!("style WGSL exceeds {MAX_FUNCTIONS} functions"));
    }
    for (_, ty) in module.types.iter() {
        let scalar = match ty.inner {
            TypeInner::Scalar(scalar)
            | TypeInner::Vector { scalar, .. }
            | TypeInner::Matrix { scalar, .. } => scalar,
            _ => return Err("style WGSL v1 supports scalars, vectors and matrices; arrays, structs, pointers and resource types are not supported".into()),
        };
        if !(matches!(
            scalar.kind,
            ScalarKind::Float | ScalarKind::Sint | ScalarKind::Uint
        ) && scalar.width == 4
            || scalar.kind == ScalarKind::Bool)
        {
            return Err("style WGSL v1 supports f32, i32, u32 and bool values".into());
        }
    }
    let expressions = module.global_expressions.len()
        + module
            .functions
            .iter()
            .map(|(_, f)| f.expressions.len())
            .sum::<usize>();
    if expressions > MAX_EXPRESSIONS {
        return Err(format!("style WGSL exceeds {MAX_EXPRESSIONS} expressions"));
    }
    require_signature(
        &module,
        "style_color",
        &[
            Some(VectorSize::Bi),
            Some(VectorSize::Tri),
            Some(VectorSize::Bi),
            None,
        ],
        VectorSize::Quad,
    )?;
    require_signature(
        &module,
        "style_background",
        &[Some(VectorSize::Bi), None],
        VectorSize::Tri,
    )?;
    let mut costs = Vec::with_capacity(module.functions.len());
    for (_, function) in module.functions.iter() {
        if function
            .expressions
            .iter()
            .any(|(_, expression)| matches!(expression, naga::Expression::Derivative { .. }))
        {
            return Err("style WGSL v1 cannot use screen derivatives inside the ray-traced surface; use analytic derivatives instead".into());
        }
        // Naga validates a dependency-ordered function DAG, with no recursion.
        let cost = function.expressions.len() + block_cost(&function.body, &costs)?;
        if cost > MAX_EXPANDED_COST {
            return Err(format!(
                "style WGSL exceeds {MAX_EXPANDED_COST} expanded operations"
            ));
        }
        costs.push(cost);
    }
    Ok(module)
}

fn require_signature(
    module: &Module,
    name: &str,
    arguments: &[Option<VectorSize>],
    result: VectorSize,
) -> Result<(), String> {
    let function = module
        .functions
        .iter()
        .find_map(|(_, f)| (f.name.as_deref() == Some(name)).then_some(f))
        .ok_or_else(|| format!("style WGSL must define {name}"))?;
    let valid = function.arguments.len() == arguments.len()
        && function
            .arguments
            .iter()
            .zip(arguments)
            .all(|(argument, size)| float_type(&module.types[argument.ty].inner, *size))
        && function
            .result
            .as_ref()
            .is_some_and(|r| float_type(&module.types[r.ty].inner, Some(result)));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "style WGSL {name} has the wrong signature; see the style authoring contract"
        ))
    }
}

fn float_type(ty: &TypeInner, size: Option<VectorSize>) -> bool {
    match (ty, size) {
        (TypeInner::Scalar(scalar), None) => scalar.kind == ScalarKind::Float && scalar.width == 4,
        (TypeInner::Vector { size, scalar }, Some(expected)) => {
            *size == expected && scalar.kind == ScalarKind::Float && scalar.width == 4
        }
        _ => false,
    }
}

fn block_cost(block: &naga::Block, costs: &[usize]) -> Result<usize, String> {
    let mut cost = 0usize;
    for statement in block.iter() {
        cost += 1;
        cost += match statement {
            Statement::Block(block) => block_cost(block, costs)?,
            Statement::If { accept, reject, .. } => block_cost(accept, costs)? + block_cost(reject, costs)?,
            Statement::Switch { cases, .. } => {
                let mut total = 0;
                for case in cases { total += block_cost(&case.body, costs)?; }
                total
            }
            Statement::Call { function, .. } => *costs.get(function.index())
                .ok_or("style WGSL call graph is not dependency ordered")?,
            Statement::Loop { .. } => return Err("style WGSL v1 does not support loops; write bounded expressions or unroll a small fixed sequence".into()),
            Statement::Kill => return Err("style WGSL must return alpha for cutouts instead of discard".into()),
            _ => 0,
        };
        if cost > MAX_EXPANDED_COST {
            return Err(format!(
                "style WGSL exceeds {MAX_EXPANDED_COST} expanded operations"
            ));
        }
    }
    Ok(cost)
}

fn glsl_library(source: &str, es: bool) -> Result<String, String> {
    checked_user_module(source)?;
    let (mut module, _) = checked_module(&format!("{source}{PROBE}"), "style WGSL probe")?;
    let mut color = None;
    let mut background = None;
    // Rename IR symbols, never translate source text. Names end in letters so
    // Naga's GLSL namer preserves them exactly (numeric endings get a suffix).
    for (handle, function) in module.functions.iter_mut() {
        let name = format!("wwStyleFunction{}f", handle.index());
        match function.name.as_deref() {
            Some("style_color") => color = Some(name.clone()),
            Some("style_background") => background = Some(name.clone()),
            _ => (),
        }
        function.name = Some(name);
        for (index, argument) in function.arguments.iter_mut().enumerate() {
            argument.name = Some(format!("wwStyleArg{}x{}a", handle.index(), index));
        }
        for (local, variable) in function.local_variables.iter_mut() {
            variable.name = Some(format!("wwStyleLocal{}x{}v", handle.index(), local.index()));
        }
        for (expression, name) in function.named_expressions.iter_mut() {
            *name = format!("wwStyleValue{}x{}e", handle.index(), expression.index());
        }
    }
    for (handle, constant) in module.constants.iter_mut() {
        constant.name = Some(format!("wwStyleConstant{}c", handle.index()));
    }
    let types = module
        .types
        .iter()
        .map(|(handle, ty)| {
            let mut ty = ty.clone();
            ty.name = Some(format!("wwStyleType{}t", handle.index()));
            (handle, ty)
        })
        .collect::<Vec<_>>();
    for (handle, ty) in types {
        module.types.replace(handle, ty);
    }
    let info = validate_module(&module, "style GLSL module")?;
    let options = naga::back::glsl::Options {
        version: if es {
            naga::back::glsl::Version::new_gles(300)
        } else {
            naga::back::glsl::Version::Desktop(330)
        },
        writer_flags: naga::back::glsl::WriterFlags::empty(),
        ..Default::default()
    };
    let pipeline = naga::back::glsl::PipelineOptions {
        shader_stage: ShaderStage::Fragment,
        entry_point: "wwStyleCompilerProbe".into(),
        multiview: None,
    };
    let mut emitted = String::new();
    naga::back::glsl::Writer::new(
        &mut emitted,
        &module,
        &info,
        &options,
        &pipeline,
        Default::default(),
    )
    .map_err(|e| format!("style GLSL preparation: {e}"))?
    .write()
    .map_err(|e| format!("style GLSL translation: {e}"))?;
    // The compiler probe has no inputs, outputs, or resources, so the only
    // generated scaffolding is the version/precision header and final main.
    // All function bodies and constants come directly from Naga's writer.
    let end = emitted
        .rfind("void main()")
        .ok_or("style GLSL probe entry point is missing")?;
    let mut library = String::new();
    for line in emitted[..end].lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version ") || trimmed.starts_with("precision ") {
            continue;
        }
        if trimmed.starts_with('#') {
            return Err(
                "style WGSL requires a GLSL extension outside the portable v1 profile".into(),
            );
        }
        library.push_str(line);
        library.push('\n');
    }
    let color = color.ok_or("style_color was lost during GLSL translation")?;
    let background = background.ok_or("style_background was lost during GLSL translation")?;
    if !library.contains(&format!("vec4 {color}("))
        || !library.contains(&format!("vec3 {background}("))
    {
        return Err("style GLSL function names differ from the compiler contract".into());
    }
    library.push_str(&format!("\nvec4 style_color(vec2 uv, vec3 p, vec2 size, float time) {{ return {color}(uv,p,size,time); }}\nvec3 style_background(vec2 uv, float time) {{ return {background}(uv,time); }}\n"));
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SHADER: &str = "
fn grain(x:f32)->f32 {return sin(x*18.0)*0.08;}
fn style_color(uv:vec2<f32>,p:vec3<f32>,size:vec2<f32>,time:f32)->vec4<f32> {
 return vec4<f32>(vec3<f32>(0.3+grain(uv.y)),1.0);
}
fn style_background(uv:vec2<f32>,time:f32)->vec3<f32> {return vec3<f32>(0.1+uv.y*0.1);}
";

    #[test]
    fn compiles_helpers_to_both_glsl_profiles() {
        validate(SHADER).unwrap();
        for es in [false, true] {
            let library = glsl_library(SHADER, es).unwrap();
            assert!(library.contains("vec4 style_color("));
            assert!(library.contains("vec3 style_background("));
            assert!(library.contains("wwStyleFunction0f"));
            assert!(!library.contains("void main"));
            assert!(!library.contains("#version"));
        }
    }

    #[test]
    fn rejects_resources_loops_and_wrong_signatures() {
        assert!(
            validate(&format!("var<private> hidden:f32;{SHADER}"))
                .unwrap_err()
                .contains("global variables")
        );
        assert!(
            validate(&SHADER.replace("return sin(x*18.0)*0.08;", "loop {break;} return x;"))
                .unwrap_err()
                .contains("loops")
        );
        assert!(
            validate(&SHADER.replace("p:vec3<f32>", "p:vec2<f32>"))
                .unwrap_err()
                .contains("signature")
        );
        assert!(
            validate(&SHADER.replace("style_background", "missing_background"))
                .unwrap_err()
                .contains("must define style_background")
        );
        assert!(
            validate(&SHADER.replace("sin(x*18.0)*0.08", "dpdx(x)"))
                .unwrap_err()
                .contains("derivatives")
        );
    }

    #[test]
    fn rejects_exponential_helper_cost() {
        let mut source = "fn f0(x:f32)->f32{return x+1.0;}\n".to_owned();
        for i in 1..15 {
            source.push_str(&format!(
                "fn f{i}(x:f32)->f32{{return f{}(x)+f{}(x);}}\n",
                i - 1,
                i - 1
            ));
        }
        source.push_str(&SHADER.replace("sin(x*18.0)*0.08", "f14(x)"));
        assert!(
            validate(&source)
                .unwrap_err()
                .contains("expanded operations")
        );
    }

    #[test]
    fn replacement_preserves_surrounding_engine_code() {
        let original = format!("before\n{BEGIN}\nold\n{END}\nafter");
        let replaced = replace_hooks(&original, SHADER).unwrap();
        assert!(replaced.starts_with("before\n"));
        assert!(replaced.ends_with("\nafter"));
        assert!(!replaced.contains("\nold\n"));
        assert!(replace_hooks("no markers", SHADER).is_err());
    }

    #[test]
    fn custom_shader_joins_actual_workstation_and_emits_vulkan_fragment() {
        let original = include_str!("../Procedures/Shaders/workstation.wgsl");
        let joined = replace_hooks(original, SHADER).unwrap();
        checked_module(&joined, "test workstation WGSL").unwrap();
        let bytes = compile_vulkan_fragment(&joined).unwrap();
        let words = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(words[0], 0x0723_0203);
        let mut offset = 5;
        let mut fragment = false;
        while offset < words.len() {
            let count = (words[offset] >> 16) as usize;
            let opcode = words[offset] & 0xffff;
            assert!(count > 0 && offset + count <= words.len());
            if opcode == 15 {
                // OpEntryPoint: execution model, function id, name.
                assert_eq!(words[offset + 1], 4); // Fragment.
                assert_eq!(words[offset + 3], u32::from_le_bytes(*b"main"));
                fragment = true;
            }
            offset += count;
        }
        assert!(fragment);
    }
}
