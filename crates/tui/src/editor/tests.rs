use super::{Editor, KillContinuation};
use crate::component::{CURSOR_MARKER, Component, Focusable};

#[test]
fn test_new_editor_empty() {
    let editor = Editor::new();
    assert_eq!(editor.get_text(), "");
}

#[test]
fn test_set_text() {
    let mut editor = Editor::new();
    editor.set_text("hello\nworld");
    assert_eq!(editor.get_text(), "hello\nworld");
}

#[test]
fn test_insert_char() {
    let mut editor = Editor::new();
    editor.insert_char('h');
    editor.insert_char('i');
    assert_eq!(editor.get_text(), "hi");
}

#[test]
fn test_backspace() {
    let mut editor = Editor::new();
    editor.set_text("hello");
    editor.backspace();
    assert_eq!(editor.get_text(), "hell");
}

#[test]
fn test_backspace_empty() {
    let mut editor = Editor::new();
    editor.backspace(); // should not panic
    assert_eq!(editor.get_text(), "");
}

#[test]
fn test_new_line() {
    let mut editor = Editor::new();
    editor.set_text("hello");
    // Move cursor to middle
    editor.state.cursor_col = 2;
    editor.new_line();
    assert_eq!(editor.get_text(), "he\nllo");
}

#[test]
fn test_submit() {
    let mut editor = Editor::new();
    editor.set_text("hello world");
    let result = editor.try_submit();
    assert_eq!(result, Some("hello world".to_string()));
    assert_eq!(editor.get_text(), "");
}

#[test]
fn test_submit_empty() {
    let mut editor = Editor::new();
    let result = editor.try_submit();
    assert_eq!(result, None);
}

#[test]
fn test_history() {
    let mut editor = Editor::new();
    editor.add_to_history("first");
    editor.add_to_history("second");
    editor.navigate_history(-1); // up -> most recent
    assert_eq!(editor.get_text(), "second");
    editor.navigate_history(-1); // up -> older
    assert_eq!(editor.get_text(), "first");
    editor.navigate_history(1); // down -> back to recent
    assert_eq!(editor.get_text(), "second");
}

#[test]
fn test_render_bordered() {
    let mut editor = Editor::new();
    editor.set_text("hello");
    <Editor as Focusable>::set_focused(&mut editor, true);
    let lines = editor.render(40);
    // Should have: top border, content line, bottom border
    assert!(
        lines.len() >= 3,
        "Expected at least 3 lines, got {}",
        lines.len()
    );
    // Top border should contain ─
    assert!(lines[0].contains("─"), "Top border missing");
    // Last line should contain ─
    assert!(lines.last().unwrap().contains("─"), "Bottom border missing");
}

#[test]
fn test_render_cursor_marker() {
    let mut editor = Editor::new();
    editor.set_text("hi");
    <Editor as Focusable>::set_focused(&mut editor, true);
    let lines = editor.render(40);
    let joined = lines.join("");
    assert!(
        joined.contains(CURSOR_MARKER),
        "Should contain cursor marker when focused"
    );
}

#[test]
fn test_render_no_cursor_when_unfocused() {
    let editor = Editor::new();
    let lines = editor.render(40);
    let joined = lines.join("");
    assert!(
        !joined.contains(CURSOR_MARKER),
        "Should not contain cursor marker when unfocused"
    );
}

#[test]
fn test_word_wrap_line() {
    let chunks = Editor::word_wrap_line("hello world foo", 10);
    assert!(
        chunks.len() >= 2,
        "Should wrap, got {} chunks",
        chunks.len()
    );
}

#[test]
fn test_slash_menu_shows_on_slash() {
    let mut editor = Editor::new();
    editor.insert_char('/');
    editor.update_slash_menu();
    assert!(editor.is_showing_slash_menu());
}

#[test]
fn test_slash_menu_hides_after_space() {
    let mut editor = Editor::new();
    editor.set_text("/model foo");
    assert!(!editor.is_showing_slash_menu());
}

#[test]
fn test_slash_menu_render_contains_commands() {
    let mut editor = Editor::new();
    editor.insert_char('/');
    editor.update_slash_menu();
    let lines = editor.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("/help") || joined.contains("/model"));
}

#[test]
fn test_delete_word_backward_accumulates_kill_ring_in_read_order() {
    let mut editor = Editor::new();
    editor.set_text("alpha beta");

    editor.delete_word_backward(KillContinuation::NewEntry);
    assert_eq!(editor.get_text(), "alpha ");
    assert_eq!(editor.kill_ring.peek(), Some("beta"));

    editor.delete_word_backward(KillContinuation::Continue);
    assert_eq!(editor.get_text(), "");
    assert_eq!(editor.kill_ring.peek(), Some("alpha beta"));
}

#[test]
fn test_new_kill_sequence_starts_a_fresh_kill_ring_entry() {
    let mut editor = Editor::new();
    editor.set_text("alpha");
    editor.kill_to_end(KillContinuation::NewEntry);

    editor.set_text("beta");
    editor.kill_to_end(KillContinuation::NewEntry);

    assert_eq!(editor.kill_ring.len(), 2);
    assert_eq!(editor.kill_ring.peek(), Some("beta"));
}
