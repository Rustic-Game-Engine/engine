fn main() {
    println!("cargo:rerun-if-changed=src/egl-compat.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        cc::Build::new()
            .file("src/egl-compat.c")
            .compile("rustic_egl_compat");
    }
}
