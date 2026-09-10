use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    str,
};
fn main() {
    let mut cmake_conf = cmake::Config::new("open_jtalk");
    let target = env::var("TARGET").unwrap();
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    let cmake_conf = if target.starts_with("i686") {
        let cmake_conf = cmake_conf.define("OPEN_JTALK_X86", "true");

        if target.contains("windows") {
            cmake_conf.define("CMAKE_GENERATOR_PLATFORM", "Win32")
        } else {
            cmake_conf
        }
    } else {
        &mut cmake_conf
    };

    let debug = env::var("DEBUG").is_ok();
    // open_jtalkのビルドprofileがdebugだとWindowsでリンクエラーになるため、Releaseにする
    if debug {
        cmake_conf.profile("Release");
    }

    // androidのminSdkを指定する
    if target.contains("android") {
        // nfkとcmake間でパスに問題があるため１にする
        cmake_conf.define("CMAKE_SYSTEM_VERSION", "1");
    }

    // CMAKE_OSX_DEPLOYMENT_TARGETを指定する
    if target.ends_with("apple-darwin") && env::var("MACOSX_DEPLOYMENT_TARGET").is_err() {
        let cmake_osx_deployment_target = Command::new(env::var("RUSTC").unwrap())
            .arg("--target")
            .arg(&target)
            .arg("--print=deployment-target")
            .output()
            .unwrap();
        let cmake_osx_deployment_target = str::from_utf8(&cmake_osx_deployment_target.stdout)
            .unwrap()
            .trim()
            .strip_prefix("MACOSX_DEPLOYMENT_TARGET=")
            .unwrap();
        cmake_conf.define("CMAKE_OSX_DEPLOYMENT_TARGET", cmake_osx_deployment_target);
    }

    // iOS SDKで必要な引数を指定する
    if target.contains("ios") {
        // iOSとiPhone simulatorは別扱いになる
        let sdk = if target.ends_with("sim") || target.starts_with("x86_64") {
            "iphonesimulator"
        } else {
            "iphoneos"
        };
        let cmake_osx_sysroot = Command::new("xcrun")
            .args(["--sdk", sdk, "--show-sdk-path"])
            .output()
            .expect("Failed to run xcrun command");
        let cmake_osx_sysroot = str::from_utf8(&cmake_osx_sysroot.stdout)
            .unwrap()
            .trim()
            .to_string();
        cmake_conf.define("CMAKE_OSX_SYSROOT", &cmake_osx_sysroot);
        // x86_64アーキテクチャのiPhoneシミュレータではC++のヘッダーのパスが通っていないので、通す
        if target.starts_with("x86_64") {
            let include_dir = PathBuf::from(&cmake_osx_sysroot)
                .join("usr")
                .join("include")
                .join("c++")
                .join("v1");
            include_dirs.push(include_dir);
        }
    }

    let dst_dir = cmake_conf.build();
    let lib_dir = dst_dir.join("lib");
    if target.ends_with("-apple-darwin") {
        let lib_dir_str = lib_dir.display();
        let script = format!(
            r#"
            find {lib_dir_str} -type f -exec sh -c '
            for f do
                echo "=== $f ==="
                otool -l "$f" 2>/dev/null |
                grep -8 -E "LC_BUILD_VERSION|LC_VERSION_MIN_MACOSX"
            done
            echo "done"
            ' sh {{}} +
        "#
        );
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .output()
            .expect("Failed to execute command");
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            println!("cargo::warning=stdout:{}", line);
        }
        for line in String::from_utf8_lossy(&output.stderr).lines() {
            println!("cargo::warning=stderr:{}", line);
        }
    } else {
        println!("cargo::warning=Not macOS target");
    }
    println!("cargo:rustc-link-search={}", lib_dir.to_str().unwrap());
    println!("cargo:rustc-link-lib=openjtalk");
    generate_bindings(dst_dir.join("include"), include_dirs);
}

#[cfg(not(feature = "generate-bindings"))]
#[allow(unused_variables)]
fn generate_bindings(
    allow_dir: impl AsRef<Path>,
    include_dirs: impl IntoIterator<Item = impl AsRef<Path>>,
) {
}

#[cfg(feature = "generate-bindings")]
fn generate_bindings(
    allow_dir: impl AsRef<Path>,
    include_dirs: impl IntoIterator<Item = impl AsRef<Path>>,
) {
    let include_dir = allow_dir.as_ref();
    let clang_args = include_dirs
        .into_iter()
        .map(|dir| format!("-I{}", dir.as_ref().to_str().unwrap()))
        .chain([format!("-I{}", include_dir.to_str().unwrap())])
        .collect::<Vec<_>>();
    println!("cargo:rerun-if-changed=wrapper.hpp");
    println!("cargo:rerun-if-changed=src/generated/bindings.rs");
    let mut bind_builder = bindgen::Builder::default()
        .header("wrapper.hpp")
        .allowlist_recursively(true)
        .clang_args(clang_args)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .size_t_is_usize(true)
        .formatter(bindgen::Formatter::Prettyplease)
        .rustified_enum(".*");
    let paths = std::fs::read_dir(include_dir).unwrap();
    for path in paths {
        let path = path.unwrap();
        let file_name = path.file_name().to_str().unwrap().to_string();
        bind_builder =
            bind_builder.allowlist_file(format!(".*{}", file_name.replace(".h", "\\.h")));
    }

    let bindings = bind_builder
        .generate()
        .expect("Unable to generate bindings");
    let generated_file = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("generated")
        .join(env::var("CARGO_CFG_TARGET_OS").unwrap())
        .join(env::var("CARGO_CFG_TARGET_ARCH").unwrap())
        .join("bindings.rs");
    println!("cargo:rerun-if-changed={:?}", generated_file);
    bindings
        .write_to_file(&generated_file)
        .expect("Couldn't write bindings!");
}
