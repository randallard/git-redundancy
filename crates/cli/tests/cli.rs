//! End-to-end integration tests for the `gr` binary.
//!
//! Each test builds real, **hermetic** git fixtures in a tempdir (isolated HOME +
//! disabled global/system git config + isolated XDG dirs) and runs the actual
//! compiled binary — codifying the status/push scenarios that were exercised by
//! hand: new-branch, dry-run, fast-forward, up-to-date, failover, diverged-skip,
//! dirty-warn, audit log, and the non-zero exit on real failure.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

struct Fixture {
    _tmp: TempDir,
    root: PathBuf,
    home: PathBuf,
    xdg_config: PathBuf,
    xdg_state: PathBuf,
    dev: PathBuf,
    bare: PathBuf,
    workrepo: PathBuf,
}

impl Fixture {
    /// A repo `myrepo` (one commit) under `dev/`, with `data-lan` + `data` both
    /// pointing at a single local bare remote (the interchangeable-paths design).
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        let xdg_config = root.join("xdg");
        let xdg_state = root.join("state");
        let dev = root.join("dev");
        let bare = root.join("home.git");
        let workrepo = dev.join("myrepo");
        for d in [&home, &dev] {
            std::fs::create_dir_all(d).unwrap();
        }

        let fx = Fixture {
            _tmp: tmp,
            root,
            home,
            xdg_config,
            xdg_state,
            dev,
            bare,
            workrepo,
        };

        fx.git(&fx.root, &["init", "--bare", fx.bare.to_str().unwrap()]);
        fx.git(&fx.root, &["init", fx.workrepo.to_str().unwrap()]);
        fx.write("a.txt", "one\ntwo\nthree\n");
        fx.git(&fx.workrepo, &["add", "a.txt"]);
        fx.git(&fx.workrepo, &["commit", "-m", "c1"]);
        fx.git(
            &fx.workrepo,
            &["remote", "add", "data-lan", fx.bare.to_str().unwrap()],
        );
        fx.git(
            &fx.workrepo,
            &["remote", "add", "data", fx.bare.to_str().unwrap()],
        );

        fx.write_config(&format!(
            "roots = [\"{}\"]\n[transport]\norder = [\"data-lan\", \"data\"]\n",
            fx.dev.display()
        ));
        fx
    }

    fn write_config(&self, body: &str) {
        let dir = self.xdg_config.join("git-redundancy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), body).unwrap();
    }

    fn write(&self, rel: &str, contents: &str) {
        std::fs::write(self.workrepo.join(rel), contents).unwrap();
    }

    fn commit_all(&self, msg: &str) {
        self.git(&self.workrepo, &["commit", "-am", msg]);
    }

    fn audit_log(&self) -> PathBuf {
        self.xdg_state.join("git-redundancy").join("audit.log")
    }

    /// Run a hermetic git command; panics with stderr on failure.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = StdCommand::new("git")
            .current_dir(dir)
            .env("HOME", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The `gr` binary, with isolated config/state/home.
    fn gr(&self) -> Command {
        let mut cmd = Command::cargo_bin("gr").unwrap();
        cmd.env("XDG_CONFIG_HOME", &self.xdg_config)
            .env("XDG_STATE_HOME", &self.xdg_state)
            .env("HOME", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }
}

#[test]
fn empty_config_reports_nothing_to_do() {
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("gr")
        .unwrap()
        .env("XDG_CONFIG_HOME", tmp.path().join("xdg"))
        .env("HOME", tmp.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("No repos configured"));
}

#[test]
fn status_shows_new_before_push() {
    let fx = Fixture::new();
    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo").and(predicate::str::contains("new")));
}

#[test]
fn status_offline_shows_lifecycle_column_unknown() {
    let fx = Fixture::new(); // no [server] → home side unknown
    fx.gr()
        .args(["status", "--offline"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Life")
                .and(predicate::str::contains("?"))
                .and(predicate::str::contains("myrepo")),
        );
}

#[test]
fn status_flags_other_branches_needing_attention() {
    let fx = Fixture::new();
    // current branch is main; a second, un-backed-up branch should raise +1⚠.
    fx.git(&fx.workrepo, &["branch", "feature"]);
    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("+1⚠"));
}

#[test]
fn status_repo_detail_shows_sync_action_column() {
    let fx = Fixture::new();
    // `gr status <repo>` resolves by directory name and previews sync actions.
    fx.gr()
        .args(["status", "myrepo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sync").and(predicate::str::contains("push")));
}

#[test]
fn homes_is_an_alias_for_the_status_fleet_view() {
    // `homes` retired into a thin alias for `status` (lifecycle is a column now).
    let fx = Fixture::new();
    fx.gr()
        .arg("homes")
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo").and(predicate::str::contains("Life")));
}

#[test]
fn status_json_emits_structured_output() {
    let fx = Fixture::new();
    // --json replaces the table with parseable JSON (no box-drawing characters).
    fx.gr()
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"repo\": \"myrepo\"")
                .and(predicate::str::contains("\"branches\""))
                .and(predicate::str::contains("\"lifecycle\""))
                .and(predicate::str::contains('╭').not()),
        );
}

#[test]
fn create_without_server_config_fails_with_guidance() {
    let fx = Fixture::new(); // default config has no [server]
    fx.gr()
        .current_dir(&fx.workrepo)
        .arg("create")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no [server] configured"));
}

#[test]
fn clone_target_outside_roots_is_refused_with_guidance() {
    let fx = Fixture::new();
    fx.write_config(&format!(
        "roots = [\"{}\"]\n[server]\nroot = \"/data/git\"\naliases = [\"tenx-lan\"]\n",
        fx.dev.display()
    ));
    // A target outside every configured root: refused before any network, exit 0,
    // with the roots listed (the user's move).
    fx.gr()
        .args(["clone", "somerepo", "/tmp/definitely-not-a-root/x"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("not inside a configured root")
                .and(predicate::str::contains("your move")),
        );
}

#[test]
fn sync_with_nonmatching_only_filter_matches_nothing() {
    let fx = Fixture::new();
    fx.gr()
        .args(["sync", "no-such-repo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No repos match"));
}

#[test]
fn sync_pushes_committed_work() {
    let fx = Fixture::new();
    // The fixture's one commit was never pushed → sync pushes it (new branch).
    fx.gr()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("pushed"));
}

#[test]
fn sync_dry_run_pushes_nothing() {
    let fx = Fixture::new();
    fx.gr()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would push"));
}

#[test]
fn sync_fast_forwards_when_home_is_ahead_and_tree_clean() {
    let fx = Fixture::new();
    // Advance the home past the work repo, then move the work repo back so it is
    // strictly behind: sync must fast-forward it.
    fx.git(&fx.workrepo, &["push", "data-lan", "main"]);
    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.commit_all("c2");
    fx.git(&fx.workrepo, &["push", "data-lan", "main"]); // home @ c2
    fx.git(&fx.workrepo, &["reset", "--hard", "HEAD~1"]); // work repo @ c1, clean
    fx.gr()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("fast-forwarded"));
}

#[test]
fn sync_blocks_fast_forward_when_tree_is_dirty() {
    let fx = Fixture::new();
    fx.git(&fx.workrepo, &["push", "data-lan", "main"]);
    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.commit_all("c2");
    fx.git(&fx.workrepo, &["push", "data-lan", "main"]); // home @ c2
    fx.git(&fx.workrepo, &["reset", "--hard", "HEAD~1"]); // behind by 1
    fx.write("a.txt", "dirty edit\n"); // uncommitted change to a tracked file
    fx.gr()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("tree dirty"));
}

#[test]
fn dry_run_changes_nothing_and_is_not_audited() {
    let fx = Fixture::new();
    fx.gr()
        .args(["push", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would push (new)"));
    // No remote update...
    let refs = fx.git(&fx.workrepo, &["for-each-ref", "refs/remotes"]);
    assert!(
        refs.trim().is_empty(),
        "dry-run must not create tracking refs"
    );
    // ...and no audit record.
    assert!(
        !fx.audit_log().exists(),
        "dry-run must not write the audit log"
    );
}

#[test]
fn push_new_then_uptodate_with_failover_and_audit() {
    let fx = Fixture::new();

    // First push creates the branch via the first reachable remote (data-lan).
    fx.gr()
        .arg("push")
        .assert()
        .success()
        .stdout(predicate::str::contains("pushed (new)"));

    // Failover pushed once: data-lan is ok, data is still new.
    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").and(predicate::str::contains("new")));

    // Nothing to do now.
    fx.gr()
        .arg("push")
        .assert()
        .success()
        .stdout(predicate::str::contains("up-to-date"));

    // Audit log captured the real push.
    let log = std::fs::read_to_string(fx.audit_log()).unwrap();
    assert!(log.contains("result=pushed"), "audit log: {log}");
    assert!(log.contains("remote=data-lan"));
}

#[test]
fn push_fast_forwards_new_commit() {
    let fx = Fixture::new();
    fx.gr().arg("push").assert().success();

    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.commit_all("c2");

    fx.gr()
        .arg("push")
        .assert()
        .success()
        .stdout(predicate::str::contains("pushed (↑1)"));
}

#[test]
fn push_skips_diverged_conflict_without_failing() {
    let fx = Fixture::new();
    fx.gr().arg("push").assert().success();

    // A second clone advances the remote with a conflicting edit.
    let clone2 = fx.root.join("clone2");
    fx.git(
        &fx.root,
        &["clone", fx.bare.to_str().unwrap(), clone2.to_str().unwrap()],
    );
    std::fs::write(clone2.join("a.txt"), "one\ntwo\nCLONE2\n").unwrap();
    fx.git(&clone2, &["commit", "-am", "c_clone"]);
    fx.git(&clone2, &["push", "origin", "main"]);

    // Local diverges with an overlapping edit; fetch so the tracking ref shows it.
    fx.git(&fx.workrepo, &["fetch", "data-lan"]);
    fx.write("a.txt", "one\ntwo\nWORK\n");
    fx.commit_all("c_work");

    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("CONFLICT"));

    // Skipping a diverged branch is success, not failure — and never forced.
    fx.gr()
        .arg("push")
        .assert()
        .success()
        .stdout(predicate::str::contains("SKIPPED").and(predicate::str::contains("never forced")));
}

#[test]
fn dirty_tree_is_warned_and_not_pushed() {
    let fx = Fixture::new();
    fx.gr().arg("push").assert().success();

    // Uncommitted edit + an untracked file.
    fx.write("a.txt", "one\ntwo\nthree\nlocal-edit\n");
    fx.write("scratch.txt", "junk");

    fx.gr().arg("push").assert().success().stdout(
        predicate::str::contains("up-to-date").and(predicate::str::contains("NOT backed up")),
    );
}

#[test]
fn push_failure_exits_nonzero() {
    let fx = Fixture::new();
    // Point both remotes at a path that doesn't exist.
    let nope = fx.root.join("nope.git");
    fx.git(
        &fx.workrepo,
        &["remote", "set-url", "data-lan", nope.to_str().unwrap()],
    );
    fx.git(
        &fx.workrepo,
        &["remote", "set-url", "data", nope.to_str().unwrap()],
    );

    fx.gr()
        .arg("push")
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("FAILED"));
}
