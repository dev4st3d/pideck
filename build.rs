fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/app.ico");
        println!("cargo:rerun-if-changed=resources/windows/app.rc");
        embed_resource::compile("resources/windows/app.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embed Windows application icon");
    }
}
