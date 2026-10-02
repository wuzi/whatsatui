#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

struct Checkout {
    temp: tempfile::TempDir,
    root: PathBuf,
}

impl Checkout {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout with spaces");
        fs::create_dir_all(root.join(".githooks")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        for file in [".githooks/pre-push", "scripts/check.sh"] {
            fs::copy(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file),
                root.join(file),
            )
            .unwrap();
        }
        fs::write(root.join("tracked"), "original").unwrap();
        let tools = temp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        let cargo = tools.join("cargo");
        // Only the expensive Cargo subprocess is replaced. The hook, shared
        // runner and all Git state checks run for real in an isolated repo.
        fs::write(&cargo, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$CHECK_TEST_LOG\"\nif [ \"${CHECK_TEST_FAIL:-}\" = \"$1\" ]; then exit 17; fi\n").unwrap();
        fs::set_permissions(cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let checkout = Self { temp, root };
        checkout.git(&["init", "-q"]);
        checkout.git(&["config", "user.name", "Hook test"]);
        checkout.git(&["config", "user.email", "hook-test@example.invalid"]);
        checkout.git(&["config", "commit.gpgsign", "false"]);
        checkout.git(&["config", "core.hooksPath", ".githooks"]);
        checkout.git(&["add", "."]);
        checkout.git(&["commit", "-qm", "test fixture"]);
        checkout
    }
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        // Hooks can export repository-specific Git variables; nested test
        // repositories must never address the developer's real checkout.
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ] {
            command.env_remove(name);
        }
        command
    }
    fn git(&self, args: &[&str]) -> String {
        let output = self.command("git").args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn update(&self, revision: &str) -> String {
        format!(
            "refs/heads/test {} refs/heads/test {}\n",
            self.git(&["rev-parse", revision]),
            "0".repeat(40)
        )
    }
    fn run(&self, input: &str, fail: &str) -> Output {
        let log = self.temp.path().join("cargo.log");
        fs::write(&log, "").unwrap();
        let mut child = self
            .command("./.githooks/pre-push")
            .args(["origin", "unused-test-remote"])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.temp.path().join("tools").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("CHECK_TEST_LOG", log)
            .env("CHECK_TEST_FAIL", fail)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.temp.path().join("cargo.log"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn pre_push_stops_on_each_failed_check() {
    let checkout = Checkout::new();
    let update = checkout.update("HEAD");
    let stages = ["fmt", "clippy", "test", "build"];
    for (index, failed) in stages.iter().enumerate() {
        let result = checkout.run(&update, failed);
        assert_eq!(
            result.status.code(),
            Some(17),
            "{failed}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(checkout.calls(), stages[..=index]);
    }
}

#[test]
fn pre_push_checks_matching_branch_and_annotated_tag_once() {
    let checkout = Checkout::new();
    checkout.git(&["-c", "tag.gpgsign=false", "tag", "-a", "demo", "-m", "demo"]);
    let input = checkout.update("HEAD") + &checkout.update("demo");
    let result = checkout.run(&input, "");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(checkout.calls(), ["fmt", "clippy", "test", "build"]);
}

#[test]
fn pre_push_rejects_a_different_revision_or_dirty_checkout() {
    for dirty in ["unstaged", "staged", "untracked", "different-revision"] {
        let checkout = Checkout::new();
        let update = checkout.update("HEAD");
        if dirty == "untracked" {
            fs::write(checkout.root.join("extra"), "change").unwrap();
        } else {
            fs::write(checkout.root.join("tracked"), "change").unwrap();
            if dirty != "unstaged" {
                checkout.git(&["add", "tracked"]);
            }
            if dirty == "different-revision" {
                checkout.git(&["commit", "-qm", "newer checkout"]);
            }
        }
        let result = checkout.run(&update, "");
        assert!(!result.status.success(), "{dirty}");
        assert!(checkout.calls().is_empty(), "{dirty}");
    }
}

#[test]
fn pre_push_skips_deletion_only_and_empty_pushes() {
    let checkout = Checkout::new();
    fs::write(checkout.root.join("tracked"), "local change").unwrap();
    let deletion = format!(
        "(delete) {} refs/heads/test {}\n",
        "0".repeat(40),
        checkout.git(&["rev-parse", "HEAD"])
    );
    for input in ["", deletion.as_str()] {
        assert!(checkout.run(input, "test").status.success());
        assert!(checkout.calls().is_empty());
    }
}
