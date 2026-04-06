use super::*;
use crate::component::Text;
use crate::utils::visible_width;

#[test]
fn test_composite_line_at_basic() {
    let base = "Hello World Goodbye!";
    let overlay = "XXXXX";
    let result = TUI::composite_line_at(base, overlay, 6, 5, 20);
    // before="Hello " overlay="XXXXX" after="Goodbye!"
    assert!(result.contains("Hello "));
    assert!(result.contains("XXXXX"));
    assert!(result.contains("Goodbye!"));
}

#[test]
fn test_composite_line_at_empty_base() {
    let result = TUI::composite_line_at("", "OK", 5, 4, 20);
    // Should pad before to 5, place "OK" padded to 4, then pad rest
    let vw = visible_width(&result);
    assert!(vw <= 20, "result width {vw} > 20");
}

#[test]
fn test_resolve_layout_center() {
    let opts = OverlayOptions {
        anchor: OverlayAnchor::Center,
        ..Default::default()
    };
    let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
    // Centred: row ≈ (24 - 5) / 2 = 9, col ≈ (80 - 80) / 2 = 0
    assert!(layout.row > 0);
    assert!(layout.width > 0);
}

#[test]
fn test_resolve_layout_top_left() {
    let opts = OverlayOptions {
        anchor: OverlayAnchor::TopLeft,
        width: Some(SizeValue::Absolute(40)),
        ..Default::default()
    };
    let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
    assert_eq!(layout.row, 0);
    assert_eq!(layout.col, 0);
    assert_eq!(layout.width, 40);
}

#[test]
fn test_resolve_layout_percentage() {
    let opts = OverlayOptions {
        width: Some(SizeValue::Percent(50.0)),
        row: Some(SizeValue::Percent(25.0)),
        col: Some(SizeValue::Percent(50.0)),
        anchor: OverlayAnchor::Center,
        ..Default::default()
    };
    let layout = TUI::resolve_overlay_layout(&opts, 5, 80, 24);
    assert_eq!(layout.width, 40); // 50% of 80
}

#[test]
fn test_set_focus_calls_set_focused() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FocusTracker {
        focused: Arc<AtomicBool>,
    }
    impl Component for FocusTracker {
        fn render(&self, _w: u16) -> Vec<String> {
            vec![]
        }
        fn set_focused(&mut self, f: bool) {
            self.focused.store(f, Ordering::SeqCst);
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    let flag_a = Arc::new(AtomicBool::new(false));
    let flag_b = Arc::new(AtomicBool::new(false));
    let mut tui = TUI::new();
    tui.root.add(Box::new(FocusTracker {
        focused: flag_a.clone(),
    }));
    tui.root.add(Box::new(FocusTracker {
        focused: flag_b.clone(),
    }));

    tui.set_focus(Some(0));
    assert!(flag_a.load(Ordering::SeqCst));
    assert!(!flag_b.load(Ordering::SeqCst));

    tui.set_focus(Some(1));
    assert!(!flag_a.load(Ordering::SeqCst));
    assert!(flag_b.load(Ordering::SeqCst));

    tui.set_focus(None);
    assert!(!flag_b.load(Ordering::SeqCst));
}

#[test]
fn test_non_capturing_overlay_keeps_focus() {
    let mut tui = TUI::new();
    tui.root.add(Box::new(Text::new("main")));
    tui.set_focus(Some(0));

    let _id = tui.show_overlay_with(
        Box::new(Text::new("popup")),
        OverlayOptions {
            non_capturing: true,
            ..Default::default()
        },
    );

    // Focus should stay on root child, not switch to overlay.
    assert_eq!(tui.focus_index(), Some(0));
}

#[test]
fn test_capturing_overlay_steals_focus() {
    let mut tui = TUI::new();
    tui.root.add(Box::new(Text::new("main")));
    tui.set_focus(Some(0));

    tui.show_overlay(Box::new(Text::new("modal")));

    // Focus should be None (overlay has focus).
    assert_eq!(tui.focus_index(), None);

    tui.hide_overlay();
    // Restored.
    assert_eq!(tui.focus_index(), Some(0));
}
