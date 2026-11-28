use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitFileEntry {
    pub path: String,
    pub index_status: char,
    pub worktree_status: char,
}

impl GitFileEntry {
    pub fn is_staged(&self) -> bool {
        self.index_status != ' ' && self.index_status != '?'
    }

    pub fn is_unstaged(&self) -> bool {
        self.worktree_status != ' ' || self.index_status == '?'
    }

    pub fn status_badge(&self, staged: bool) -> char {
        if staged {
            self.index_status
        } else if self.index_status == '?' {
            '?'
        } else {
            self.worktree_status
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub is_bare: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub branch: String,
    pub is_repository: bool,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFileEntry>,
    pub commits: Vec<GitCommitEntry>,
    pub branches: Vec<String>,
    pub worktrees: Vec<WorktreeEntry>,
}

impl GitSnapshot {
    pub fn parse_worktrees(output: &str) -> Vec<WorktreeEntry> {
        let mut list = Vec::new();
        let mut cur_path: Option<String> = None;
        let mut cur_head = String::new();
        let mut cur_branch = String::new();
        let mut cur_bare = false;

        for line in output.lines() {
            if line.trim().is_empty() {
                if let Some(p) = cur_path.take() {
                    list.push(WorktreeEntry {
                        path: p,
                        head: std::mem::take(&mut cur_head),
                        branch: std::mem::take(&mut cur_branch),
                        is_bare: cur_bare,
                    });
                    cur_bare = false;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("worktree ") {
                if let Some(p) = cur_path.take() {
                    list.push(WorktreeEntry {
                        path: p,
                        head: std::mem::take(&mut cur_head),
                        branch: std::mem::take(&mut cur_branch),
                        is_bare: cur_bare,
                    });
                    cur_bare = false;
                }
                cur_path = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("HEAD ") {
                cur_head = rest.trim().chars().take(7).collect();
            } else if let Some(rest) = line.strip_prefix("branch ") {
                let b = rest
                    .trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(rest.trim());
                cur_branch = b.to_string();
            } else if line.trim() == "bare" {
                cur_bare = true;
            }
        }
        if let Some(p) = cur_path.take() {
            list.push(WorktreeEntry {
                path: p,
                head: cur_head,
                branch: cur_branch,
                is_bare: cur_bare,
            });
        }
        list
    }
}

pub fn run_git_in(cwd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Switch without forcing or discarding local changes. Git also resolves unique remote branches.
pub fn switch_branch_in(cwd: &str, branch: &str) -> Result<String, String> {
    let local = run_git_in(
        cwd,
        &["show-ref", "--verify", &format!("refs/heads/{branch}")],
    )
    .is_some();
    if !local {
        let remote = branch.strip_prefix("remotes/").unwrap_or(branch);
        if run_git_in(
            cwd,
            &["show-ref", "--verify", &format!("refs/remotes/{remote}")],
        )
        .is_some()
        {
            return git_command_in(cwd, &["switch", "--track", "--", remote]);
        }
    }
    git_command_in(cwd, &["switch", "--", branch])
}

pub fn git_command_in(cwd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[derive(Clone, Debug)]
pub struct GitDiffPreview {
    pub title: String,
    pub text: String,
}

const MAX_DIFF_BYTES: usize = 256 * 1024;
const MAX_DIFF_LINES: usize = 2000;

fn bounded_diff(text: &str) -> String {
    let mut end = text.len().min(MAX_DIFF_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some((offset, _)) = text[..end].match_indices('\n').nth(MAX_DIFF_LINES - 1) {
        end = offset;
    }
    if end < text.len() {
        format!(
            "{}\n\n[Diff truncated: display limited to 256 KiB / 2000 lines]",
            &text[..end]
        )
    } else if text.is_empty() {
        "No diff to display.".into()
    } else {
        text.to_string()
    }
}

pub fn diff_file_in(
    cwd: &str,
    entry: &GitFileEntry,
    staged: bool,
) -> Result<GitDiffPreview, String> {
    let pathspec = format!(":(top,literal){}", entry.path);
    let text = if entry.index_status == '?' {
        let root = git_command_in(cwd, &["rev-parse", "--show-toplevel"])?;
        let path = std::path::Path::new(&root).join(&entry.path);
        let out = Command::new("git")
            .current_dir(cwd)
            .args([
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
            ])
            .arg(path)
            .output()
            .map_err(|e| e.to_string())?;
        // --no-index returns 1 when the files differ.
        if !matches!(out.status.code(), Some(0 | 1)) {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        let mut args = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color"];
        if staged {
            args.push("--cached");
        }
        args.extend(["--", &pathspec]);
        git_command_in(cwd, &args)?
    };
    Ok(GitDiffPreview {
        title: format!(
            "{} · {}",
            if staged {
                "Staged diff"
            } else {
                "Working tree diff"
            },
            entry.path
        ),
        text: bounded_diff(&text),
    })
}

pub fn collect_snapshot(cwd: &str) -> GitSnapshot {
    let mut snap = GitSnapshot::default();
    snap.is_repository = run_git_in(cwd, &["rev-parse", "--is-inside-work-tree"])
        .is_some_and(|s| s.trim() == "true");
    if !snap.is_repository {
        return snap;
    }

    if let Some(b) = run_git_in(cwd, &["branch", "--show-current"]) {
        snap.branch = b.trim().to_string();
    }

    if let Some(status_raw) = run_git_in(cwd, &["status", "--porcelain=v1", "-z"]) {
        let sep = if status_raw.as_bytes().contains(&0) {
            '\0'
        } else {
            '\n'
        };
        let mut iter = status_raw.split(sep);
        while let Some(entry) = iter.next() {
            if entry.len() < 4 {
                continue;
            }
            let bytes = entry.as_bytes();
            let x = bytes[0] as char;
            let y = bytes[1] as char;
            let path = entry[3..].to_string();
            if x == 'R' || x == 'C' {
                let _ = iter.next();
            }
            snap.files.push(GitFileEntry {
                path,
                index_status: x,
                worktree_status: y,
            });
        }
    }

    if let Some(log_raw) = run_git_in(cwd, &["log", "--pretty=format:%h|%an|%ar|%s", "-n", "50"]) {
        for line in log_raw.lines() {
            let mut parts = line.splitn(4, '|');
            if let (Some(h), Some(a), Some(d), Some(m)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            {
                snap.commits.push(GitCommitEntry {
                    hash: h.to_string(),
                    author: a.to_string(),
                    date: d.to_string(),
                    message: m.to_string(),
                });
            }
        }
    }

    if let Some(br_raw) = run_git_in(
        cwd,
        &["branch", "-a", "--format=%(refname:short)%09%(symref)"],
    ) {
        for line in br_raw.lines() {
            let (name, symbolic_target) = line.split_once('\t').unwrap_or((line, ""));
            if !name.is_empty() && symbolic_target.is_empty() {
                snap.branches.push(name.to_string());
            }
        }
    }

    if let Some(wt_raw) = run_git_in(cwd, &["worktree", "list", "--porcelain"]) {
        snap.worktrees = GitSnapshot::parse_worktrees(&wt_raw);
    }

    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRepo(std::path::PathBuf);

    impl TestRepo {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "gtuim-git-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn cwd(&self) -> &str {
            self.0.to_str().unwrap()
        }
        fn git(&self, args: &[&str]) -> String {
            git_command_in(self.cwd(), args).unwrap()
        }
        fn init(&self) {
            self.git(&["init", "-b", "main"]);
            self.git(&["config", "user.email", "test@example.com"]);
            self.git(&["config", "user.name", "Test"]);
        }
        fn write(&self, path: &str, text: &str) {
            std::fs::write(self.0.join(path), text).unwrap();
        }
    }
    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn detects_and_initializes_repository_with_unborn_branch() {
        let repo = TestRepo::new();
        assert!(!collect_snapshot(repo.cwd()).is_repository);
        repo.init();
        let snap = collect_snapshot(repo.cwd());
        assert!(snap.is_repository);
        assert_eq!(snap.branch, "main");
        assert!(snap.commits.is_empty());
        repo.write("new file.txt", "new content\n");
        let entry = collect_snapshot(repo.cwd()).files.remove(0);
        assert!(diff_file_in(repo.cwd(), &entry, false)
            .unwrap()
            .text
            .contains("+new content"));
        repo.git(&["add", "."]);
        let entry = collect_snapshot(repo.cwd()).files.remove(0);
        assert!(diff_file_in(repo.cwd(), &entry, true)
            .unwrap()
            .text
            .contains("+new content"));
    }

    #[test]
    fn previews_index_and_worktree_separately_without_mutation() {
        let repo = TestRepo::new();
        repo.init();
        repo.write("file [1].txt", "base\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "base"]);
        repo.write("file [1].txt", "staged\n");
        repo.git(&["add", "."]);
        repo.write("file [1].txt", "worktree\n");
        let before = repo.git(&["status", "--porcelain"]);
        let entry = collect_snapshot(repo.cwd()).files.remove(0);
        let staged = diff_file_in(repo.cwd(), &entry, true).unwrap().text;
        assert!(staged.contains("-base"));
        assert!(staged.contains("+staged"));
        let unstaged = diff_file_in(repo.cwd(), &entry, false).unwrap().text;
        assert!(unstaged.contains("-staged"));
        assert!(unstaged.contains("+worktree"));
        assert_eq!(before, repo.git(&["status", "--porcelain"]));
        std::fs::remove_file(repo.0.join("file [1].txt")).unwrap();
        assert!(diff_file_in(repo.cwd(), &entry, false)
            .unwrap()
            .text
            .contains("deleted file"));
    }

    #[test]
    fn switch_branch_preserves_changes_and_reports_conflicts() {
        let repo = TestRepo::new();
        repo.init();
        repo.write("file", "base\n");
        repo.git(&["add", "."]);
        repo.git(&["commit", "-m", "base"]);
        repo.git(&["switch", "-c", "feature"]);
        repo.write("file", "feature\n");
        repo.git(&["commit", "-am", "feature"]);
        switch_branch_in(repo.cwd(), "main").unwrap();
        repo.write("file", "local edit\n");
        assert!(switch_branch_in(repo.cwd(), "feature").is_err());
        assert_eq!(collect_snapshot(repo.cwd()).branch, "main");
        assert_eq!(
            std::fs::read_to_string(repo.0.join("file")).unwrap(),
            "local edit\n"
        );
        repo.git(&["restore", "file"]);
        switch_branch_in(repo.cwd(), "feature").unwrap();
        assert_eq!(collect_snapshot(repo.cwd()).branch, "feature");
        assert!(switch_branch_in(repo.cwd(), "--discard-changes").is_err());
    }

    #[test]
    fn switches_remote_tracking_branch() {
        let repo = TestRepo::new();
        repo.init();
        repo.git(&["commit", "--allow-empty", "-m", "base"]);
        repo.git(&["update-ref", "refs/remotes/origin/topic", "HEAD"]);
        repo.git(&["config", "remote.origin.url", repo.cwd()]);
        repo.git(&[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ]);
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/topic",
        ]);
        repo.git(&["update-ref", "refs/remotes/origin/HEAD-feature", "HEAD"]);
        let snap = collect_snapshot(repo.cwd());
        assert!(!snap.branches.iter().any(|b| b == "origin/HEAD"));
        assert!(snap.branches.iter().any(|b| b == "origin/HEAD-feature"));
        let remote = snap
            .branches
            .iter()
            .find(|b| b.ends_with("origin/topic"))
            .unwrap();
        switch_branch_in(repo.cwd(), remote).unwrap();
        assert_eq!(collect_snapshot(repo.cwd()).branch, "topic");
    }

    #[test]
    fn diff_display_is_bounded_and_preserves_utf8() {
        let long = "日".repeat(MAX_DIFF_BYTES);
        let bounded = bounded_diff(&long);
        assert!(bounded.len() < MAX_DIFF_BYTES + 100);
        assert!(bounded.ends_with("[Diff truncated: display limited to 256 KiB / 2000 lines]"));
        let lines = bounded_diff(&"line\n".repeat(MAX_DIFF_LINES + 100));
        assert!(lines.lines().count() <= MAX_DIFF_LINES + 2);
        assert!(lines.contains("Diff truncated"));
        assert_eq!(bounded_diff("small\n"), "small\n");
    }

    #[test]
    fn snapshot_parses_worktrees() {
        let raw = "worktree /repo/main\nHEAD 1234567890abcdef\nbranch refs/heads/main\n\nworktree /repo/feat\nHEAD fedcba0987654321\nbranch refs/heads/feat-x\n";
        let wts = GitSnapshot::parse_worktrees(raw);
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].path, "/repo/main");
        assert_eq!(wts[0].branch, "main");
        assert_eq!(wts[1].path, "/repo/feat");
        assert_eq!(wts[1].branch, "feat-x");
    }
}
