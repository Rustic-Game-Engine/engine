fn main() {
    #[cfg(windows)]
    embed_resource::compile("windows.rc", embed_resource::NONE)
        .manifest_required()
        .expect("failed to embed the application icon");
}
