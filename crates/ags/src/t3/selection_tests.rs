use super::*;

#[test]
fn selections_start_empty_and_require_explicit_toggles() {
    let mut selection = Selection::new(3);
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    assert_eq!(selection.handle(key(KeyCode::Enter)), Some(Some(vec![])));
    selection.handle(key(KeyCode::Up));
    assert_eq!(selection.list.selected(), Some(0));
    selection.handle(key(KeyCode::Char(' ')));
    selection.handle(key(KeyCode::End));
    selection.handle(key(KeyCode::Char(' ')));
    selection.handle(key(KeyCode::Down));
    assert_eq!(selection.list.selected(), Some(2));
    assert_eq!(
        selection.handle(key(KeyCode::Enter)),
        Some(Some(vec![0, 2]))
    );
    selection.handle(key(KeyCode::Char(' ')));
    assert_eq!(selection.handle(key(KeyCode::Enter)), Some(Some(vec![0])));
}

#[test]
fn cancellation_does_not_return_checked_environments() {
    let mut selection = Selection::new(2);
    selection.handle(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    for key in [
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        assert_eq!(selection.handle(key), Some(None));
    }
    let mut release = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(selection.handle(release), None);
    assert_eq!(selection.checked, vec![true, false]);
}

#[test]
fn list_shows_unchecked_environment_details_and_scrolls_to_later_rows() {
    let entries: Vec<_> = (0..12)
        .map(|index| Entry {
            registration: None,
            status: serde_json::json!({"environment": format!("env-{index}"),
            "ssh_alias": format!("ssh-{index}"), "runtime": "0.0.45",
            "repository": format!("/repo-{index} with spaces")}),
        })
        .collect();
    let references = entries.iter().collect::<Vec<_>>();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 14)).unwrap();
    let mut selection = Selection::new(entries.len());
    terminal
        .draw(|frame| selection.render(frame, &references, "upgrade"))
        .unwrap();
    let screen = format!("{}", terminal.backend());
    assert!(screen.contains("[ ] env-0 (SSH ssh-0) | T3 0.0.45"));
    assert!(screen.contains("/repo-0 with spaces") && screen.contains("0 selected"));
    selection.handle(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    terminal
        .draw(|frame| selection.render(frame, &references, "upgrade"))
        .unwrap();
    assert!(format!("{}", terminal.backend()).contains("[ ] env-11"));
}
