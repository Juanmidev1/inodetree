//! Squarified treemap layout (Bruls, Huizing, van Wijk). Pure geometry with no GUI types.

mod squarify;

use crate::arena::{Arena, NodeId, NodeKind};

pub use squarify::squarify;

/// Rectangles thinner than this are not worth drawing or hit-testing.
pub const MIN_SIDE: f32 = 2.0;
/// Inner margin of a directory, so nesting stays visible.
const DIRECTORY_PADDING: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bounds {
    #[must_use]
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn area(&self) -> f64 {
        f64::from(self.w) * f64::from(self.h)
    }

    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn shrunk(&self, amount: f32) -> Option<Self> {
        let (w, h) = (self.w - 2.0 * amount, self.h - 2.0 * amount);
        (w > 0.0 && h > 0.0).then_some(Self::new(self.x + amount, self.y + amount, w, h))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreemapRect {
    pub id: NodeId,
    pub bounds: Bounds,
    pub is_directory: bool,
}

/// Nested layout of `root` inside `bounds`, in pre-order so parents come before their children.
/// Children of every directory must already be sorted by `use_logical` size, as `Arena` keeps them.
#[must_use]
pub fn layout(arena: &Arena, root: NodeId, bounds: Bounds, use_logical: bool) -> Vec<TreemapRect> {
    let mut rects = vec![TreemapRect {
        id: root,
        bounds,
        is_directory: true,
    }];
    layout_children(arena, root, bounds, use_logical, &mut rects);
    rects
}

fn layout_children(
    arena: &Arena,
    directory: NodeId,
    bounds: Bounds,
    use_logical: bool,
    out: &mut Vec<TreemapRect>,
) {
    let Some(inner) = bounds.shrunk(DIRECTORY_PADDING) else {
        return;
    };
    let children: Vec<(NodeId, u64)> = arena
        .children(directory)
        .map(|id| (id, &arena.nodes[id as usize]))
        .filter(|(_, node)| !node.is_deleted && node.size(use_logical) > 0)
        .map(|(id, node)| (id, node.size(use_logical)))
        .collect();
    let sizes: Vec<u64> = children.iter().map(|&(_, size)| size).collect();
    for (&(id, _), child_bounds) in children.iter().zip(squarify(&sizes, inner)) {
        if child_bounds.w < MIN_SIDE || child_bounds.h < MIN_SIDE {
            continue;
        }
        let is_directory = arena.nodes[id as usize].kind == NodeKind::Directory;
        out.push(TreemapRect {
            id,
            bounds: child_bounds,
            is_directory,
        });
        if is_directory {
            layout_children(arena, id, child_bounds, use_logical, out);
        }
    }
}

/// Deepest rectangle under the point. Pre-order layout means later entries are nested deeper.
#[must_use]
pub fn hit_test(rects: &[TreemapRect], x: f32, y: f32) -> Option<NodeId> {
    rects
        .iter()
        .rev()
        .find(|rect| rect.bounds.contains(x, y))
        .map(|rect| rect.id)
}
