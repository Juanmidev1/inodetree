use inodetree::arena::{Arena, ChildBatch, NewNode, NodeId, NodeKind, ROOT};
use inodetree::filter::{FilterMask, NameFilter};

fn matches(pattern: &str, name: &str) -> bool {
    NameFilter::parse(pattern).unwrap().matches(name)
}

fn add_children(arena: &mut Arena, parent: NodeId, entries: &[(&str, NodeKind)]) -> NodeId {
    let mut batch = ChildBatch::default();
    for &(name, kind) in entries {
        let name_start = batch.names.len();
        batch.names.push_str(name);
        batch.entries.push(NewNode {
            name_start,
            name_end: batch.names.len(),
            kind,
            logical_size: 10,
            allocated_size: 10,
            mtime_secs: 0,
            is_hardlink_duplicate: false,
        });
    }
    arena.append_children(parent, &batch)
}

// root / { movies/ { a.mkv, b.txt }, node_modules/ { lib.js }, notes.txt }
// ids: movies 1, node_modules 2, notes.txt 3, a.mkv 4, b.txt 5, lib.js 6
fn sample() -> Arena {
    let mut arena = Arena::with_root("/r", 0, 0, 0);
    let top = add_children(
        &mut arena,
        ROOT,
        &[
            ("movies", NodeKind::Directory),
            ("node_modules", NodeKind::Directory),
            ("notes.txt", NodeKind::File),
        ],
    );
    add_children(
        &mut arena,
        top,
        &[("a.mkv", NodeKind::File), ("b.txt", NodeKind::File)],
    );
    add_children(&mut arena, top + 1, &[("lib.js", NodeKind::File)]);
    arena.aggregate();
    arena
}

#[test]
fn blank_input_means_no_filter() {
    assert!(NameFilter::parse("").is_none());
    assert!(NameFilter::parse("   ").is_none());
}

#[test]
fn plain_text_is_a_case_insensitive_substring() {
    assert!(matches("iso", "Ubuntu.ISO"));
    assert!(matches("node_modules", "node_modules"));
    assert!(matches(" mkv ", "movie.mkv"));
    assert!(!matches("iso", "movie.mkv"));
    assert!(!matches("longer-than-name", "short"));
}

#[test]
fn globs_match_the_whole_name() {
    assert!(matches("*.mkv", "movie.MKV"));
    assert!(!matches("*.mkv", "movie.mkv.part"));
    assert!(matches("node_*", "node_modules"));
    assert!(matches("?at", "cat"));
    assert!(!matches("?at", "at"));
    assert!(matches("a*b*c", "aXXbYYc"));
    assert!(matches("*", "anything"));
    assert!(matches("*.tar.*", "backup.tar.gz"));
    assert!(!matches("*.iso", "iso"));
}

#[test]
fn mask_marks_matches_ancestors_and_matching_directory_contents() {
    let arena = sample();
    let mask = FilterMask::build(&arena, &NameFilter::parse("*.mkv").unwrap());

    assert_eq!(mask.match_count, 1);
    assert!(mask.is_visible(4) && mask.is_highlighted(4));
    // The directory holding the match is visible and opened, but not highlighted itself
    assert!(mask.is_visible(1) && mask.has_match_below(1) && !mask.is_highlighted(1));
    assert!(!mask.is_visible(2) && !mask.is_visible(3) && !mask.is_visible(5));
}

#[test]
fn a_matching_directory_shows_and_highlights_its_contents_without_opening_itself() {
    let arena = sample();
    let mask = FilterMask::build(&arena, &NameFilter::parse("node_modules").unwrap());

    assert!(mask.is_visible(2) && mask.is_highlighted(2));
    assert!(mask.is_visible(6) && mask.is_highlighted(6));
    assert!(!mask.has_match_below(2));
}

#[test]
fn the_root_is_never_a_match_and_deleted_nodes_are_ignored() {
    let mut arena = sample();
    assert_eq!(
        FilterMask::build(&arena, &NameFilter::parse("/r").unwrap()).match_count,
        0
    );

    arena.mark_deleted(1);
    let mask = FilterMask::build(&arena, &NameFilter::parse("*.mkv").unwrap());
    assert_eq!(mask.match_count, 0);
}
