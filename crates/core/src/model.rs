//! Value types for repo state. Pure data, no IO.

/// Counts of working-tree changes, derived from `git status --porcelain=v2`.
///
/// `staged` / `unstaged` count index- vs worktree-side changes; `untracked`
/// counts files git isn't tracking yet; `conflicts` counts unmerged entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkingTree {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicts: u32,
}

impl WorkingTree {
    /// Nothing changed, nothing untracked, nothing unmerged.
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.unstaged == 0 && self.untracked == 0 && self.conflicts == 0
    }

    /// Are there *changes* that a push would not capture (i.e. not yet committed)?
    /// Untracked files are excluded — they are not modifications to tracked content,
    /// but a `push` summary still surfaces them separately.
    pub fn has_uncommitted_changes(&self) -> bool {
        self.staged != 0 || self.unstaged != 0 || self.conflicts != 0
    }
}

/// How far a local branch is ahead of / behind one remote-tracking branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AheadBehind {
    pub ahead: u32,
    pub behind: u32,
}
