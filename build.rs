fn main() {
    println!("cargo:rerun-if-changed=native/bpf.c");
    println!("cargo:rerun-if-changed=native/sandbox.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/bpf.c")
            .file("native/sandbox.c")
            .warnings(true)
            .compile("galaxybridge_bpf");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }
}
