fn main() {
    println!("cargo:rerun-if-changed=assets/inodetree.ico");
    #[cfg(target_os = "windows")]
    embed_windows_resources();
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/inodetree.ico");
    res.compile()
        .expect("failed to compile Windows icon resource");
}
