//! Embeds every file under `web/` into the binary, so the workbench page can be split into as many
//! modules as it needs without listing them in Rust. Generates `$OUT_DIR/web_assets.rs`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let web = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("web");
    println!("cargo:rerun-if-changed={}", web.display());
    let mut files = Vec::new();
    collect(&web, &mut files);
    files.sort();
    let mut out = String::from(
        "/// (path relative to web/, bytes, content type)\npub static ASSETS: &[(&str, &[u8], &str)] = &[\n",
    );
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(&web)
            .expect("under web/")
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let Some(kind) = content_type(file) else {
            panic!(
                "web/{relative}: no content type for this extension; add one to build.rs or remove the file"
            );
        };
        out.push_str(&format!(
            "    ({relative:?}, include_bytes!({:?}), {kind:?}),\n",
            file.display().to_string()
        ));
    }
    out.push_str("];\n");
    let target = PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("web_assets.rs");
    fs::write(target, out).expect("write web_assets.rs");
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn content_type(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()? {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "md" | "txt" => "text/plain; charset=utf-8",
        _ => return None,
    })
}
