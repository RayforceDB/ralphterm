#[test]
fn vanity_install_sh_redirects_to_cargo_dist_installer() {
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
        "install.sh must redirect to the cargo-dist installer URL"
    );
    assert!(
        body.contains("exec sh -c"),
        "install.sh must exec the downloaded installer"
    );
    assert!(
        body.contains("\"$@\""),
        "install.sh must forward args to the downstream installer"
    );
}

#[test]
fn vanity_install_ps1_redirects_to_cargo_dist_installer() {
    let body = std::fs::read_to_string("site/install.ps1").expect("read site/install.ps1");
    assert!(
        body.contains("ErrorActionPreference = 'Stop'"),
        "install.ps1 must set ErrorActionPreference to Stop"
    );
    assert!(
        body.contains(
            "github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.ps1"
        ),
        "install.ps1 must redirect to the cargo-dist installer URL"
    );
    assert!(
        body.contains("iex (irm $installerUrl)"),
        "install.ps1 must invoke the downloaded installer via iex (irm ...)"
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
        cargo.contains("\"ralphex\""),
        "Cargo.toml keywords must include \"ralphex\""
    );
    assert!(
        cargo.contains("[[bin]]\nname = \"ralphterm\"")
            || cargo.contains("[[bin]]\r\nname = \"ralphterm\""),
        "Cargo.toml must declare the ralphterm binary"
    );
    assert!(
        cargo.contains("[[bin]]\nname = \"ralphex\"")
            || cargo.contains("[[bin]]\r\nname = \"ralphex\""),
        "Cargo.toml must declare the ralphex binary"
    );
}
