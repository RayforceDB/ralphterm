#[test]
fn vanity_install_sh_routes_through_cargo_dist_with_cargo_fallback() {
    let body = std::fs::read_to_string("site/install.sh").expect("read site/install.sh");
    assert!(
        body.starts_with("#!/bin/sh\n"),
        "install.sh must start with #!/bin/sh"
    );
    assert!(body.contains("set -eu"), "install.sh must `set -eu`");
    assert!(
        body.contains(
            "github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.sh"
        ),
        "install.sh must reference the cargo-dist installer URL"
    );
    assert!(
        body.contains("cargo install ralphterm"),
        "install.sh must fall back to `cargo install ralphterm` when no prebuilt binary exists for the host platform"
    );
    assert!(
        body.contains("isn't a download for your platform")
            || body.contains("no precompiled binaries"),
        "install.sh must detect the cargo-dist \"no download for your platform\" failure to trigger the fallback"
    );
    assert!(
        body.contains("rustup.rs"),
        "install.sh must point at https://rustup.rs when cargo isn't installed either"
    );
}

#[test]
fn vanity_install_ps1_routes_through_cargo_dist_with_cargo_fallback() {
    let body = std::fs::read_to_string("site/install.ps1").expect("read site/install.ps1");
    assert!(
        body.contains("ErrorActionPreference = 'Stop'"),
        "install.ps1 must set ErrorActionPreference to Stop"
    );
    assert!(
        body.contains(
            "github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.ps1"
        ),
        "install.ps1 must reference the cargo-dist installer URL"
    );
    assert!(
        body.contains("cargo install ralphterm"),
        "install.ps1 must fall back to `cargo install ralphterm` when no prebuilt binary exists for the host platform"
    );
    assert!(
        body.contains("rustup.rs"),
        "install.ps1 must point at https://rustup.rs when cargo isn't installed either"
    );
}

#[test]
fn cargo_toml_has_publish_metadata() {
    let cargo = std::fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
    for field in &[
        "description = ",
        "license = ",
        "repository = ",
        "homepage = ",
        "readme = ",
        "keywords = ",
        "categories = ",
        "authors = ",
        "documentation = ",
    ] {
        assert!(cargo.contains(field), "Cargo.toml must declare {field}");
    }
    assert!(
        cargo.contains("[[bin]]\nname = \"ralphterm\"")
            || cargo.contains("[[bin]]\r\nname = \"ralphterm\""),
        "Cargo.toml must declare the ralphterm binary"
    );
    assert!(
        !cargo.contains("name = \"ralphex\""),
        "ralphex binary was dropped from the package; Cargo.toml must not re-declare it"
    );
}
