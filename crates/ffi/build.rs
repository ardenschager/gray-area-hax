//! Generates `include/chromagrain.h` from the `extern "C"` API with cbindgen.
//! The header is checked in (Swift/Xcode consume it); it is only rewritten
//! when its contents change.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    let config = cbindgen::Config::from_file(format!("{dir}/cbindgen.toml")).expect("cbindgen.toml");
    let bindings = cbindgen::Builder::new().with_crate(&dir).with_config(config).generate().expect("cbindgen");
    let mut out = Vec::new();
    bindings.write(&mut out);
    let path = std::path::Path::new(&dir).join("include/chromagrain.h");
    if std::fs::read(&path).ok().as_deref() != Some(&out[..]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, out).unwrap();
    }
}
