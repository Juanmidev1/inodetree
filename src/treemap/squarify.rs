//! Row-by-row squarified placement of sorted sizes inside a rectangle.

use super::Bounds;

/// Items below this area (in square pixels) cannot reach `MIN_SIDE` on both sides.
const MIN_AREA: f64 = 4.0;

/// Places `sizes` (positive, in descending order) inside `bounds` with areas proportional to size.
/// Stops at the first item too small to draw, which bounds the cost for huge directories.
#[must_use]
pub fn squarify(sizes: &[u64], bounds: Bounds) -> Vec<Bounds> {
    let total: f64 = sizes.iter().map(|&size| size as f64).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let scale = bounds.area() / total;
    let mut placed = Vec::new();
    let mut remaining = bounds;
    let mut start = 0;
    while start < sizes.len() {
        let side = f64::from(remaining.w.min(remaining.h));
        let row = grow_row(sizes, start, scale, side);
        if row.end == start {
            break;
        }
        place_row(
            &sizes[start..row.end],
            scale,
            row.area,
            &mut remaining,
            &mut placed,
        );
        if row.hit_cutoff {
            break;
        }
        start = row.end;
    }
    placed
}

struct Row {
    end: usize,
    area: f64,
    hit_cutoff: bool,
}

/// Adds items to the row while the worst aspect ratio keeps improving.
fn grow_row(sizes: &[u64], start: usize, scale: f64, side: f64) -> Row {
    let (mut sum, mut min, mut max) = (0.0_f64, f64::MAX, 0.0_f64);
    let mut best_ratio = f64::MAX;
    let mut end = start;
    while end < sizes.len() {
        let area = sizes[end] as f64 * scale;
        if area < MIN_AREA {
            return Row {
                end,
                area: sum,
                hit_cutoff: true,
            };
        }
        let (new_sum, new_min, new_max) = (sum + area, min.min(area), max.max(area));
        let ratio = worst_ratio(new_sum, new_min, new_max, side);
        if end > start && ratio > best_ratio {
            break;
        }
        (sum, min, max, best_ratio) = (new_sum, new_min, new_max, ratio);
        end += 1;
    }
    Row {
        end,
        area: sum,
        hit_cutoff: false,
    }
}

fn worst_ratio(sum: f64, min: f64, max: f64, side: f64) -> f64 {
    let side_squared = side * side;
    (side_squared * max / (sum * sum)).max(sum * sum / (side_squared * min))
}

/// Lays the row as a strip along the shorter side of `remaining`, then shrinks `remaining`.
fn place_row(
    sizes: &[u64],
    scale: f64,
    row_area: f64,
    remaining: &mut Bounds,
    out: &mut Vec<Bounds>,
) {
    let is_vertical_strip = remaining.w >= remaining.h;
    let side = if is_vertical_strip {
        remaining.h
    } else {
        remaining.w
    };
    let thickness = (row_area / f64::from(side)) as f32;
    let mut offset = 0.0_f32;
    for &size in sizes {
        let length = (size as f64 * scale / f64::from(thickness)) as f32;
        out.push(if is_vertical_strip {
            Bounds::new(remaining.x, remaining.y + offset, thickness, length)
        } else {
            Bounds::new(remaining.x + offset, remaining.y, length, thickness)
        });
        offset += length;
    }
    if is_vertical_strip {
        remaining.x += thickness;
        remaining.w -= thickness;
    } else {
        remaining.y += thickness;
        remaining.h -= thickness;
    }
}
