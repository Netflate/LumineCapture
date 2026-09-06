use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_OCR_GPU").is_none() {
        return;
    }
    // The GPU dist links libwebgpu_dawn.so dynamically and the binary does not
    // start without it. Look next to the binary first, so a packaged copy works,
    // then in the ort cache this build took the library from.
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    match dawn_dir() {
        Some(dir) => println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display()),
        None => println!(
            "cargo:warning=libwebgpu_dawn.so not found in the ort cache; \
             an installed binary may need LD_LIBRARY_PATH"
        ),
    }
}

fn dawn_dir() -> Option<PathBuf> {
    let target = std::env::var("TARGET").ok()?;
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let dists = cache.join("ort.pyke.io").join("dfbin").join(target);
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(dists).ok()?.filter_map(Result::ok) {
        let dir = entry.path();
        let library = dir.join("libwebgpu_dawn.so");
        let Ok(stamp) = fs::metadata(&library).and_then(|meta| meta.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(before, _)| stamp > *before) {
            newest = Some((stamp, dir));
        }
    }
    newest.map(|(_, dir)| dir)
}
