//! Windows: embeds the icon and version details in pdfr.exe (Explorer,
//! the taskbar, "Open with"). Only for MSVC builds (CI and Windows PCs),
//! which have the resource compiler; other builds skip it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon/pdfr.ico");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/pdfr.ico")
            .set("ProductName", "pdfr")
            .set("FileDescription", "pdfr: fill in PDF forms")
            .set("LegalCopyright", "Copyright (c) 2026 Rasmus Kjellberg");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
