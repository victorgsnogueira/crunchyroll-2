//! Compiles the Slint UI. If the official brand fonts were converted locally into
//! `assets/fonts/private/` (not in git), they are embedded; otherwise Lato is used.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fonts = root.join("assets/fonts");
    let atyp = fonts.join("private/CrunchyrollAtyp-Bold.ttf");
    println!("cargo:rerun-if-changed={}", atyp.display());

    let slash = |p: PathBuf| p.canonicalize().unwrap().display().to_string().replace('\\', "/").trim_start_matches("//?/").to_string();
    let mut src = String::new();
    for f in ["Lato-Regular.ttf", "Lato-Bold.ttf", "Lato-Black.ttf"] {
        src.push_str(&format!("import \"{}\";\n", slash(fonts.join(f))));
    }
    let display = if atyp.exists() {
        src.push_str(&format!("import \"{}\";\n", slash(atyp)));
        "Crunchyroll Atyp"
    } else {
        "Lato"
    };
    src.push_str(&format!(
        "export global BrandFonts {{ out property <string> display: \"{display}\"; }}\n"
    ));
    std::fs::write(out.join("brand_fonts.slint"), src).unwrap();

    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![out.clone()])
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/main.slint", config).unwrap();
}
