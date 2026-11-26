//! The exe's resources on Windows: the icon, the version info and the manifest (Momotus.rc). The version is the
//! app's, from apps/web/package.json, which release-please bumps.

fn main() {
    println!("cargo:rerun-if-changed=Momotus.rc");
    println!("cargo:rerun-if-changed=Momotus.ico");
    println!("cargo:rerun-if-changed=Momotus.manifest");
    println!("cargo:rerun-if-changed=../web/package.json");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let package = std::fs::read_to_string("../web/package.json").expect("apps/web/package.json");
    let version = serde_version(&package);
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "apps/web/package.json version is MAJOR.MINOR.PATCH"
    );
    let text = format!("VERSION_TEXT=\"{version}\"");
    let comma = format!("VERSION_COMMA={},{},{},0", parts[0], parts[1], parts[2]);
    embed_resource::compile("Momotus.rc", [text.as_str(), comma.as_str()])
        .manifest_required()
        .expect("Momotus.rc compiles");
    // The manifest is in the resources: the linker writes none of its own (a second one fails the link).
    println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
}

/// The `"version": "x.y.z"` line of package.json, without a JSON parser at build time.
fn serde_version(package: &str) -> String {
    package
        .lines()
        .find_map(|l| l.trim().strip_prefix("\"version\": \"")?.split('"').next())
        .expect("a version line in apps/web/package.json")
        .to_owned()
}
