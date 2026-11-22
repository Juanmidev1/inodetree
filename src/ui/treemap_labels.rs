//! Name labels drawn inside treemap rectangles that are large enough to hold them.

use eframe::egui::{Align2, Color32, FontId, Painter, Rect, vec2};

use crate::arena::{Arena, NodeId};
use crate::treemap::TreemapRect;

const MIN_LABEL_WIDTH: f32 = 64.0;
const MIN_LABEL_HEIGHT: f32 = 20.0;
const LABEL_FONT_SIZE: f32 = 11.0;
const LABEL_OFFSET: [f32; 2] = [4.0, 2.0];
const DARK_TEXT: Color32 = Color32::from_rgb(20, 20, 24);

pub struct LabelSpot {
    pub id: NodeId,
    pub rect: Rect,
    pub color: Color32,
}

/// Picks the rectangles worth labelling. `fill` gives each rectangle's color so the text contrasts.
/// A directory is skipped when its children are drawn on top of it, since the text would collide.
pub fn collect(
    rects: &[TreemapRect],
    arena: &Arena,
    fill: impl Fn(&TreemapRect) -> Color32,
    to_rect: impl Fn(&TreemapRect) -> Rect,
) -> Vec<LabelSpot> {
    // The first entry is the zoomed directory itself, which the breadcrumb already names
    (1..rects.len())
        .filter(|&index| {
            let rect = &rects[index];
            let fits = rect.bounds.w >= MIN_LABEL_WIDTH && rect.bounds.h >= MIN_LABEL_HEIGHT;
            fits && !(rect.is_directory && has_drawn_children(rects, index, arena))
        })
        .map(|index| LabelSpot {
            id: rects[index].id,
            rect: to_rect(&rects[index]),
            color: text_color_for(fill(&rects[index])),
        })
        .collect()
}

/// Layout is in pre-order, so a directory's first drawn child directly follows it.
fn has_drawn_children(rects: &[TreemapRect], index: usize, arena: &Arena) -> bool {
    rects
        .get(index + 1)
        .is_some_and(|next| arena.nodes[next.id as usize].parent == rects[index].id)
}

fn text_color_for(background: Color32) -> Color32 {
    let [red, green, blue, _] = background.to_array();
    let luminance = 0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
    if luminance > 150.0 {
        DARK_TEXT
    } else {
        Color32::WHITE
    }
}

pub fn paint(painter: &Painter, panel: Rect, arena: &Arena, spots: &[LabelSpot]) {
    for spot in spots {
        // Clipping to the rectangle keeps long names from spilling into neighbouring blocks
        painter.with_clip_rect(spot.rect.intersect(panel)).text(
            spot.rect.min + vec2(LABEL_OFFSET[0], LABEL_OFFSET[1]),
            Align2::LEFT_TOP,
            arena.name(spot.id),
            FontId::proportional(LABEL_FONT_SIZE),
            spot.color,
        );
    }
}
