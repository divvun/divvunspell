//! Stamps the git revision and build date into the `--version` banner.
//!
//! Both degrade to `unknown` rather than failing the build: the crate has to
//! compile from a source tarball, a vendored copy, or a registry download,
//! none of which carry a `.git`.

use std::process::Command;

fn main() {
    // Only re-run when the checked-out commit actually moves, not on every
    // build. `--git-dir` resolves worktrees and submodules correctly, where a
    // hardcoded `../../.git` would not.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
        // HEAD on a branch is a symref; the branch tip is what changes on
        // commit, so watch the packed and loose ref stores too.
        println!("cargo:rerun-if-changed={git_dir}/refs");
        println!("cargo:rerun-if-changed={git_dir}/packed-refs");
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let rev = match git(&["rev-parse", "--short", "HEAD"]) {
        Some(rev) => {
            // A dirty tree means the binary matches no commit — worth saying
            // out loud, since a stale or hand-patched build is otherwise
            // indistinguishable from the commit it claims to be.
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if dirty { format!("{rev}-dirty") } else { rev }
        }
        None => "unknown".to_string(),
    };
    println!("cargo:rustc-env=DIVVUNSPELL_BUILD_REV={rev}");
    println!("cargo:rustc-env=DIVVUNSPELL_BUILD_DATE={}", build_date());
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// `YYYY-MM-DD` in UTC. Honours `SOURCE_DATE_EPOCH` so reproducible builds
/// stay reproducible; falls back to the wall clock.
fn build_date() -> String {
    let secs = match std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
    {
        Some(s) => s,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    };
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days-since-epoch to a civil (proleptic Gregorian) date — Howard Hinnant's
/// `civil_from_days`. Inlined to keep build-dependencies at zero.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
