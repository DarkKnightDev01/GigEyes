// The GigEyes icon for Explorer, the taskbar, shortcuts and Installed apps, and the version shown in
// the file's Properties (taken from look::VERSION, since Cargo's version can't hold four numbers).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/gigeyes.ico");
    println!("cargo:rerun-if-changed=src/look.rs");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    let look = std::fs::read_to_string("src/look.rs").expect("reading look.rs");
    let version = look
        .lines()
        .find_map(|l| {
            l.strip_prefix("pub const VERSION: &str = \"")?
                .strip_suffix("\";")
        })
        .expect("look.rs has VERSION");
    // "0.5.0.1" -> 0x0000_0005_0000_0001: four 16-bit numbers.
    let packed = version
        .split('.')
        .chain(std::iter::repeat("0"))
        .take(4)
        .fold(0u64, |n, part| {
            (n << 16) | part.parse::<u64>().expect("numeric version")
        });
    winresource::WindowsResource::new()
        .set_icon("icons/gigeyes.ico")
        .set("FileDescription", "GigEyes")
        .set("ProductName", "GigEyes")
        .set("FileVersion", version)
        .set("ProductVersion", version)
        .set_version_info(winresource::VersionInfo::FILEVERSION, packed)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed)
        .compile()
        .expect("embedding the icon");
}
