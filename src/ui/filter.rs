//! Bounded, subtree-scoped name filtering.
//!
//! The filter is intentionally recursive: typing `libw` at a scan root finds
//! real entries below it, not only direct children. Results are capped by both
//! visits and matches so a large volume can never stall input. Matched folders
//! keep their complete size; the paths between the current folder and a match
//! become containers whose size is the sum of the matching entries beneath
//! them. A container's tile therefore reports filtered bytes, not invented
//! totals.

use crate::model::{Category, NodeId, Tree};

/// A large volume needs a predictable upper bound for synchronous input.
pub const MAX_VISITS: usize = 120_000;
pub const MAX_MATCHES: usize = 2_000;

#[derive(Debug, Clone)]
pub struct FilterNode {
    pub id: NodeId,
    pub size: u64,
    pub cat: Category,
    pub is_dir: bool,
    /// True when the query matched this entry itself. Matching folders are
    /// leaves in the filtered map; activating one navigates into it.
    pub is_match: bool,
    pub children: Vec<FilterNode>,
}

#[derive(Debug, Clone)]
pub struct FilterView {
    pub query: String,
    pub roots: Vec<FilterNode>,
    pub match_count: usize,
    pub matched_files: u64,
    pub matched_bytes: u64,
    pub truncated: bool,
    pub visits: usize,
}

impl FilterView {
    /// Build a filtered hierarchy for `current` (exclusive). Hidden categories
    /// are excluded from matches while visible descendants of a hidden-dominant
    /// folder remain discoverable.
    pub fn build(tree: &Tree, current: NodeId, query: &str, hidden: &[bool; 8]) -> Option<Self> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() || current as usize >= tree.len() {
            return None;
        }

        let mut stack = vec![current];
        let mut matches = Vec::new();
        let mut visits = 0usize;
        let mut truncated = false;

        while let Some(id) = stack.pop() {
            if visits >= MAX_VISITS || matches.len() >= MAX_MATCHES {
                truncated = true;
                break;
            }
            visits += 1;
            let node = tree.node(id);
            let name = tree.name(id);
            if name_matches(name, &needle) && !hidden[node.cat.id() as usize] {
                matches.push(id);
                // A matching folder subsumes all of its contents. Matching
                // files have no children to visit.
                continue;
            }
            if node.is_dir {
                let children = tree.children(id);
                for child in children.into_iter().rev() {
                    if tree.node(child).is_dir || !type_hidden(tree, child, hidden) {
                        stack.push(child);
                    }
                }
            }
        }
        if !stack.is_empty() {
            truncated = true;
        }

        let mut roots: Vec<FilterNode> = Vec::new();
        let mut matched_files = 0u64;
        let mut matched_bytes = 0u64;
        for id in &matches {
            let node = tree.node(*id);
            if node.is_dir {
                matched_files = matched_files.saturating_add(node.file_count);
            } else {
                matched_files = matched_files.saturating_add(1);
            }
            matched_bytes = matched_bytes.saturating_add(node.size);
            let lineage = tree.lineage(*id);
            if let Some(start) = lineage.iter().position(|entry| *entry == current) {
                let tail = &lineage[start + 1..];
                if !tail.is_empty() {
                    insert_path(&mut roots, tree, tail, node.size);
                }
            }
        }
        sort_nodes(tree, &mut roots);

        Some(Self {
            query: needle,
            roots,
            match_count: matches.len(),
            matched_files,
            matched_bytes,
            truncated,
            visits,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// All matched ids in display order. Used by keyboard selection while a
    /// query is active; containers are navigation context, not results.
    pub fn match_ids(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        collect_matches(&self.roots, &mut out);
        out
    }
}

fn type_hidden(tree: &Tree, id: NodeId, hidden: &[bool; 8]) -> bool {
    hidden[tree.node(id).cat.id() as usize]
}

/// Case-insensitive contains without allocating when the name is too short to
/// match. Unicode names go through the same lowercase path as search.
fn name_matches(name: &str, needle: &str) -> bool {
    if name.len() < needle.len() {
        return false;
    }
    name.to_lowercase().contains(needle)
}

fn insert_path(nodes: &mut Vec<FilterNode>, tree: &Tree, path: &[NodeId], contribution: u64) {
    let Some((&id, rest)) = path.split_first() else {
        return;
    };
    let is_match = rest.is_empty();
    if let Some(existing) = nodes.iter_mut().find(|node| node.id == id) {
        existing.size = existing.size.saturating_add(contribution);
        if !is_match {
            insert_path(&mut existing.children, tree, rest, contribution);
        }
        return;
    }

    let model = tree.node(id);
    let mut node = FilterNode {
        id,
        size: contribution,
        cat: model.cat,
        is_dir: model.is_dir,
        is_match,
        children: Vec::new(),
    };
    if !is_match {
        insert_path(&mut node.children, tree, rest, contribution);
    }
    nodes.push(node);
}

fn sort_nodes(tree: &Tree, nodes: &mut [FilterNode]) {
    nodes.sort_by(|a, b| {
        b.size.cmp(&a.size).then_with(|| {
            tree.name(a.id)
                .to_lowercase()
                .cmp(&tree.name(b.id).to_lowercase())
        })
    });
    for node in nodes {
        if !node.children.is_empty() {
            sort_nodes(tree, &mut node.children);
        }
    }
}

fn collect_matches(nodes: &[FilterNode], out: &mut Vec<NodeId>) {
    for node in nodes {
        if node.is_match {
            out.push(node.id);
        } else {
            collect_matches(&node.children, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, CategoryTotals, TreeBuilder};

    fn fixture() -> Tree {
        let mut b = TreeBuilder::new("C:\\fixture", "fixture");
        let media = b.add_node("media", 0, true, 0, Category::Media);
        let deep = b.add_node("deep", media, true, 0, Category::Media);
        b.add_node("libwinter.mp4", deep, false, 4_000, Category::Media);
        b.add_node("notes.txt", deep, false, 100, Category::Documents);
        let code = b.add_node("src", 0, true, 0, Category::Code);
        let nested = b.add_node("nested", code, true, 0, Category::Code);
        b.add_node("libwindow.rs", nested, false, 2_000, Category::Code);
        b.add_node("libwire.rs", code, false, 1_000, Category::Code);
        b.add_node("README.md", 0, false, 50, Category::Documents);
        b.finish(Vec::new(), CategoryTotals::default())
    }

    #[test]
    fn finds_deep_matches_and_keeps_chain() {
        let tree = fixture();
        let view = FilterView::build(&tree, 0, "libw", &[false; 8]).unwrap();
        assert_eq!(view.match_count, 3);
        assert_eq!(view.matched_files, 3);
        assert_eq!(view.matched_bytes, 1_000 + 2_000 + 4_000);
        // Direct roots are the ancestors of the matches, not a flat list.
        assert_eq!(view.roots.len(), 2);
        let ids = view.match_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().any(|id| tree.name(*id) == "libwindow.rs"));
        assert!(ids.iter().any(|id| tree.name(*id) == "libwinter.mp4"));
        assert!(ids.iter().any(|id| tree.name(*id) == "libwire.rs"));
    }

    #[test]
    fn matching_folder_is_a_leaf_and_sums_full_size() {
        let tree = fixture();
        let view = FilterView::build(&tree, 0, "deep", &[false; 8]).unwrap();
        assert_eq!(view.match_count, 1);
        let media = view
            .roots
            .iter()
            .find(|n| tree.name(n.id) == "media")
            .unwrap();
        let deep = media
            .children
            .iter()
            .find(|n| tree.name(n.id) == "deep")
            .unwrap();
        assert!(deep.is_match);
        assert!(deep.children.is_empty());
        assert_eq!(deep.size, 4_100);
    }

    #[test]
    fn hidden_category_does_not_hide_visible_descendants() {
        let tree = fixture();
        let mut hidden = [false; 8];
        hidden[Category::Media.id() as usize] = true;
        let view = FilterView::build(&tree, 0, "libw", &[false; 8]).unwrap();
        assert_eq!(view.match_count, 3);
        let view_hidden = FilterView::build(&tree, 0, "libw", &hidden).unwrap();
        assert_eq!(view_hidden.match_count, 2);
        assert!(
            view_hidden
                .match_ids()
                .iter()
                .all(|id| tree.node(*id).cat != Category::Media)
        );
    }

    #[test]
    fn scopes_to_current_folder_and_never_matches_it() {
        let tree = fixture();
        let code = tree
            .children(0)
            .into_iter()
            .find(|id| tree.name(*id) == "src")
            .unwrap();
        let view = FilterView::build(&tree, code, "libw", &[false; 8]).unwrap();
        assert_eq!(view.match_count, 2);
        assert_eq!(view.roots.len(), 2);
        assert!(view.roots.iter().any(|node| tree.name(node.id) == "nested"));
    }
}
