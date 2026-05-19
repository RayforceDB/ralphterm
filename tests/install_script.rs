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

#[test]
fn homebrew_formula_installs_release_artifacts() {
    let formula = std::fs::read_to_string("Formula/ralphterm.rb").expect("read Homebrew formula");

    for expected in [
        "class Ralphterm < Formula",
        "version \"0.4.16\"",
        "ralphterm-aarch64-apple-darwin.tar.xz",
        "ralphterm-x86_64-apple-darwin.tar.xz",
        "ralphterm-x86_64-unknown-linux-gnu.tar.xz",
        "bin.install Dir[\"*/ralphterm\"].first => \"ralphterm\"",
        "ralphterm --version",
    ] {
        assert!(
            formula.contains(expected),
            "Homebrew formula should contain {expected}"
        );
    }

    for expected_sha in [
        "5ac97c4c9b7df3a363c45f8d3c7a5fbdce55a17f521aa1a6525dbf6d45f2a5b2",
        "bf14233d96f6e90844b0ebbf36fe120649d711e59e1493cda27c1ef12e9518e7",
        "f3aa791e67d1f7d71d0881770278c58b47a0884371a8e1a68d32c00d3809dad9",
    ] {
        assert!(
            formula.contains(expected_sha),
            "Homebrew formula should pin release checksum {expected_sha}"
        );
    }
}
