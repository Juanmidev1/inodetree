use inodetree::arena::{Arena, ChildBatch, NO_NODE, NewNode, NodeId, NodeKind, ROOT};
use inodetree::stats;

/// Builds: root / { big.mkv (3000), docs/ { a.txt (100), b.txt (200) }, small.txt (10) }
fn sample_arena() -> (Arena, NodeId) {
    let mut arena = Arena::with_root("/data", 0, 0, 1);
    let mut batch = ChildBatch::default();
    for (name, kind, size) in [
        ("small.txt", NodeKind::File, 10),
        ("docs", NodeKind::Directory, 0),
        ("big.mkv", NodeKind::File, 3000),
    ] {
        let name_start = batch.names.len();
        batch.names.push_str(name);
        batch.entries.push(NewNode {
            name_start,
            name_end: batch.names.len(),
            kind,
            logical_size: size,
            allocated_size: size,
            mtime_secs: 5,
            is_hardlink_duplicate: false,
        });
    }
    let first = arena.append_children(ROOT, &batch);
    let docs = first + 1;

    let mut docs_batch = ChildBatch::default();
    for (name, size) in [("a.txt", 100), ("b.txt", 200)] {
        let name_start = docs_batch.names.len();
        docs_batch.names.push_str(name);
        docs_batch.entries.push(NewNode {
            name_start,
            name_end: docs_batch.names.len(),
            kind: NodeKind::File,
            logical_size: size,
            allocated_size: size,
            mtime_secs: 9,
            is_hardlink_duplicate: false,
        });
    }
    arena.append_children(docs, &docs_batch);
    arena.aggregate();
    (arena, docs)
}

#[test]
fn aggregation_rolls_totals_up() {
    let (arena, docs) = sample_arena();
    let root = &arena.nodes[ROOT as usize];
    assert_eq!(root.allocated_size, 3310);
    assert_eq!((root.file_count, root.dir_count), (4, 1));
    assert_eq!(root.mtime_secs, 9);
    assert_eq!(arena.nodes[docs as usize].allocated_size, 300);
}

#[test]
fn children_are_sorted_by_size_descending() {
    let (arena, _) = sample_arena();
    let names: Vec<_> = arena.children(ROOT).map(|id| arena.name(id)).collect();
    assert_eq!(names, ["big.mkv", "docs", "small.txt"]);
}

#[test]
fn paths_are_rebuilt_from_parents() {
    let (arena, docs) = sample_arena();
    let child = arena.children(docs).next().unwrap();
    assert_eq!(arena.path_of(child), "/data/docs/b.txt");
}

#[test]
fn deleting_a_folder_updates_every_ancestor() {
    let (mut arena, docs) = sample_arena();
    arena.mark_deleted(docs);
    let root = &arena.nodes[ROOT as usize];
    assert_eq!(root.allocated_size, 3010);
    assert_eq!((root.file_count, root.dir_count), (2, 0));
    assert!(
        arena
            .children(docs)
            .all(|id| arena.nodes[id as usize].is_deleted)
    );
}

#[test]
fn deleting_twice_does_not_subtract_twice() {
    let (mut arena, docs) = sample_arena();
    arena.mark_deleted(docs);
    arena.mark_deleted(docs);
    assert_eq!(arena.nodes[ROOT as usize].allocated_size, 3010);
}

#[test]
fn stats_ignore_deleted_nodes_and_exclude_root() {
    let (mut arena, docs) = sample_arena();
    let top_files = stats::top_files(&arena, 2);
    assert_eq!(arena.name(top_files[0]), "big.mkv");
    assert_eq!(stats::top_dirs(&arena, 5), [docs]);

    arena.mark_deleted(docs);
    assert_eq!(stats::top_dirs(&arena, 5), Vec::<u32>::new());
    let breakdown = stats::extension_breakdown(&arena);
    assert_eq!(breakdown.len(), 2);
    assert_eq!(arena.extensions.name(breakdown[0].ext_id), "mkv");
}

#[test]
fn leaf_nodes_have_no_children() {
    let (arena, _) = sample_arena();
    let big = arena.children(ROOT).next().unwrap();
    assert_eq!(arena.nodes[big as usize].first_child, NO_NODE);
}
