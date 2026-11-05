// The setup (and the uninstaller, the same program) writes to Program Files and the machine-wide
// Installed apps list, so Windows has to start it as administrator: that's what this manifest asks for.
use embed_manifest::{embed_manifest, manifest::ExecutionLevel, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(
            new_manifest("GigEyes.Setup")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("embedding the setup's manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
