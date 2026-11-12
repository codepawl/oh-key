//! The settings window's geometry as plain numbers, so it can be tested without a window.
//! The constants are oh-snap's, except the width and label column, which Vietnamese needs more of.

pub const WIDTH: f32 = 1200.0;
pub const PADDING: f32 = 32.0;
pub const TITLE_HEIGHT: f32 = 72.0;
pub const COLUMN_GAP: f32 = 48.0;
pub const SECTION_HEIGHT: f32 = 44.0;
pub const SECTION_GAP: f32 = 18.0;
pub const ROW_HEIGHT: f32 = 42.0;
pub const LABEL_WIDTH: f32 = 228.0;
pub const ICON_SLOT: f32 = 26.0;
pub const BUTTON_HEIGHT: f32 = 30.0;
pub const BUTTON_PADDING: f32 = 20.0;
pub const BUTTON_GAP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn contains(&self, point: (f32, f32)) -> bool {
        point.0 >= self.left && point.0 < self.right && point.1 >= self.top && point.1 < self.bottom
    }
}

pub fn column_width() -> f32 {
    (WIDTH - 2.0 * PADDING - COLUMN_GAP) / 2.0
}

pub fn column_left(column_index: usize) -> f32 {
    PADDING + column_index as f32 * (column_width() + COLUMN_GAP)
}

/// Height of one column, given the number of rows in each of its groups.
pub fn column_height(rows_per_section: &[usize]) -> f32 {
    let sections_height: f32 = rows_per_section
        .iter()
        .map(|rows| SECTION_HEIGHT + *rows as f32 * ROW_HEIGHT)
        .sum();
    sections_height + SECTION_GAP * rows_per_section.len().saturating_sub(1) as f32
}

pub fn window_height(left_rows: &[usize], right_rows: &[usize]) -> f32 {
    TITLE_HEIGHT + column_height(left_rows).max(column_height(right_rows)) + PADDING
}

/// The monitor's scale, or less when the window would not fit its work area, as on a
/// 1080p laptop at 150%. Everything shrinks together, so no row is cut off.
pub fn fitted_scale(monitor_scale: f32, window_size: (f32, f32), room: (f32, f32)) -> f32 {
    let fit_width = room.0 / window_size.0;
    let fit_height = room.1 / window_size.1;
    monitor_scale.min(fit_width).min(fit_height)
}

/// Button rectangles for one row, laid out from the right edge so every row lines up on the
/// right. `text_widths` are in option order; the result is too.
pub fn button_rects(row_top: f32, right_edge: f32, text_widths: &[f32]) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(text_widths.len());
    let mut right = right_edge;
    for text_width in text_widths.iter().rev() {
        let rect = Rect {
            left: right - text_width - BUTTON_PADDING,
            top: row_top + (ROW_HEIGHT - BUTTON_HEIGHT) / 2.0,
            right,
            bottom: row_top + (ROW_HEIGHT + BUTTON_HEIGHT) / 2.0,
        };
        right = rect.left - BUTTON_GAP;
        rects.push(rect);
    }
    rects.reverse();
    rects
}

/// The first rectangle that holds the point.
pub fn hit_test<Target: Copy>(hits: &[(Target, Rect)], point: (f32, f32)) -> Option<Target> {
    hits.iter()
        .find(|(_, rect)| rect.contains(point))
        .map(|(target, _)| *target)
}

/// Pixels in the window's client area to device-independent pixels at the target's DPI.
pub fn pixels_to_dips(pixel: (i32, i32), dpi: (f32, f32)) -> (f32, f32) {
    (pixel.0 as f32 * 96.0 / dpi.0, pixel.1 as f32 * 96.0 / dpi.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_keeps_the_monitor_scale_when_it_fits() {
        assert_eq!(fitted_scale(1.0, (1200.0, 700.0), (1920.0, 1000.0)), 1.0);
    }

    #[test]
    fn the_window_shrinks_to_fit_a_small_work_area() {
        let scale = fitted_scale(1.5, (1200.0, 700.0), (1920.0, 1000.0));
        assert!(scale < 1.5);
        assert!(700.0 * scale <= 1000.0);
    }

    #[test]
    fn a_column_adds_groups_rows_and_gaps() {
        let expected = 2.0 * SECTION_HEIGHT + 5.0 * ROW_HEIGHT + SECTION_GAP;
        assert_eq!(column_height(&[3, 2]), expected);
        assert_eq!(column_height(&[]), 0.0);
    }

    #[test]
    fn the_window_is_as_tall_as_its_taller_column() {
        let height = window_height(&[5], &[1, 1]);
        assert_eq!(height, TITLE_HEIGHT + column_height(&[5]) + PADDING);
    }

    #[test]
    fn the_two_columns_do_not_overlap() {
        assert!(column_left(0) + column_width() < column_left(1));
        assert_eq!(column_left(1) + column_width() + PADDING, WIDTH);
    }

    #[test]
    fn buttons_line_up_on_the_right_edge_in_option_order() {
        let rects = button_rects(100.0, 500.0, &[40.0, 20.0]);
        assert_eq!(rects[1].right, 500.0);
        assert_eq!(rects[1].left, 500.0 - 20.0 - BUTTON_PADDING);
        assert_eq!(rects[0].right, rects[1].left - BUTTON_GAP);
        assert!(rects[0].left < rects[1].left);
    }

    #[test]
    fn buttons_are_centred_in_their_row() {
        let rect = button_rects(100.0, 500.0, &[40.0])[0];
        assert_eq!(rect.top - 100.0, 100.0 + ROW_HEIGHT - rect.bottom);
        assert_eq!(rect.bottom - rect.top, BUTTON_HEIGHT);
    }

    #[test]
    fn hit_testing_finds_the_button_under_the_pointer() {
        let rects = button_rects(0.0, 300.0, &[40.0, 40.0]);
        let hits = [("first", rects[0]), ("second", rects[1])];
        let inside_second = (rects[1].left + 5.0, ROW_HEIGHT / 2.0);
        assert_eq!(hit_test(&hits, inside_second), Some("second"));
        assert_eq!(hit_test(&hits, (0.0, 0.0)), None);
    }

    #[test]
    fn a_rectangle_includes_its_top_left_and_excludes_its_bottom_right() {
        let rect = Rect {
            left: 1.0,
            top: 2.0,
            right: 3.0,
            bottom: 4.0,
        };
        assert!(rect.contains((1.0, 2.0)));
        assert!(!rect.contains((3.0, 4.0)));
    }

    #[test]
    fn pixels_become_dips() {
        assert_eq!(pixels_to_dips((144, 72), (144.0, 144.0)), (96.0, 48.0));
    }
}
