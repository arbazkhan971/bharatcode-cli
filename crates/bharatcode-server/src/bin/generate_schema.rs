use bharatcode_server::openapi;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let schema = openapi::generate_schema();

    let package_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let output_path = env::var_os("BHARATCODE_OPENAPI_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(package_dir)
                .join("ui")
                .join("desktop")
                .join("openapi.json")
        });

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    fs::write(&output_path, format!("{schema}\n")).unwrap();
    eprintln!(
        "Successfully generated OpenAPI schema at {}",
        output_path.canonicalize().unwrap().display()
    );
    println!("{}", schema);
}
