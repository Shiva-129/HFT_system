use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("strategies.rs");
    let mut f = fs::File::create(&dest_path).unwrap();

    let src_dir = Path::new("src");
    let mut strategies = Vec::new();

    println!("cargo:rerun-if-changed=src");

    if let Ok(entries) = fs::read_dir(src_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().to_string();
                        // Exclude lib.rs and mod.rs
                        if name != "lib" && name != "mod" {
                            strategies.push(name.to_uppercase());
                        }
                    }
                }
            }
        }
    }

    strategies.sort();

    let strategies_str = strategies
        .iter()
        .map(|s| format!("\"{}\"", s))
        .collect::<Vec<_>>()
        .join(", ");

    let content = format!(
        "pub const AVAILABLE_STRATEGIES: &[&str] = &[{}];",
        strategies_str
    );

    f.write_all(content.as_bytes()).unwrap();
}
