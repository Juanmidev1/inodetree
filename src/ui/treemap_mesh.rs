//! Colors and the single shared mesh that draws every treemap rectangle.

use eframe::egui::{Color32, Mesh, Rect, pos2, vec2};

use super::treemap::TreemapView;
use crate::treemap::{Bounds, TreemapRect};

pub(super) const BACKGROUND: Color32 = Color32::from_rgb(22, 24, 30);
const DIRECTORY_FILL: Color32 = Color32::from_rgb(52, 58, 72);
const DIM_AMOUNT: f32 = 0.75;

pub(super) fn build_mesh(rects: &[TreemapRect], view: &TreemapView<'_>) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.reserve_triangles(rects.len() * 2);
    mesh.reserve_vertices(rects.len() * 4);
    for rect in rects {
        // Files are inset by half a pixel so the directory color shows through as a thin grid
        let inset = if rect.is_directory { 0.0 } else { 0.5 };
        mesh.add_colored_rect(to_rect(rect.bounds, inset), fill_color(rect, view));
    }
    mesh
}

pub(super) fn fill_color(rect: &TreemapRect, view: &TreemapView<'_>) -> Color32 {
    if rect.is_directory {
        return DIRECTORY_FILL;
    }
    let node = &view.arena.nodes[rect.id as usize];
    let color = view
        .colors
        .get(usize::from(node.ext_id))
        .copied()
        .unwrap_or(Color32::GRAY);
    let outside_extension = view.highlighted_ext.is_some_and(|ext| ext != node.ext_id);
    let outside_filter = view
        .filter
        .is_some_and(|filter| !filter.is_highlighted(rect.id));
    if outside_extension || outside_filter {
        color.lerp_to_gamma(BACKGROUND, DIM_AMOUNT)
    } else {
        color
    }
}

pub(super) fn to_rect(bounds: Bounds, inset: f32) -> Rect {
    Rect::from_min_size(
        pos2(bounds.x + inset, bounds.y + inset),
        vec2(bounds.w - 2.0 * inset, bounds.h - 2.0 * inset),
    )
}
