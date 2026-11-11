//! Compiles tests/c/test_api.c against include/chromagrain.h and the
//! staticlib with the system C compiler, runs it, and requires exit 0.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn c_program_links_and_passes() {
    if Command::new("cc").arg("--version").output().is_err() {
        eprintln!("skipping: no C compiler");
        return;
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // target/<profile>/deps/c_api-<hash> -> target/<profile>
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let release = profile_dir.file_name().is_some_and(|n| n == "release");

    // The test harness builds the rlib; make sure the staticlib exists too.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut build = Command::new(cargo);
    build.args(["build", "-p", "chromagrain-ffi", "--lib"]).current_dir(&crate_dir);
    if release {
        build.arg("--release");
    }
    let status = build.status().expect("run cargo build");
    assert!(status.success(), "building the staticlib failed");
    let lib = profile_dir.join("libchromagrain_ffi.a");
    assert!(lib.exists(), "{lib:?} missing");

    let bin = profile_dir.join("chromagrain_ffi_c_test");
    let mut cc = Command::new("cc");
    cc.args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O1"])
        .arg("-I")
        .arg(crate_dir.join("include"))
        .arg(crate_dir.join("tests/c/test_api.c"))
        .arg(&lib)
        .arg("-o")
        .arg(&bin);
    // `cargo rustc -p chromagrain-ffi --lib -- --print native-static-libs`
    if cfg!(target_os = "macos") {
        cc.args(["-liconv", "-lSystem", "-lc", "-lm", "-lpthread"]);
    } else {
        cc.args(["-lpthread", "-ldl", "-lc", "-lm"]);
    }
    let out = cc.output().expect("run cc");
    assert!(out.status.success(), "C compile/link failed:\n{}", String::from_utf8_lossy(&out.stderr));

    let run = Command::new(&bin).output().expect("run the C test");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "C test failed:\n{stdout}\n{stderr}");
    assert!(stdout.contains("all checks passed"), "{stdout}");
}
