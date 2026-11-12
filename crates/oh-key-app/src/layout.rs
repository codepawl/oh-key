//! Window geometry as plain numbers, so it can be tested without a window. The constants at
//! the top are oh-snap's and shared by the tool windows; the settings window's compact layout
//! is in the `settings` module.

pub const PADDING: f32 = 32.0;
pub const TITLE_HEIGHT: f32 = 72.0;
pub const COLUMN_GAP: f32 = 48.0;
pub const SECTION_HEIGHT: f32 = 44.0;
pub const SECTION_GAP: f32 = 18.0;
pub const ROW_HEIGHT: f32 = 42.0;
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

/// The settings window: a title bar, two mode buttons, two cards of rows and three tool buttons
/// stacked in one column.
pub mod settings {
    use super::Rect;

    pub const WIDTH: f32 = 480.0;
    pub const MARGIN: f32 = 18.0;
    pub const TITLE_BAR_HEIGHT: f32 = 40.0;
    /// The same gap between every block, so no gap is uneven.
    pub const BLOCK_GAP: f32 = 14.0;
    pub const MODE_BUTTON_HEIGHT: f32 = 58.0;
    pub const MODE_BUTTON_GAP: f32 = 8.0;
    pub const CARD_PADDING: f32 = 6.0;
    pub const ROW_HEIGHT: f32 = 44.0;
    /// Space between a card's edge and the label or control inside it.
    pub const ROW_INSET: f32 = 14.0;
    pub const ACTION_HEIGHT: f32 = 42.0;
    pub const ACTION_GAP: f32 = 8.0;
    pub const DROPDOWN_WIDTH: f32 = 172.0;
    pub const CONTROL_HEIGHT: f32 = 32.0;
    pub const CHIP_HEIGHT: f32 = 30.0;
    pub const CHIP_TEXT_PADDING: f32 = 12.0;
    pub const CHIP_GAP: f32 = 6.0;
    pub const SWITCH_WIDTH: f32 = 38.0;
    pub const SWITCH_HEIGHT: f32 = 22.0;
    pub const POPUP_PADDING: f32 = 4.0;
    pub const POPUP_ITEM_HEIGHT: f32 = 32.0;
    pub const POPUP_OFFSET: f32 = 4.0;

    /// Where the blocks that do not depend on the rows sit, plus the window's height.
    pub struct Frame {
        pub mode_buttons: [Rect; 2],
        pub cards: Vec<Rect>,
        pub actions: [Rect; 3],
        pub height: f32,
    }

    fn equal_columns<const COUNT: usize>(top: f32, height: f32, gap: f32) -> [Rect; COUNT] {
        let total_gap = gap * (COUNT - 1) as f32;
        let width = (WIDTH - 2.0 * MARGIN - total_gap) / COUNT as f32;
        std::array::from_fn(|index| {
            let left = MARGIN + index as f32 * (width + gap);
            Rect {
                left,
                top,
                right: left + width,
                bottom: top + height,
            }
        })
    }

    pub fn card_height(row_count: usize) -> f32 {
        2.0 * CARD_PADDING + row_count as f32 * ROW_HEIGHT
    }

    /// Lays out the blocks top to bottom; `card_row_counts` has one entry per card.
    pub fn frame(card_row_counts: &[usize]) -> Frame {
        let mut top = TITLE_BAR_HEIGHT;
        let mode_buttons = equal_columns(top, MODE_BUTTON_HEIGHT, MODE_BUTTON_GAP);
        top += MODE_BUTTON_HEIGHT + BLOCK_GAP;
        let mut cards = Vec::with_capacity(card_row_counts.len());
        for row_count in card_row_counts {
            let height = card_height(*row_count);
            cards.push(Rect {
                left: MARGIN,
                top,
                right: WIDTH - MARGIN,
                bottom: top + height,
            });
            top += height + BLOCK_GAP;
        }
        let actions = equal_columns(top, ACTION_HEIGHT, ACTION_GAP);
        Frame {
            mode_buttons,
            cards,
            actions,
            height: top + ACTION_HEIGHT + MARGIN,
        }
    }

    pub fn row_rect(card: Rect, row_index: usize) -> Rect {
        let top = card.top + CARD_PADDING + row_index as f32 * ROW_HEIGHT;
        Rect {
            left: card.left,
            top,
            right: card.right,
            bottom: top + ROW_HEIGHT,
        }
    }

    /// A box `width` x `height` against the row's right inset, centred vertically in it.
    fn right_aligned(row: Rect, width: f32, height: f32) -> Rect {
        let top = row.top + (ROW_HEIGHT - height) / 2.0;
        Rect {
            left: row.right - ROW_INSET - width,
            top,
            right: row.right - ROW_INSET,
            bottom: top + height,
        }
    }

    pub fn dropdown_rect(row: Rect) -> Rect {
        right_aligned(row, DROPDOWN_WIDTH, CONTROL_HEIGHT)
    }

    pub fn switch_rect(row: Rect) -> Rect {
        right_aligned(row, SWITCH_WIDTH, SWITCH_HEIGHT)
    }

    /// Chip rectangles in option order, the last one on the row's right inset.
    pub fn chip_rects(row: Rect, text_widths: &[f32]) -> Vec<Rect> {
        let top = row.top + (ROW_HEIGHT - CHIP_HEIGHT) / 2.0;
        let mut right = row.right - ROW_INSET;
        let mut rects = Vec::with_capacity(text_widths.len());
        for text_width in text_widths.iter().rev() {
            let left = right - text_width - 2.0 * CHIP_TEXT_PADDING;
            rects.push(Rect {
                left,
                top,
                right,
                bottom: top + CHIP_HEIGHT,
            });
            right = left - CHIP_GAP;
        }
        rects.reverse();
        rects
    }

    /// The popup list under `anchor`, or above it when there is no room below.
    pub fn popup_rect(anchor: Rect, item_count: usize, window_height: f32) -> Rect {
        let height = 2.0 * POPUP_PADDING + item_count as f32 * POPUP_ITEM_HEIGHT;
        let below = anchor.bottom + POPUP_OFFSET;
        let top = if below + height + POPUP_PADDING > window_height {
            anchor.top - POPUP_OFFSET - height
        } else {
            below
        };
        Rect {
            left: anchor.left,
            top,
            right: anchor.right,
            bottom: top + height,
        }
    }

    pub fn popup_item_rects(popup: Rect, item_count: usize) -> Vec<Rect> {
        (0..item_count)
            .map(|index| {
                let top = popup.top + POPUP_PADDING + index as f32 * POPUP_ITEM_HEIGHT;
                Rect {
                    left: popup.left + POPUP_PADDING,
                    top,
                    right: popup.right - POPUP_PADDING,
                    bottom: top + POPUP_ITEM_HEIGHT,
                }
            })
            .collect()
    }

    /// `rect` grown by `amount` on every side.
    pub fn grown(rect: Rect, amount: f32) -> Rect {
        Rect {
            left: rect.left - amount,
            top: rect.top - amount,
            right: rect.right + amount,
            bottom: rect.bottom + amount,
        }
    }

    /// The smallest rectangle holding both.
    pub fn union(first: Rect, second: Rect) -> Rect {
        Rect {
            left: first.left.min(second.left),
            top: first.top.min(second.top),
            right: first.right.max(second.right),
            bottom: first.bottom.max(second.bottom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::settings as compact;
    use super::*;

    #[test]
    fn the_blocks_are_stacked_with_one_gap_between_each() {
        let frame = compact::frame(&[3, 6]);
        let gap = compact::BLOCK_GAP;
        assert_eq!(frame.mode_buttons[0].top, compact::TITLE_BAR_HEIGHT);
        assert_eq!(frame.cards[0].top - frame.mode_buttons[0].bottom, gap);
        assert_eq!(frame.cards[1].top - frame.cards[0].bottom, gap);
        assert_eq!(frame.actions[0].top - frame.cards[1].bottom, gap);
        assert_eq!(frame.height, frame.actions[0].bottom + compact::MARGIN);
    }

    #[test]
    fn the_side_by_side_buttons_fill_the_width_with_equal_gaps() {
        let frame = compact::frame(&[1]);
        assert_eq!(frame.mode_buttons[0].left, compact::MARGIN);
        assert_eq!(
            frame.mode_buttons[1].right,
            compact::WIDTH - compact::MARGIN
        );
        assert_eq!(
            frame.mode_buttons[1].left - frame.mode_buttons[0].right,
            compact::MODE_BUTTON_GAP
        );
        let widths: Vec<f32> = frame
            .actions
            .iter()
            .map(|action| action.right - action.left)
            .collect();
        assert!((widths[0] - widths[2]).abs() < 0.001);
        assert_eq!(frame.actions[2].right, compact::WIDTH - compact::MARGIN);
    }

    #[test]
    fn a_card_is_its_rows_plus_padding_top_and_bottom() {
        let frame = compact::frame(&[3, 6]);
        assert_eq!(
            frame.cards[1].bottom - frame.cards[1].top,
            6.0 * 44.0 + 12.0
        );
        let last_row = compact::row_rect(frame.cards[0], 2);
        assert_eq!(
            frame.cards[0].bottom - last_row.bottom,
            compact::CARD_PADDING
        );
    }

    #[test]
    fn controls_are_centred_in_their_row_and_share_the_right_edge() {
        let row = compact::row_rect(compact::frame(&[3]).cards[0], 1);
        let controls = [
            compact::dropdown_rect(row),
            compact::switch_rect(row),
            compact::chip_rects(row, &[30.0, 30.0])[1],
        ];
        for control in controls {
            assert_eq!(control.right, row.right - compact::ROW_INSET);
            assert_eq!(control.top - row.top, row.bottom - control.bottom);
        }
    }

    #[test]
    fn chips_keep_option_order_with_a_gap() {
        let row = compact::row_rect(compact::frame(&[1]).cards[0], 0);
        let chips = compact::chip_rects(row, &[20.0, 40.0]);
        assert_eq!(chips[0].right + compact::CHIP_GAP, chips[1].left);
        assert_eq!(chips[1].right - chips[1].left, 40.0 + 24.0);
    }

    #[test]
    fn the_popup_opens_below_the_control_or_above_when_cramped() {
        let anchor = Rect {
            left: 100.0,
            top: 50.0,
            right: 270.0,
            bottom: 82.0,
        };
        let below = compact::popup_rect(anchor, 3, 600.0);
        assert_eq!(below.top, anchor.bottom + compact::POPUP_OFFSET);
        assert_eq!(below.left, anchor.left);
        let above = compact::popup_rect(anchor, 3, 120.0);
        assert_eq!(above.bottom, anchor.top - compact::POPUP_OFFSET);
    }

    #[test]
    fn popup_items_fit_inside_the_popup_without_gaps() {
        let popup = compact::popup_rect(
            Rect {
                left: 0.0,
                top: 0.0,
                right: 100.0,
                bottom: 32.0,
            },
            4,
            600.0,
        );
        let items = compact::popup_item_rects(popup, 4);
        assert_eq!(items[1].top, items[0].bottom);
        assert_eq!(items[3].bottom + compact::POPUP_PADDING, popup.bottom);
    }

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
