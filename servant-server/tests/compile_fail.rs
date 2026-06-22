#[test]
fn public_api_misuse_fails_to_compile() {
    if rustc_minor_version().is_some_and(|minor| minor <= 83) {
        portable_compile_fail_checks();
        return;
    }

    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}

fn rustc_minor_version() -> Option<u32> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    let mut parts = version.split_whitespace().nth(1)?.split('.');
    let _major = parts.next()?;
    parts.next()?.parse().ok()
}

fn portable_compile_fail_checks() {
    let cases = [
        CompileFailCase {
            name: "auth_handler_user_mismatch",
            expected: &["error[E0631]", "AuthenticatedUser", "String"],
        },
        CompileFailCase {
            name: "generated_client_auth_protect_unsupported",
            expected: &["error[E0277]", "AuthProtect<User", "HasClient"],
        },
        CompileFailCase {
            name: "generated_client_remote_host_unsupported",
            expected: &["error[E0277]", "RemoteHost<", "HasClient"],
        },
        CompileFailCase {
            name: "generated_client_vault_unsupported",
            expected: &["error[E0277]", "Vault<", "HasClient"],
        },
        CompileFailCase {
            name: "generated_client_with_resource_unsupported",
            expected: &["error[E0277]", "WithResource<u32", "HasClient"],
        },
        CompileFailCase {
            name: "unsupported_content_type",
            expected: &["NotAMime: MediaType", "String: MimeRender<NotAMime>"],
        },
        CompileFailCase {
            name: "wrong_client_arguments",
            expected: &["(): RunClient", "expected `u64`, found `String`"],
        },
        CompileFailCase {
            name: "wrong_handler_shape",
            expected: &["error[E0593]", "closure is expected to take 1 argument"],
        },
    ];

    for case in cases {
        case.assert_compile_fails();
    }
}

struct CompileFailCase {
    name: &'static str,
    expected: &'static [&'static str],
}

impl CompileFailCase {
    fn assert_compile_fails(&self) {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest_dir
            .parent()
            .expect("servant-server should live under the workspace root");
        let target_dir = workspace
            .join("target")
            .join("tests")
            .join("portable-compile-fail")
            .join(self.name);
        std::fs::create_dir_all(&target_dir).expect("create portable compile-fail target dir");
        let manifest_path = target_dir.join("Cargo.toml");
        let source_path = manifest_dir
            .join("tests")
            .join("ui")
            .join(format!("{}.rs", self.name));
        let manifest = format!(
            r#"[package]
name = "servant-server-portable-compile-fail-{name}"
version = "0.0.0"
edition = "2021"

[workspace]

[dependencies]
servant = {{ path = "{servant}" }}
servant-client = {{ path = "{servant_client}" }}
servant-server = {{ path = "{servant_server}" }}

[[bin]]
name = "{name}"
path = "{source}"
"#,
            name = self.name.replace('_', "-"),
            servant = workspace.join("servant").display(),
            servant_client = workspace.join("servant-client").display(),
            servant_server = manifest_dir.display(),
            source = source_path.display(),
        );
        std::fs::write(&manifest_path, manifest).expect("write portable compile-fail manifest");

        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = std::process::Command::new(cargo)
            .arg("check")
            .arg("--manifest-path")
            .arg(&manifest_path)
            .arg("--bin")
            .arg(self.name.replace('_', "-"))
            .env("CARGO_TARGET_DIR", target_dir.join("target"))
            .output()
            .expect("run portable compile-fail cargo check");

        assert!(
            !output.status.success(),
            "{} unexpectedly compiled successfully",
            self.name
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for expected in self.expected {
            assert!(
                stderr.contains(expected),
                "{} stderr did not contain {:?}\n{}",
                self.name,
                expected,
                stderr
            );
        }
    }
}
