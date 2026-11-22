//! Name filter behind the search box: case-insensitive substring, or a `*` / `?` glob.

use crate::arena::{Arena, NO_NODE, NodeId, ROOT};

const MATCH: u8 = 1;
const MATCH_BELOW: u8 = 2;
const UNDER_MATCH: u8 = 4;

#[derive(Debug, Clone)]
pub struct NameFilter {
    pattern: Vec<u8>,
    is_glob: bool,
}

impl NameFilter {
    /// Returns `None` for blank input, which means "no filter".
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self {
            pattern: text.as_bytes().to_vec(),
            is_glob: text.contains(['*', '?']),
        })
    }

    /// Plain text matches anywhere in the name (so `iso` also finds `disk.iso`);
    /// a glob must match the whole name (so `*.mkv` does not find `a.mkv.part`).
    /// Comparison is ASCII case-insensitive and byte based, which avoids allocating per name.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        let name = name.as_bytes();
        if self.is_glob {
            glob_matches(&self.pattern, name)
        } else {
            name.windows(self.pattern.len())
                .any(|window| window.eq_ignore_ascii_case(&self.pattern))
        }
    }
}

/// Iterative matcher that remembers the last `*` instead of recursing.
fn glob_matches(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut last_star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&text[t])) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            last_star = Some((p, t));
            p += 1;
        } else if let Some((star, matched_until)) = last_star {
            p = star + 1;
            t = matched_until + 1;
            last_star = Some((star, matched_until + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&byte| byte == b'*')
}

/// Per-node result of applying a filter to a whole arena.
#[derive(Debug)]
pub struct FilterMask {
    flags: Vec<u8>,
    pub match_count: usize,
}

impl FilterMask {
    /// The scan root never counts as a match, so a filter cannot highlight the whole tree.
    #[must_use]
    pub fn build(arena: &Arena, filter: &NameFilter) -> Self {
        let mut flags = vec![0_u8; arena.nodes.len()];
        let mut match_count = 0;
        // Parents always have smaller ids than their children, so one forward pass sees them first
        for (index, node) in arena.nodes.iter().enumerate() {
            if node.is_deleted {
                continue;
            }
            let id = index as NodeId;
            if id != ROOT && filter.matches(arena.name(id)) {
                flags[index] |= MATCH;
                match_count += 1;
            }
            if node.parent != NO_NODE && flags[node.parent as usize] & (MATCH | UNDER_MATCH) != 0 {
                flags[index] |= UNDER_MATCH;
            }
        }
        for index in (1..arena.nodes.len()).rev() {
            // Only real matches propagate upwards: a directory is not "expanded for a match"
            // just because it contains files under another matching directory
            if flags[index] & (MATCH | MATCH_BELOW) != 0 {
                flags[arena.nodes[index].parent as usize] |= MATCH_BELOW;
            }
        }
        Self { flags, match_count }
    }

    /// Matches, their ancestors, and everything inside a matching directory.
    #[must_use]
    pub fn is_visible(&self, id: NodeId) -> bool {
        self.flags[id as usize] != 0
    }

    /// Directories that contain a match deeper down, which the tree opens automatically.
    #[must_use]
    pub fn has_match_below(&self, id: NodeId) -> bool {
        self.flags[id as usize] & MATCH_BELOW != 0
    }

    /// Nodes the treemap keeps at full brightness.
    #[must_use]
    pub fn is_highlighted(&self, id: NodeId) -> bool {
        self.flags[id as usize] & (MATCH | UNDER_MATCH) != 0
    }
}
