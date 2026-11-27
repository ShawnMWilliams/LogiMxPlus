//! Embed a version resource on Windows so Task Manager shows "LogiMxPlus".

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set("FileDescription", "LogiMxPlus")
        .set("ProductName", "LogiMxPlus")
        .set("InternalName", "LogiMxPlus")
        .set("OriginalFilename", "LogiMxPlus.exe");
    if let Err(e) = res.compile() {
        println!("cargo:warning=no Windows version resource: {e}");
    }
}
