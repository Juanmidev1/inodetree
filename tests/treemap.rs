use std::time::Instant;

use inodetree::arena::{Arena, ChildBatch, NewNode, NodeId, NodeKind, ROOT};
use inodetree::treemap::{Bounds, MIN_SIDE, TreemapRect, hit_test, layout, squarify};

const EPSILON: f64 = 0.05;

fn overlap_area(a: &Bounds, b: &Bounds) -> f64 {
    let width = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let height = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if width <= 0.0 || height <= 0.0 {
        0.0
    } else {
        f64::from(width) * f64::from(height)
    }
}

fn is_inside(inner: &Bounds, outer: &Bounds) -> bool {
    let slack = 0.01;
    inner.x >= outer.x - slack
        && inner.y >= outer.y - slack
        && inner.x + inner.w <= outer.x + outer.w + slack
        && inner.y + inner.h <= outer.y + outer.h + slack
}

fn add_children(arena: &mut Arena, parent: NodeId, entries: &[(&str, NodeKind, u64)]) -> NodeId {
    let mut batch = ChildBatch::default();
    for &(name, kind, size) in entries {
        let name_start = batch.names.len();
        batch.names.push_str(name);
        batch.entries.push(NewNode {
            name_start,
            name_end: batch.names.len(),
            kind,
            logical_size: size,
            allocated_size: size,
            mtime_secs: 0,
            is_hardlink_duplicate: false,
        });
    }
    arena.append_children(parent, &batch)
}

/// Returns the arena plus the ids of `docs` and its first child `a`:
/// root / { docs/ { a (100), b (300) }, big.bin (600), empty (0) }, with `big.bin` at `docs + 1`
fn sample_arena() -> (Arena, NodeId, NodeId) {
    let mut arena = Arena::with_root("/r", 0, 0, 0);
    let first = add_children(
        &mut arena,
        ROOT,
        &[
            ("docs", NodeKind::Directory, 0),
            ("big.bin", NodeKind::File, 600),
            ("empty", NodeKind::File, 0),
        ],
    );
    let kids = add_children(
        &mut arena,
        first,
        &[("a", NodeKind::File, 100), ("b", NodeKind::File, 300)],
    );
    arena.aggregate();
    (arena, first, kids)
}

fn find_rect(rects: &[TreemapRect], id: NodeId) -> Option<&TreemapRect> {
    rects.iter().find(|rect| rect.id == id)
}

#[test]
fn areas_are_proportional_and_fill_the_bounds() {
    let bounds = Bounds::new(10.0, 20.0, 100.0, 100.0);
    let placed = squarify(&[60, 30, 10], bounds);

    assert_eq!(placed.len(), 3);
    let expected = [6000.0, 3000.0, 1000.0];
    for (rect, want) in placed.iter().zip(expected) {
        assert!((rect.area() - want).abs() < want * 0.001, "{rect:?}");
        assert!(is_inside(rect, &bounds));
    }
    let total: f64 = placed.iter().map(Bounds::area).sum();
    assert!((total - bounds.area()).abs() < 1.0);
}

#[test]
fn siblings_never_overlap_and_stay_squarish() {
    let bounds = Bounds::new(0.0, 0.0, 400.0, 300.0);
    let sizes: Vec<u64> = (1..=20).rev().map(|n| n * 100).collect();
    let placed = squarify(&sizes, bounds);

    assert_eq!(placed.len(), sizes.len());
    for (index, first) in placed.iter().enumerate() {
        assert!(is_inside(first, &bounds));
        assert!(
            first.w.max(first.h) / first.w.min(first.h) < 6.0,
            "{first:?}"
        );
        for second in &placed[index + 1..] {
            assert!(
                overlap_area(first, second) < EPSILON,
                "{first:?} overlaps {second:?}"
            );
        }
    }
}

#[test]
fn items_too_small_to_draw_stop_the_layout() {
    let mut sizes = vec![1_000_000_u64];
    sizes.extend(std::iter::repeat_n(1, 1000));
    let placed = squarify(&sizes, Bounds::new(0.0, 0.0, 200.0, 100.0));

    assert_eq!(placed.len(), 1);
}

#[test]
fn huge_directories_stay_cheap() {
    let sizes: Vec<u64> = (1..=200_000_u64).rev().collect();
    let bounds = Bounds::new(0.0, 0.0, 1200.0, 400.0);
    let started = Instant::now();
    let placed = squarify(&sizes, bounds);

    assert!(placed.len() as f64 <= bounds.area() / 4.0);
    assert!(
        started.elapsed().as_millis() < 1000,
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn empty_and_zero_inputs_place_nothing() {
    let bounds = Bounds::new(0.0, 0.0, 100.0, 100.0);
    assert_eq!(squarify(&[], bounds), Vec::<Bounds>::new());
    assert_eq!(squarify(&[0, 0], bounds), Vec::<Bounds>::new());
}

#[test]
fn nested_layout_keeps_children_inside_their_directory() {
    let (arena, docs, kids) = sample_arena();
    let bounds = Bounds::new(0.0, 0.0, 800.0, 400.0);
    let rects = layout(&arena, ROOT, bounds, false);

    assert_eq!(rects[0].id, ROOT);
    let docs_rect = find_rect(&rects, docs).unwrap();
    assert!(docs_rect.is_directory);
    for child in [kids, kids + 1] {
        assert!(is_inside(
            &find_rect(&rects, child).unwrap().bounds,
            &docs_rect.bounds
        ));
    }
    // Pre-order: a directory is listed before its children
    let position = |id| rects.iter().position(|rect| rect.id == id).unwrap();
    assert!(position(docs) < position(kids));
}

#[test]
fn zero_sized_and_deleted_nodes_are_skipped() {
    let (mut arena, docs, kids) = sample_arena();
    let bounds = Bounds::new(0.0, 0.0, 800.0, 400.0);
    assert!(find_rect(&layout(&arena, ROOT, bounds, false), docs + 2).is_none());

    arena.mark_deleted(docs);
    let rects = layout(&arena, ROOT, bounds, false);
    assert!(rects.iter().all(|rect| rect.id != docs && rect.id != kids));
}

#[test]
fn layout_is_deterministic_and_respects_the_minimum_side() {
    let (arena, _, _) = sample_arena();
    let bounds = Bounds::new(0.0, 0.0, 300.0, 120.0);
    let first = layout(&arena, ROOT, bounds, false);
    assert_eq!(first, layout(&arena, ROOT, bounds, false));
    assert!(
        first
            .iter()
            .skip(1)
            .all(|rect| rect.bounds.w >= MIN_SIDE && rect.bounds.h >= MIN_SIDE)
    );
}

#[test]
fn hit_test_returns_the_deepest_rectangle() {
    let (arena, _, kids) = sample_arena();
    let rects = layout(&arena, ROOT, Bounds::new(0.0, 0.0, 800.0, 400.0), false);
    let leaf = find_rect(&rects, kids + 1).unwrap().bounds;

    let center = (leaf.x + leaf.w / 2.0, leaf.y + leaf.h / 2.0);
    assert_eq!(hit_test(&rects, center.0, center.1), Some(kids + 1));
    assert_eq!(hit_test(&rects, -5.0, -5.0), None);
}

#[test]
fn ancestry_and_nearest_live_follow_the_parent_chain() {
    let (mut arena, docs, kids) = sample_arena();
    assert_eq!(arena.ancestry(kids), [ROOT, docs, kids]);

    arena.mark_deleted(docs);
    assert_eq!(arena.nearest_live(kids), ROOT);
    assert_eq!(arena.nearest_live(docs + 1), docs + 1);
}
