use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragAnchor {
    pub start_col: u16,
    pub start_row: u16,
    pub start_offset_x: i16,
    pub start_offset_y: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalGeometry {
    width_pct: u16,
    height_pct: u16,
    default_width_pct: u16,
    default_height_pct: u16,
    min_pct: u16,
    max_pct: u16,
    step_pct: u16,
    offset_x: i16,
    offset_y: i16,
    drag_anchor: Option<DragAnchor>,
}

impl ModalGeometry {
    pub fn new(
        default_width_pct: u16,
        default_height_pct: u16,
        min_pct: u16,
        max_pct: u16,
        step_pct: u16,
    ) -> Self {
        Self {
            width_pct: default_width_pct,
            height_pct: default_height_pct,
            default_width_pct,
            default_height_pct,
            min_pct,
            max_pct,
            step_pct,
            offset_x: 0,
            offset_y: 0,
            drag_anchor: None,
        }
    }

    pub fn width_pct(&self) -> u16 {
        self.width_pct
    }

    pub fn height_pct(&self) -> u16 {
        self.height_pct
    }

    pub fn offset_x(&self) -> i16 {
        self.offset_x
    }

    pub fn offset_y(&self) -> i16 {
        self.offset_y
    }

    pub fn drag_anchor(&self) -> Option<DragAnchor> {
        self.drag_anchor
    }

    pub fn set_offset(&mut self, x: i16, y: i16) {
        self.offset_x = x;
        self.offset_y = y;
    }

    pub fn begin_drag(&mut self, anchor: DragAnchor) {
        self.drag_anchor = Some(anchor);
    }

    pub fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

    pub fn shrink(&mut self) {
        self.width_pct = self
            .width_pct
            .saturating_sub(self.step_pct)
            .max(self.min_pct);
        self.height_pct = self
            .height_pct
            .saturating_sub(self.step_pct)
            .max(self.min_pct);
    }

    pub fn grow(&mut self) {
        self.width_pct = self
            .width_pct
            .saturating_add(self.step_pct)
            .min(self.max_pct);
        self.height_pct = self
            .height_pct
            .saturating_add(self.step_pct)
            .min(self.max_pct);
    }

    pub fn reset(&mut self) {
        self.width_pct = self.default_width_pct;
        self.height_pct = self.default_height_pct;
        self.offset_x = 0;
        self.offset_y = 0;
        self.drag_anchor = None;
    }

    pub fn area(&self, viewport: Rect) -> Rect {
        modal_area(
            viewport,
            self.width_pct,
            self.height_pct,
            self.offset_x,
            self.offset_y,
        )
    }
}

pub fn modal_area(
    viewport: Rect,
    width_pct: u16,
    height_pct: u16,
    offset_x: i16,
    offset_y: i16,
) -> Rect {
    let base = crate::ui::dashboard::centered_rect(width_pct, height_pct, viewport);
    apply_clamped_offset(base, viewport, offset_x, offset_y)
}

pub fn apply_clamped_offset(base: Rect, viewport: Rect, offset_x: i16, offset_y: i16) -> Rect {
    let min_x = viewport.x as i32;
    let min_y = viewport.y as i32;
    let max_x = viewport.x.saturating_add(viewport.width).saturating_sub(4) as i32;
    let max_y = viewport.y.saturating_add(viewport.height).saturating_sub(1) as i32;
    let x = (base.x as i32 + offset_x as i32).clamp(min_x, max_x.max(min_x));
    let y = (base.y as i32 + offset_y as i32).clamp(min_y, max_y.max(min_y));
    Rect {
        x: x as u16,
        y: y as u16,
        ..base
    }
}

pub fn title_row_contains(area: Rect, col: u16, row: u16) -> bool {
    row == area.y && col >= area.x && col < area.x.saturating_add(area.width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_area_with_zero_offset_matches_centered_rect() {
        let viewport = Rect::new(0, 0, 100, 40);
        let geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        let area = geometry.area(viewport);
        assert_eq!(area, crate::ui::dashboard::centered_rect(80, 70, viewport));
    }

    #[test]
    fn modal_area_applies_positive_offset() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        let base = geometry.area(viewport);
        geometry.set_offset(5, 3);
        let moved = geometry.area(viewport);
        assert_eq!(moved.x, base.x + 5);
        assert_eq!(moved.y, base.y + 3);
    }

    #[test]
    fn modal_area_left_top_clamps_hard() {
        let viewport = Rect::new(10, 5, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.set_offset(i16::MIN, i16::MIN);
        let area = geometry.area(viewport);
        assert_eq!(area.x, viewport.x);
        assert_eq!(area.y, viewport.y);
    }

    #[test]
    fn modal_area_right_bottom_leaves_visible_grip() {
        let viewport = Rect::new(0, 0, 100, 40);
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.set_offset(i16::MAX, i16::MAX);
        let area = geometry.area(viewport);
        assert!(area.x < viewport.x + viewport.width);
        assert!(area.y < viewport.y + viewport.height);
        assert!(viewport.x + viewport.width - area.x >= 4);
        assert!(viewport.y + viewport.height - area.y >= 1);
    }

    #[test]
    fn shrink_grow_and_reset_geometry() {
        let mut geometry = ModalGeometry::new(80, 70, 50, 99, 5);
        geometry.grow();
        assert_eq!(geometry.width_pct(), 85);
        assert_eq!(geometry.height_pct(), 75);
        geometry.shrink();
        geometry.shrink();
        assert_eq!(geometry.width_pct(), 75);
        assert_eq!(geometry.height_pct(), 65);
        geometry.set_offset(4, 2);
        geometry.reset();
        assert_eq!(geometry.width_pct(), 80);
        assert_eq!(geometry.height_pct(), 70);
        assert_eq!(geometry.offset_x(), 0);
        assert_eq!(geometry.offset_y(), 0);
    }
}
