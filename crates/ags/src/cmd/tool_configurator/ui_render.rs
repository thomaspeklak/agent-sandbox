impl App {
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let outer = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_top_bar(frame, outer[0]);
        self.render_package_screen(frame, outer[1]);
        if self.show_help {
            self.render_help_overlay(frame, outer[1]);
        }
        self.render_bottom_bar(frame, outer[2]);
    }

    fn render_top_bar(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .title(" AGS Tool Configurator ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let selected = self.state.selected_tool_count();
        let mut lines = vec![Line::from(vec![
            Span::raw(" Config: "),
            Span::styled(
                self.config_path.display().to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("    Packages: "),
            Span::styled(
                self.packages_path.display().to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("    Selected tools: "),
            Span::styled(selected.to_string(), Style::default().fg(Color::Green)),
        ])];

        if !self.state.packages.is_empty() {
            lines.push(self.package_tabs_line());
        }

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn package_tabs_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        for (index, package) in self.state.packages.iter().enumerate() {
            let text = format!(
                " {} ({}/{}) ",
                package.package,
                package.selected_count(),
                package.available_count()
            );
            let style = if index == self.current_package {
                Style::default().fg(Color::Black).bg(Color::Cyan).bold()
            } else if package.available_count() == 0 {
                Style::default().fg(Color::DarkGray)
            } else if package.all_available_selected() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            spans.push(Span::styled(text, style));
        }
        Line::from(spans)
    }

    fn render_package_screen(&self, frame: &mut Frame, area: Rect) {
        let Some(package) = self.current_package() else {
            let empty = Paragraph::new("No tool packages loaded.")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
            return;
        };

        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(8),
        ])
        .split(area);

        self.render_package_header(frame, chunks[0], package);
        self.render_tool_table(frame, chunks[1], package);
        self.render_tool_details(frame, chunks[2]);
    }

    fn render_package_header(&self, frame: &mut Frame, area: Rect, package: &PackageState) {
        let selected = package.selected_count();
        let available = package.available_count();
        let missing = package.missing_count();
        let status = if available == 0 {
            Span::styled("disabled", Style::default().fg(Color::DarkGray))
        } else if selected == available {
            Span::styled("selected", Style::default().fg(Color::Green))
        } else if selected == 0 {
            Span::styled("deselected", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("partial", Style::default().fg(Color::Yellow))
        };

        let text = vec![
            Line::from(vec![
                Span::raw(" Package: "),
                Span::styled(
                    package.package.clone(),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::raw("    State: "),
                status,
            ]),
            Line::from(vec![
                Span::raw(" Selected available tools: "),
                Span::styled(
                    format!("{selected}/{available}"),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("    Missing on PATH: "),
                Span::styled(missing.to_string(), Style::default().fg(Color::Yellow)),
            ]),
        ];

        let block = Block::bordered()
            .title(" Package ")
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        frame.render_widget(Paragraph::new(text).block(block), area);
    }

    fn render_tool_table(&self, frame: &mut Frame, area: Rect, package: &PackageState) {
        let header = Row::new(vec!["", "Tool", "Host", "Secrets", "Description"])
            .style(Style::default().fg(Color::DarkGray).bold());

        let rows = package.tools.iter().enumerate().map(|(index, tool)| {
            let selected_style = if index == self.selected_tool {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            let checkbox = if !tool.available() {
                "[-]"
            } else if tool.selected {
                "[x]"
            } else {
                "[ ]"
            };
            let checkbox_style = if !tool.available() {
                Style::default().fg(Color::DarkGray)
            } else if tool.selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let host = tool
                .host_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "missing on PATH".to_owned());
            let host_style = if tool.available() {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            Row::new(vec![
                Cell::from(checkbox).style(checkbox_style),
                Cell::from(tool.definition.name.clone()),
                Cell::from(host).style(host_style),
                Cell::from(tool.definition.secrets.len().to_string()),
                Cell::from(tool.definition.description.clone()),
            ])
            .style(selected_style)
        });

        let widths = [
            Constraint::Length(4),
            Constraint::Length(18),
            Constraint::Length(34),
            Constraint::Length(8),
            Constraint::Min(20),
        ];
        let table = Table::new(rows, widths).header(header).block(
            Block::bordered()
                .title(" Tools ")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(table, area);
    }

    fn render_tool_details(&self, frame: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        if let Some(tool) = self.current_tool() {
            lines.push(Line::from(vec![
                Span::styled(
                    tool.definition.name.clone(),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::raw(if tool.available() {
                    " available"
                } else {
                    " unavailable"
                }),
            ]));
            if !tool.definition.description.trim().is_empty() {
                lines.push(Line::from(tool.definition.description.clone()));
            }
            if let Some(path) = &tool.host_path {
                lines.push(Line::from(vec![
                    Span::raw("Host binary: "),
                    Span::styled(path.display().to_string(), Style::default().fg(Color::Green)),
                ]));
            }

            if tool.definition.secrets.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No declared secrets.",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "Declared secrets (names and sources only; no values are read):",
                    Style::default().fg(Color::Yellow),
                )));
                for (env, input) in tool.definition.secrets.iter().take(3) {
                    lines.push(Line::from(format!(
                        "  {} -> {}",
                        env,
                        secret_supply_summary(env, input)
                    )));
                }
                if tool.definition.secrets.len() > 3 {
                    lines.push(Line::from(format!(
                        "  ...and {} more",
                        tool.definition.secrets.len() - 3
                    )));
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No tools in this package.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let block = Block::bordered()
            .title(" Details ")
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(72, 70, area);
        frame.render_widget(Clear, popup);
        let block = Block::bordered()
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));
        let text = vec![
            Line::from(
                "Configure tool package selections. This does not install or configure host tools.",
            ),
            Line::from(""),
            Line::from("Left/Right or h/l  Change package screen"),
            Line::from("Up/Down or j/k     Move through tools"),
            Line::from("Space              Toggle selected tool (available tools only)"),
            Line::from("p                  Toggle entire package"),
            Line::from("s                  Save selected tools to config and quit"),
            Line::from("q or Esc           Quit without saving"),
            Line::from("?                  Show/close this help"),
            Line::from(""),
            Line::from("Unavailable tools are detected from PATH and cannot be selected."),
        ];
        frame.render_widget(
            Paragraph::new(text).block(block).wrap(Wrap { trim: true }),
            popup,
        );
    }

    fn render_bottom_bar(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match &self.status_message {
            Some((message, kind)) => (message.clone(), status_style(*kind)),
            None => (
                "h/l package  j/k tool  Space toggle  p package  s save  q quit  ? help"
                    .to_owned(),
                Style::default().fg(Color::DarkGray),
            ),
        };
        frame.render_widget(Paragraph::new(text).style(style), area);
    }
}

fn secret_supply_summary(env: &str, input: &SecretInput) -> String {
    let spec = input.normalized(env);
    let mut parts = Vec::new();
    if let Some(from_env) = spec
        .from_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("from host env {from_env}"));
    }
    if let Some(store) = spec.secret_store.as_ref().filter(|store| !store.is_empty()) {
        let attrs = store
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("secret-tool attributes: {attrs}"));
    }
    if parts.is_empty() {
        parts.push(format!("from host env {env}"));
    }
    let required = if spec.required {
        "required"
    } else {
        "optional"
    };
    if spec.description.trim().is_empty() {
        format!("{} ({required})", parts.join(" or "))
    } else {
        format!("{} ({required}; {})", parts.join(" or "), spec.description)
    }
}

fn status_style(kind: StatusKind) -> Style {
    match kind {
        StatusKind::Info => Style::default().fg(Color::Cyan),
        StatusKind::Success => Style::default().fg(Color::Green),
        StatusKind::Warning => Style::default().fg(Color::Yellow),
        StatusKind::Error => Style::default().fg(Color::Red),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);

    horizontal[1]
}
