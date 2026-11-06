// The setup (and the uninstaller, the same program) writes to Program Files and the machine-wide
// Installed apps list, so Windows has to start it as administrator: that's what this manifest asks for.
// It also gets the GigEyes icon and the app's version in its Properties.
use embed_manifest::{embed_manifest, manifest::ExecutionLevel, new_manifest};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../gigeyes/icons/gigeyes.ico");
    println!("cargo:rerun-if-changed=../gigeyes/src/look.rs");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    embed_manifest(
        new_manifest("GigEyes.Setup")
            .requested_execution_level(ExecutionLevel::RequireAdministrator),
    )
    .expect("embedding the setup's manifest");
    let look = std::fs::read_to_string("../gigeyes/src/look.rs").expect("reading look.rs");
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
        .set_icon("../gigeyes/icons/gigeyes.ico")
        .set("FileDescription", "GigEyes Setup")
        .set("ProductName", "GigEyes")
        .set("FileVersion", version)
        .set("ProductVersion", version)
        .set_version_info(winresource::VersionInfo::FILEVERSION, packed)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed)
        .compile()
        .expect("embedding the icon");
}
