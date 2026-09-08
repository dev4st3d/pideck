use std::{env, fs, path::PathBuf};

const FONTS: [&str; 4] = [
    "DMSans-Variable.ttf", "InstrumentSerif-Regular.ttf",
    "IBMPlexMono-Regular.ttf", "IBMPlexMono-Medium.ttf",
];

fn prepare_fonts() {
    println!("cargo:rerun-if-env-changed=PIDECK_DESIGN_DIR");
    let source = env::var_os("PIDECK_DESIGN_DIR")
        .map(PathBuf::from).map(|path| path.join("fonts"))
        .unwrap_or_else(|| PathBuf::from("assets/fonts"));
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"))
        .join("fonts");
    fs::create_dir_all(&destination).expect("create embedded font directory");
    for name in FONTS {
        let path = source.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        fs::copy(&path, destination.join(name)).unwrap_or_else(|error| {
            panic!("Missing design font {}: {error}. Run: node scripts/prepare-fonts.mjs --zip /path/to/pideck-design.zip", path.display())
        });
    }
}

fn main() {
    prepare_fonts();
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
