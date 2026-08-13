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
    bin: PathBuf,
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
        let bin = root.join("bin");
        for d in [&home, &dev, &bin] {
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
            bin,
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
        // `bin` is prepended to PATH but starts empty, so behaviour is unchanged
        // unless a test calls `install_fake_ssh` (ADR-0015 / ADR-0012 coverage).
        let path = match std::env::var_os("PATH") {
            Some(p) => format!("{}:{}", self.bin.display(), p.to_string_lossy()),
            None => self.bin.display().to_string(),
        };
        cmd.env("XDG_CONFIG_HOME", &self.xdg_config)
            .env("XDG_STATE_HOME", &self.xdg_state)
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        cmd
    }

    /// Install a stub `ssh` earlier on PATH than the real one, so the "home
    /// server" becomes an ordinary local directory of `*.git` dirs.
    ///
    /// `gr` reaches a server only via `ssh <alias> "ls -d <root>/*.git ..."`
    /// (io::inventory::list_homes). The stub drops the `-o` option pairs, takes
    /// the next argument as the alias, and runs the remaining command **locally**
    /// — so a listing of a real temp dir is a faithful stand-in for the real
    /// thing, with no network and no second machine.
    ///
    /// Aliases named in `GR_FAKE_SSH_UNREACHABLE` (colon-separated) fail like an
    /// unreachable host instead, which is how the `?` states are exercised.
    fn install_fake_ssh(&self) {
        let script = "#!/bin/sh\n\
             while [ \"$1\" = \"-o\" ]; do shift 2; done\n\
             alias=\"$1\"; shift\n\
             case \":$GR_FAKE_SSH_UNREACHABLE:\" in\n\
             \x20 *\":$alias:\"*) echo \"ssh: connect to host $alias: Connection refused\" >&2; exit 255 ;;\n\
             esac\n\
             exec /bin/sh -c \"$*\"\n";
        let p = self.bin.join("ssh");
        std::fs::write(&p, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Create a server-side "home" (`<root>/<name>.git`) for the fake server.
    fn make_home(&self, root: &Path, name: &str) {
        std::fs::create_dir_all(root.join(format!("{name}.git"))).unwrap();
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

// ---------------------------------------------------------------------------
// ADR-0015 — the `[backup]` server and the `Bkp` presence column.
//
// These were the ADR's `Verified-by: none` until 2026-08-13: the column rested
// entirely on live use against the real tenx primary+backup pair. The stub `ssh`
// (see `install_fake_ssh`) makes both servers ordinary temp directories, so all
// four documented states are now covered hermetically.
// ---------------------------------------------------------------------------

/// Config wiring `[server]` + optionally `[backup]` at fake-ssh-backed roots.
fn backup_config(fx: &Fixture, primary: &Path, backup: Option<&Path>) -> String {
    let mut s = format!(
        "roots = [\"{}\"]\n[transport]\norder = [\"data-lan\", \"data\"]\n\
         [server]\nroot = \"{}\"\naliases = [\"fake-primary\"]\n",
        fx.dev.display(),
        primary.display(),
    );
    if let Some(b) = backup {
        s.push_str(&format!(
            "[backup]\nroot = \"{}\"\naliases = [\"fake-backup\"]\n",
            b.display()
        ));
    }
    s
}

// Assertions go through `--json`, not the rendered table: the table's untracked
// column is *also* headed `?`, so matching `?` in the table cannot distinguish an
// unreachable backup from an untracked-file count. The JSON `backup` field is the
// exact contract ADR-0015 specifies.

#[test]
fn status_backup_field_is_ok_when_the_home_exists_on_the_backup() {
    let fx = Fixture::new();
    fx.install_fake_ssh();
    let (primary, backup) = (fx.root.join("srv"), fx.root.join("bkp"));
    // The fixture's remotes point at `<root>/home.git`, so the home name is `home`.
    fx.make_home(&primary, "home");
    fx.make_home(&backup, "home");
    fx.write_config(&backup_config(&fx, &primary, Some(&backup)));

    fx.gr()
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backup\": \"ok\""));
}

#[test]
fn status_backup_field_is_miss_when_the_home_is_absent_from_the_backup() {
    let fx = Fixture::new();
    fx.install_fake_ssh();
    let (primary, backup) = (fx.root.join("srv"), fx.root.join("bkp"));
    fx.make_home(&primary, "home");
    // Backup exists and is reachable, but this repo was never mirrored to it —
    // the exact redundancy gap ADR-0015 exists to surface. The backup must be
    // non-empty, or "reachable but missing" would be indistinguishable from
    // "listing failed".
    fx.make_home(&backup, "some-other-repo");
    fx.write_config(&backup_config(&fx, &primary, Some(&backup)));

    fx.gr()
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backup\": \"miss\""));
}

#[test]
fn status_backup_field_is_unknown_when_the_backup_is_unreachable() {
    let fx = Fixture::new();
    fx.install_fake_ssh();
    let (primary, backup) = (fx.root.join("srv"), fx.root.join("bkp"));
    fx.make_home(&primary, "home");
    // The home IS present on the backup: if unreachability were mishandled this
    // would read `ok`, so the test distinguishes `?` from a stale success.
    fx.make_home(&backup, "home");
    fx.write_config(&backup_config(&fx, &primary, Some(&backup)));

    fx.gr()
        .env("GR_FAKE_SSH_UNREACHABLE", "fake-backup")
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("\"backup\": \"?\"")
                .and(predicate::str::contains("\"backup\": \"ok\"").not()),
        );
}

#[test]
fn status_omits_the_backup_column_entirely_when_no_backup_is_configured() {
    let fx = Fixture::new();
    fx.install_fake_ssh();
    let primary = fx.root.join("srv");
    fx.make_home(&primary, "home");
    fx.write_config(&backup_config(&fx, &primary, None));

    // No `[backup]` → no JSON field at all, and no `Bkp` column in the table.
    fx.gr()
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backup\"").not());

    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Bkp").not());
}

#[test]
fn status_dirty_only_hides_clean_repos_and_shows_dirty_ones() {
    // ADR-0006 `--dirty-only`. The fixture repo starts clean, so it must be
    // absent; dirtying the tree must bring it back.
    let fx = Fixture::new();
    fx.gr()
        .args(["status", "--offline", "--dirty-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo").not());

    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.gr()
        .args(["status", "--offline", "--dirty-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo"));
}

#[test]
fn status_dirty_only_counts_untracked_files_as_dirty() {
    // Untracked-only is still "not fully backed up" — it must not read as clean.
    let fx = Fixture::new();
    fx.write("brand-new.txt", "never added\n");
    fx.gr()
        .args(["status", "--offline", "--dirty-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo"));
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
fn onboard_without_server_config_fails_with_guidance() {
    let fx = Fixture::new(); // default config has no [server]
    fx.gr()
        .arg("onboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no [server] configured"));
}

#[test]
fn onboard_with_unreachable_server_fails_loudly() {
    let fx = Fixture::new();
    // [server] is configured but its alias can't be reached: onboarding provisions
    // on the server, so it refuses rather than half-acting (ADR-0012 §5).
    fx.write_config(&format!(
        "roots = [\"{}\"]\n[server]\nroot = \"/data/git\"\naliases = [\"gr-no-such-host.invalid\"]\n",
        fx.dev.display(),
    ));
    fx.gr()
        .arg("onboard")
        .assert()
        .failure()
        .stderr(predicate::str::contains("home server unreachable"));
}

#[test]
fn ignored_repo_shows_ignored_lifecycle_in_status() {
    let fx = Fixture::new();
    // Offline status keeps it hermetic; the ignore list still applies and the
    // repo stays visible as `ignored` rather than being hidden (ADR-0017).
    fx.write_config(&format!(
        "roots = [\"{}\"]\nignore = [\"myrepo\"]\n[transport]\norder = [\"data-lan\", \"data\"]\n",
        fx.dev.display(),
    ));
    fx.gr()
        .args(["status", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myrepo").and(predicate::str::contains("ignored")));
}

#[test]
fn repoint_without_server_config_fails_with_guidance() {
    let fx = Fixture::new(); // default config has no [server]
    fx.gr()
        .args(["repoint", "myrepo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no [server] configured"));
}

#[test]
fn repoint_without_backup_is_refused() {
    let fx = Fixture::new();
    // [server] but no [backup]: repoint re-roles the backup home, so it refuses
    // before touching the network (ADR-0018).
    fx.write_config(&format!(
        "roots = [\"{}\"]\n[server]\nroot = \"/data/git\"\naliases = [\"none-such\"]\n",
        fx.dev.display(),
    ));
    fx.gr()
        .args(["repoint", "myrepo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("needs a [backup]"));
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

    // Failover pushed once via data-lan. status now fetches before classifying
    // (ADR-0019), so both transports to the *same* home read `ok` — the old stale
    // `new` on the unfetched `data` column is gone. The audit log (below) is what
    // records which transport actually did the push.
    fx.gr()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").and(predicate::str::contains("new").not()));

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

/// Regression for ADR-0019. After a *manual* `git remote set-url` repoints a
/// remote at a home that is **behind** the working copy, the stale local
/// tracking ref (still holding the old home's value, which equals the working
/// copy) must not make `gr` report a false `up-to-date`/`ok` and skip the push.
/// This is the exact field failure: a backup silently not taken.
#[test]
fn repoint_to_behind_home_still_pushes() {
    let fx = Fixture::new();

    // c1 lands on the original home; the data-lan tracking ref now equals c1.
    fx.gr().arg("push").assert().success();

    // A second home, seeded with c1 and then left behind.
    let new_home = fx.root.join("new-home.git");
    fx.git(&fx.root, &["init", "--bare", new_home.to_str().unwrap()]);
    fx.git(&fx.workrepo, &["push", new_home.to_str().unwrap(), "main"]);

    // Working copy advances to c2 and pushes to the *original* home, so the
    // data-lan tracking ref now equals the working copy (c2).
    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.commit_all("c2");
    fx.gr().arg("push").assert().success();

    // Manual repoint to the behind home — deliberately *no* fetch (the foot-gun).
    // The tracking ref is still c2, equal to the working copy.
    fx.git(
        &fx.workrepo,
        &["remote", "set-url", "data-lan", new_home.to_str().unwrap()],
    );
    fx.git(
        &fx.workrepo,
        &["remote", "set-url", "data", new_home.to_str().unwrap()],
    );

    // `--offline` trusts the stale ref → the false `ok` that masked the bug.
    fx.gr()
        .args(["status", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));

    // The fix: `push` fetches before classifying, sees the home is behind, and
    // actually pushes c2 — not a false `up-to-date`.
    fx.gr()
        .arg("push")
        .assert()
        .success()
        .stdout(predicate::str::contains("pushed (↑1)"));

    // The behind home really did receive c2.
    let new_head = fx.git(
        &fx.root,
        &["--git-dir", new_home.to_str().unwrap(), "rev-parse", "main"],
    );
    let work_head = fx.git(&fx.workrepo, &["rev-parse", "main"]);
    assert_eq!(new_head.trim(), work_head.trim());
}

// ==================== bare `gr`: dirty-repo review (ADR-0022) ====================
//
// The default (no-subcommand) invocation prints the status table, then — only
// when a repo is dirty — offers to cycle through and stage/commit. `git commit`
// needs an identity, which the fixture's global config deliberately blocks, so
// these tests supply one via env on the specific `gr()` invocation that commits.

#[test]
fn default_command_has_no_review_prompt_when_clean() {
    let fx = Fixture::new();
    fx.gr()
        .assert()
        .success()
        .stdout(predicate::str::contains("uncommitted work").not());
}

#[test]
fn default_command_offers_review_and_quits_on_empty_input() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nedited\n");
    fx.write("scratch.txt", "junk");

    fx.gr()
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 repo(s) have uncommitted work"))
        .stdout(predicate::str::contains("stage & review"));

    // Quitting must leave the tree exactly as it was.
    let status = fx.git(&fx.workrepo, &["status", "--porcelain"]);
    assert!(status.contains("a.txt"));
    assert!(status.contains("scratch.txt"));
}

#[test]
fn default_command_stages_and_commits_tracked_and_untracked_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nedited\n");
    fx.write("scratch.txt", "junk");

    fx.gr()
        .env("EDITOR", "true")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .write_stdin("s\ny\ny\nstage everything\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("committed myrepo main"));

    assert!(fx.git(&fx.workrepo, &["status", "--porcelain"]).is_empty());
    let log = fx.git(&fx.workrepo, &["log", "--oneline"]);
    assert!(log.contains("stage everything"));
}

#[test]
fn default_command_empty_commit_message_leaves_files_staged() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nedited\n");

    fx.gr()
        .env("EDITOR", "true")
        .write_stdin("s\ny\n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("left staged — not committed"));

    let status = fx.git(&fx.workrepo, &["status", "--porcelain"]);
    assert!(
        status.starts_with("M "),
        "expected a.txt staged: {status:?}"
    );
    // Exactly the fixture's one commit — nothing new landed.
    let log = fx.git(&fx.workrepo, &["log", "--oneline"]);
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn default_command_reports_conflict_without_staging_it() {
    let fx = Fixture::new();
    // Diverge two branches on the same file, then merge to leave a real conflict.
    fx.git(&fx.workrepo, &["checkout", "-b", "other"]);
    fx.write("a.txt", "one\ntwo\nother-branch\n");
    fx.commit_all("other edit");
    fx.git(&fx.workrepo, &["checkout", "main"]);
    fx.write("a.txt", "one\ntwo\nmain-branch\n");
    fx.commit_all("main edit");
    // Conflict expected — ignore the non-zero exit from `git merge`. Needs an
    // identity even though it never commits: git checks upfront, before it
    // knows the merge will conflict rather than fast-forward/auto-commit.
    let _ = std::process::Command::new("git")
        .current_dir(&fx.workrepo)
        .env("HOME", &fx.home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
        .args(["merge", "other"])
        .output();
    let before = fx.git(&fx.workrepo, &["status", "--porcelain"]);
    assert!(
        before.starts_with("UU"),
        "expected a merge conflict: {before:?}"
    );

    fx.gr()
        .write_stdin("s\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "merge conflict — resolve manually",
        ));

    // Never staged/touched — still the same unmerged entry.
    let after = fx.git(&fx.workrepo, &["status", "--porcelain"]);
    assert_eq!(before, after);
}

#[test]
fn default_command_walks_two_dirty_repos_in_order() {
    let fx = Fixture::new();
    let repo2 = fx.dev.join("zzrepo");
    fx.git(&fx.root, &["init", repo2.to_str().unwrap()]);
    std::fs::write(repo2.join("b.txt"), "hello\n").unwrap();
    fx.git(&repo2, &["add", "b.txt"]);
    fx.git(&repo2, &["commit", "-m", "c1"]);
    std::fs::write(repo2.join("b.txt"), "hello\nworld\n").unwrap();

    fx.write("a.txt", "one\ntwo\nedited\n");

    fx.gr()
        .env("EDITOR", "true")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        // top-level "s", then myrepo's a.txt (y + msg), then zzrepo's b.txt (y + msg)
        .write_stdin("s\ny\nmyrepo commit\ny\nzzrepo commit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("2 repo(s) have uncommitted work"))
        .stdout(predicate::str::contains("2 repo(s) reviewed"));

    assert!(fx.git(&fx.workrepo, &["status", "--porcelain"]).is_empty());
    assert!(fx.git(&repo2, &["status", "--porcelain"]).is_empty());
    assert!(fx
        .git(&repo2, &["log", "--oneline"])
        .contains("zzrepo commit"));
}
