impl App {
    pub fn new(config_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir().ok();
        Self::new_with_cwd(config_path, cwd.as_deref())
    }

    fn new_with_cwd(
        config_path: &Path,
        cwd: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (local_path, repo_local_available) = super::resolve_local_target_path(cwd);

        let mut state = ConfigEditorState::load(config_path, &local_path)?;
        if !repo_local_available {
            state.local_doc = "".parse::<toml_edit::DocumentMut>()?;
            state.local_exists = false;
        }

        let agent_host_status_cache = KNOWN_AGENTS.iter().map(compute_host_status).collect();

        Ok(Self {
            running: true,
            state,
            focus: Focus::Sidebar,
            selected_section: 0,
            selected_field: 0,
            show_help: false,
            search_query: String::new(),
            status_message: None,
            dialog: DialogState::None,
            edit_mode: EditMode::None,
            content_cache: vec![None; SECTIONS.len()],
            agent_enabled_cache: vec![false; KNOWN_AGENTS.len()],
            repo_local_available,
            quit_after_validation_dialog: false,
            agent_host_status_cache,
            cached_binaries: None,
            cached_home_dirs: None,
            current_suggestion: None,
        })
    }

    pub fn set_info_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), StatusKind::Info));
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = ratatui::init();

        let result = self.event_loop(&mut terminal);

        ratatui::restore();
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while self.running {
            self.ensure_cache();
            self.normalize_selection_for_filters();
            self.current_suggestion = self.active_suggestion();
            terminal.draw(|frame| self.render(frame))?;

            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            }
        }
        Ok(())
    }

    /// Pre-populate any missing cache entries before rendering.
    fn ensure_cache(&mut self) {
        let needs_fill = self.content_cache.iter().any(|s| s.is_none());
        if needs_fill {
            // Clone the document once for all sections that need filling.
            let is_merged = self.state.view_mode == ViewMode::Merged;
            let doc = if is_merged {
                self.state.compute_merged_view()
            } else {
                self.state.active_doc().clone()
            };

            for i in 0..SECTIONS.len() {
                if self.content_cache[i].is_none() {
                    let content = self.compute_section_content(i, &doc, is_merged);
                    self.content_cache[i] = Some(content);
                }
            }

            // Refresh agent enabled-state
            for (i, agent) in KNOWN_AGENTS.iter().enumerate() {
                self.agent_enabled_cache[i] = self.agent_is_enabled(agent);
            }
        }
    }

    // -------------------------------------------------------------------
    // Section helpers
    // -------------------------------------------------------------------

    /// Style for an item at `index` in the main panel — highlighted if selected.
    fn item_style(&self, index: usize) -> Style {
        if self.focus == Focus::MainPanel && index == self.selected_field {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default()
        }
    }

    fn is_agents_section(&self) -> bool {
        SECTIONS[self.selected_section].toml_key == "agent_mount"
    }

    fn panel_border_color(&self, active: Focus) -> Color {
        if self.focus == active {
            Color::Cyan
        } else {
            Color::DarkGray
        }
    }

    fn current_section_info(&self) -> &SectionInfo {
        &SECTIONS[self.selected_section]
    }

    fn current_search_query(&self) -> &str {
        match &self.edit_mode {
            EditMode::Search { input } => input.value(),
            _ => self.search_query.as_str(),
        }
    }

    fn filtered_section_indices(&self) -> Vec<usize> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..SECTIONS.len()).collect();
        }

        SECTIONS
            .iter()
            .enumerate()
            .filter(|(_, section)| {
                section.label.to_ascii_lowercase().contains(&query)
                    || section.toml_key.to_ascii_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn filtered_agent_indices(&self) -> Vec<usize> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..KNOWN_AGENTS.len()).collect();
        }

        KNOWN_AGENTS
            .iter()
            .enumerate()
            .filter(|(_, agent)| {
                agent.name.to_ascii_lowercase().contains(&query)
                    || agent
                        .mounts
                        .iter()
                        .any(|mount| mount.host.to_ascii_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn filtered_section_content(&self, section_idx: usize) -> std::borrow::Cow<'_, SectionContent> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return std::borrow::Cow::Borrowed(self.section_content(section_idx));
        }

        let content = self.section_content(section_idx).clone();
        let filtered = match content {
            SectionContent::Scalar(fields, max_key) => SectionContent::Scalar(
                fields
                    .into_iter()
                    .filter(|field| {
                        field.key.to_ascii_lowercase().contains(&query)
                            || field.value.to_ascii_lowercase().contains(&query)
                    })
                    .collect(),
                max_key,
            ),
            SectionContent::Array(entries) => SectionContent::Array(
                entries
                    .into_iter()
                    .filter(|entry| entry.summary.to_ascii_lowercase().contains(&query))
                    .collect(),
            ),
        };
        std::borrow::Cow::Owned(filtered)
    }

    fn normalize_selection_for_filters(&mut self) {
        let visible_sections = self.filtered_section_indices();
        if let Some(first) = visible_sections.first().copied()
            && !visible_sections.contains(&self.selected_section)
        {
            self.selected_section = first;
            self.selected_field = 0;
        }

        let max = self.current_item_count();
        if max == 0 {
            self.selected_field = 0;
        } else if self.selected_field >= max {
            self.selected_field = max - 1;
        }
    }

    fn ensure_suggestion_caches(&mut self) {
        if self.cached_binaries.is_none() {
            self.cached_binaries = Some(discover_path_binaries());
        }
        if self.cached_home_dirs.is_none() {
            self.cached_home_dirs = Some(discover_home_dirs());
        }
    }

    fn suggestion_cache(&self) -> SuggestionCache<'_> {
        SuggestionCache {
            binaries: self.cached_binaries.as_deref().unwrap_or(&[]),
            home_dirs: self.cached_home_dirs.as_deref().unwrap_or(&[]),
        }
    }

    fn active_suggestion(&mut self) -> Option<String> {
        match &self.edit_mode {
            EditMode::EditingField { .. } | EditMode::AddingEntry { .. } => {}
            _ => return None,
        }
        self.ensure_suggestion_caches();
        let cache = self.suggestion_cache();
        match &self.edit_mode {
            EditMode::EditingField {
                input, field_key, ..
            } => suggestion_for_field(field_key, input.value(), &cache),
            EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } => fields
                .get(*active_field)
                .and_then(|field| match field.kind {
                    FieldKind::Text => suggestion_for_field(field.key, field.input.value(), &cache),
                    _ => None,
                }),
            _ => None,
        }
    }

    /// Accept the current top-ranked suggestion into the active input field.
    fn accept_suggestion(&mut self) {
        self.ensure_suggestion_caches();
        let binaries = self.cached_binaries.as_deref().unwrap_or(&[]);
        let home_dirs = self.cached_home_dirs.as_deref().unwrap_or(&[]);
        let cache = SuggestionCache {
            binaries,
            home_dirs,
        };
        match &mut self.edit_mode {
            EditMode::EditingField {
                input, field_key, ..
            } => {
                if let Some(suggestion) = suggestion_for_field(field_key, input.value(), &cache) {
                    *input = Input::new(suggestion);
                }
            }
            EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } => {
                let field = &mut fields[*active_field];
                if let Some(suggestion) =
                    suggestion_for_field(field.key, field.input.value(), &cache)
                {
                    field.input = Input::new(suggestion);
                }
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------
    // Data extraction from real config (with caching)
    // -------------------------------------------------------------------

    /// Invalidate all cached section content. Call after any edit, save, undo,
    /// target toggle, or view mode change.
    fn invalidate_cache(&mut self) {
        for slot in &mut self.content_cache {
            *slot = None;
        }
    }
}
