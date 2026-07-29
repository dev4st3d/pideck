fn main() {
    #[cfg(windows)]
    {
        let icon = std::path::Path::new("assets/app.ico");
        let rc = std::path::Path::new("resources/windows/app.rc");
        println!("cargo:rerun-if-changed={}", icon.display());
        println!("cargo:rerun-if-changed={}", rc.display());
        // Resource ID 1 is the HICON GPUI loads for the native title bar.
        embed_resource::compile(rc, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
