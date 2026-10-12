use super::inventory::Entry;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, prelude::*, widgets::*};
use std::io::{self, IsTerminal};

pub fn choose(entries: &[&Entry], action: &str) -> io::Result<Option<Vec<usize>>> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(io::Error::other(format!(
            "global T3 {action} requires an interactive terminal; supply an environment identifier, --repository, or --all to explicitly select all working environments"
        )));
    }
    enable_raw_mode()?;
    let _restore = RestoreTerminal;
    execute!(io::stderr(), EnterAlternateScreen, Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;
    let mut selection = Selection::new(entries.len());
    loop {
        terminal.draw(|frame| selection.render(frame, entries, action))?;
        if let Event::Key(key) = event::read()?
            && let Some(result) = selection.handle(key)
        {
            return Ok(result);
        }
    }
}

struct RestoreTerminal;
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = execute!(io::stderr(), LeaveAlternateScreen, Show);
        let _ = disable_raw_mode();
    }
}

struct Selection {
    checked: Vec<bool>,
    list: ListState,
}

impl Selection {
    fn new(count: usize) -> Self {
        Self {
            checked: vec![false; count],
            list: ListState::default().with_selected(Some(0)),
        }
    }

    fn handle(&mut self, key: KeyEvent) -> Option<Option<Vec<usize>>> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return Some(None);
        }
        let index = self.list.selected().unwrap_or(0);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.list.select(Some(index.saturating_sub(1))),
            KeyCode::Down | KeyCode::Char('j') => self
                .list
                .select(Some((index + 1).min(self.checked.len().saturating_sub(1)))),
            KeyCode::Home => self.list.select(Some(0)),
            KeyCode::End => self.list.select(Some(self.checked.len().saturating_sub(1))),
            KeyCode::Char(' ') => {
                if let Some(checked) = self.checked.get_mut(index) {
                    *checked = !*checked;
                }
            }
            KeyCode::Enter => {
                return Some(Some(
                    self.checked
                        .iter()
                        .enumerate()
                        .filter_map(|(index, checked)| checked.then_some(index))
                        .collect(),
                ));
            }
            _ => {}
        }
        None
    }

    fn render(&mut self, frame: &mut Frame, entries: &[&Entry], action: &str) {
        let [notice, list, controls] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(frame.area());
        frame.render_widget(Paragraph::new(format!(
            "Choose T3 environments to {action}.\nSelected environments will be interrupted; mounted data is preserved."
        )).wrap(Wrap { trim: false }), notice);
        let items = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let checkbox = if self.checked[index] { "[x]" } else { "[ ]" };
                ListItem::new(vec![
                    Line::from(format!(
                        "{checkbox} {} (SSH {}) | T3 {}",
                        text(&entry.status["environment"]),
                        text(&entry.status["ssh_alias"]),
                        text(&entry.status["runtime"])
                    )),
                    Line::from(format!("    {}", text(&entry.status["repository"]))),
                ])
            })
            .collect::<Vec<_>>();
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title("Working environments"))
                .highlight_style(Style::default().fg(Color::Cyan))
                .highlight_symbol("> "),
            list,
            &mut self.list,
        );
        frame.render_widget(Paragraph::new(format!(
            "Up/Down: navigate | Space: toggle | Enter: {action} selected\nEsc/Ctrl-C: cancel | {} selected",
            self.checked.iter().filter(|checked| **checked).count()
        )), controls);
    }
}

fn text(value: &serde_json::Value) -> String {
    value
        .as_str()
        .unwrap_or("unknown")
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
