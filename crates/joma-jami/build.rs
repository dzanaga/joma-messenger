fn main() {
    println!("cargo:rerun-if-changed=shim/jami_shim.cpp");
    // jami.pc lists private requirements Arch does not ship .pc files for, so the paths
    // are given directly.
    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .file("shim/jami_shim.cpp")
        .include("/usr/include/jami")
        .warnings(false)
        .compile("joma_jami_shim");
    println!("cargo:rustc-link-lib=dylib=jami");
    println!("cargo:rustc-link-lib=dylib=jsoncpp");
    println!("cargo:rustc-link-lib=dylib=avutil");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
