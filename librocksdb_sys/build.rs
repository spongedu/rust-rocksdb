
extern crate cc;

use cc::Build;
use std::{fs, str};
use std::env::{self, VarError};
use std::path::{Path, PathBuf};
use std::process::Command;

fn get_env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={}", name);
    match env::var(name) {
        Ok(s) => Some(s),
        Err(VarError::NotPresent) => None,
        Err(VarError::NotUnicode(s)) => {
            panic!("unrecognize env var of {}: {:?}", name, s.to_string_lossy());
        }
    }
}

macro_rules! t {
    ($e:expr) => (match $e {
        Ok(n) => n,
        Err(e) => panic!("\n{} failed with {}\n", stringify!($e), e),
    })
}

fn main() {
    let mut cfg = build_rocksdb();

    cfg.cpp(true).file("crocksdb/c.cc");
    if !cfg!(target_os = "windows") {
        cfg.flag("-std=c++11");
    }
    cfg.compile("libcrocksdb.a");

    println!("cargo:rustc-link-lib=static=crocksdb");
}

const COMPRESS_LIBS: &'static [(&'static str, &'static str)] = &[
    ("SNAPPY", "snappy"),
    ("ZLIB", "z"),
    ("BZIP2", "bz2"),
    ("LZ4", "lz4"),
    ("ZSTD", "zstd"),
];

fn is_lib_disabled(name: &str) -> bool {
    get_env(&format!("ROCKSDB_DISABLE_{}", name)).is_some()
}

fn build_rocksdb() -> Build {
    let mut cfg = Build::new();

    for &(name, _) in COMPRESS_LIBS {
        if !is_lib_disabled(name) {
            cfg.define(name, None);
        }
    }

    if get_env("ROCKSDB_STATIC").is_none() {
        if cfg!(target_os = "windows") {
            println!("cargo:rustc-link-lib=rocksdb-shared");
        } else {
            println!("cargo:rustc-link-lib=rocksdb");
        }
        return cfg;
    }

    println!("cargo:rustc-link-lib=static=rocksdb");
    for &(name, lib) in COMPRESS_LIBS {
        if !is_lib_disabled(name) {
            println!("cargo:rust-link-lib=static={}", lib);
        }
    }

    if !cfg!(target_os = "linux") && !cfg!(target_os = "macos") {
        // Compilation is not tested in other platform, so hopefully
        // the static library is built already.
        return cfg;
    }

    let dst = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let build = dst.join("build");
    t!(fs::create_dir_all(&build));

    let fest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let p = PathBuf::from(fest_dir.clone()).join("build.sh");
    for &(name, lib) in COMPRESS_LIBS {
        if is_lib_disabled(name) {
            continue;
        }

        build_lib(p.as_path(), &lib, build.as_path(), dst.as_path());
    }

    println!("cargo:rustc-link-search=native={}", dst.display());
    cfg.include(dst.join("build").join("rocksdb").join("include"));

    let mut cpp_linked = false;
    let std_lib_name = if cfg!(target_os = "linux") {
        "libstdc++.a"
    } else {
        "libc++.a"
    };
    let short_std_lib_name = &std_lib_name[3..std_lib_name.len() - 2];
    if let Ok(libs) = env::var("ROCKSDB_OTHER_STATIC") {
        for lib in libs.split(":") {
            if lib == short_std_lib_name {
                cpp_linked = true;
            }
            println!("cargo:rustc-link-lib=static={}", lib);
        }
        if let Ok(pathes) = env::var("ROCKSDB_OTHER_STATIC_PATH") {
            for p in pathes.split(":") {
                println!("cargo:rustc-link-search=native={}", p);
            }
        }
    }
    if cpp_linked {
        cfg.cpp_link_stdlib(None);
        return cfg;
    }

    let output = Command::new(p.as_path())
        .args(&["find_library", std_lib_name])
        .output()
        .unwrap();
    if output.status.success() && !output.stdout.is_empty() {
        if let Ok(path_str) = str::from_utf8(&output.stdout) {
            let path = PathBuf::from(path_str);
            if path.is_absolute() {
                println!("cargo:rustc-link-lib=static=stdc++");
                println!(
                    "cargo:rustc-link-search=native={}",
                    path.parent().unwrap().display()
                );
                cfg.cpp_link_stdlib(None);
                return cfg;
            }
        }
    }
    println!(
        "failed to detect {}: {:?}, fallback to dynamic",
        std_lib_name,
        output
    );
    cfg

}

fn build_lib(build_tool: &Path, lib: &str, build: &Path, dst: &Path) {
    let lib_name = format!("lib{}.a", lib);
    let src = build.join(&lib_name);
    let dst = dst.join(&lib_name);

    if dst.exists() && lib != "rocksdb" {
        return;
    }

    if lib == "rocksdb" && src.exists() {
        fs::remove_dir_all(&src).unwrap();
        if dst.exists() {
            fs::remove_file(&dst).unwrap();
        }
    }

    if !src.exists() {
        let mut cmd = Command::new(build_tool);
        cmd.current_dir(&build).args(&[format!("compile_{}", lib)]);
        if lib == "rocksdb" {
            if let Some(v) = get_env("ROCKSDB_PORTABLE") {
                cmd.env("PORTABLE", v);
            }

            if let Some(v) = get_env("ROCKSDB_USE_SSE") {
                cmd.env("USE_SSE", v);
            }
        }
        run(&mut cmd);
    }

    if let Err(e) = fs::rename(src.as_path(), dst.as_path()) {
        panic!(
            "failed to move {} to {}: {:?}",
            src.display(),
            dst.display(),
            e
        );
    }
}

fn run(cmd: &mut Command) {
    println!("running: {:?}", cmd);
    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => panic!("{:?} failed: {}", cmd, e),
    };
    if !status.success() {
        panic!("{:?} failed: {}", cmd, status);
    }
}
