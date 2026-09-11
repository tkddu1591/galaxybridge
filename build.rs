fn main() {
    println!("cargo:rerun-if-changed=native/bpf.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/bpf.c")
            .warnings(true)
            .compile("galaxybridge_bpf");
    }
}
