//! Phase 7 v3 (v1.46.0): keyboard handler for the `n` (anomaly
//! events) overlay. Mirrors the `m` overlay handler's contract
//! (returns `true` when the key is consumed).

use crate::ui::anomaly_overlay::AnomalyOverlay;
use crossterm::event::KeyCode;

pub fn handle_anomaly_overlay_key(
    overlay: &mut AnomalyOverlay,
    ring_len: usize,
    key: KeyCode,
) -> bool {
    match key {
        KeyCode::Char('n') => {
            overlay.toggle();
            true
        }
        KeyCode::Esc | KeyCode::Char('q') if overlay.is_open() => {
            overlay.close();
            true
        }
        KeyCode::Down | KeyCode::Char('j') if overlay.is_open() => {
            overlay.scroll_down(ring_len.saturating_sub(1) as u16);
            true
        }
        KeyCode::Up | KeyCode::Char('k') if overlay.is_open() => {
            overlay.scroll_up();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_toggles_open_close() {
        let mut o = AnomalyOverlay::new();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(o.is_open());
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Char('n')));
        assert!(!o.is_open());
    }

    #[test]
    fn esc_closes_when_open_no_op_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 0, KeyCode::Esc));
        assert!(!o.is_open());
    }

    #[test]
    fn q_closes_when_open() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
        assert!(!o.is_open());
    }

    #[test]
    fn down_and_j_scroll_within_bounds() {
        let mut o = AnomalyOverlay::new();
        o.open();
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 1);
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Char('j')));
        assert_eq!(o.scroll(), 2);
        // ring_len=3 means max scroll index = 2
        assert!(handle_anomaly_overlay_key(&mut o, 3, KeyCode::Down));
        assert_eq!(o.scroll(), 2);
    }

    #[test]
    fn up_and_k_scroll_saturate_at_zero() {
        let mut o = AnomalyOverlay::new();
        o.open();
        o.scroll_down(5);
        o.scroll_down(5);
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Up));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert!(handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('k')));
        assert_eq!(o.scroll(), 0);
    }

    #[test]
    fn keys_ignored_when_closed() {
        let mut o = AnomalyOverlay::new();
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Down));
        assert!(!handle_anomaly_overlay_key(&mut o, 5, KeyCode::Char('q')));
    }
}
