impl App {
    /// Get cached section content. Call `ensure_cache()` before rendering.
    fn section_content(&self, section_idx: usize) -> &SectionContent {
        self.content_cache[section_idx]
            .as_ref()
            .expect("cache must be populated before rendering")
    }

    /// Build section content for display from the provided document.
    fn compute_section_content(
        &self,
        section_idx: usize,
        doc: &toml_edit::DocumentMut,
        is_merged: bool,
    ) -> SectionContent {
        let info = &SECTIONS[section_idx];

        if info.is_array {
            self.extract_array_content(doc, info, is_merged)
        } else {
            self.extract_scalar_content(doc, info, is_merged)
        }
    }

    fn extract_scalar_content(
        &self,
        doc: &toml_edit::DocumentMut,
        info: &SectionInfo,
        is_merged: bool,
    ) -> SectionContent {
        let mut fields = Vec::new();
        let table = doc.get(info.toml_key).and_then(|i| i.as_table_like());

        for schema in scalar_fields(info.toml_key) {
            let item = table.and_then(|table| table.get(schema.key));
            let present = item.is_some_and(|i| !i.is_none());
            let value = item
                .map(format_toml_value)
                .unwrap_or_else(|| missing_scalar_value(schema));
            let source = if is_merged && present {
                Some(self.state.value_source(info.toml_key, schema.key))
            } else {
                None
            };
            fields.push(FieldDisplay {
                key: schema.key.to_string(),
                value,
                source,
                present,
                editable: true,
            });
        }

        if let Some(table) = table {
            for (key, item) in table.iter() {
                if scalar_field(info.toml_key, key).is_some() {
                    continue;
                }
                let source = if is_merged {
                    Some(self.state.value_source(info.toml_key, key))
                } else {
                    None
                };
                fields.push(FieldDisplay {
                    key: key.to_string(),
                    value: format!("[unknown] {}", format_toml_value(item)),
                    source,
                    present: true,
                    editable: false,
                });
            }
        }

        let max_key = fields.iter().map(|f| f.key.len()).max().unwrap_or(0);
        SectionContent::Scalar(fields, max_key)
    }

    fn extract_array_content(
        &self,
        doc: &toml_edit::DocumentMut,
        info: &SectionInfo,
        is_merged: bool,
    ) -> SectionContent {
        let mut entries = Vec::new();

        if let Some(aot) = doc.get(info.toml_key).and_then(|i| i.as_array_of_tables()) {
            for (idx, table) in aot.iter().enumerate() {
                let summary = summarize_array_entry(info.toml_key, table);
                let source = if is_merged {
                    Some(self.state.array_entry_source(info.toml_key, idx))
                } else {
                    None
                };
                entries.push(ArrayEntry {
                    summary,
                    source,
                    raw_index: idx,
                });
            }
        }

        SectionContent::Array(entries)
    }

    /// Returns `true` (and sets a warning) if we're in merged view and edits
    /// should be blocked.
    fn reject_if_merged(&mut self) -> bool {
        if self.state.view_mode == ViewMode::Merged {
            self.status_message = Some((
                "Press Enter to jump to the originating Raw value, or Ctrl-V to switch views."
                    .into(),
                StatusKind::Warning,
            ));
            true
        } else {
            false
        }
    }

    // -------------------------------------------------------------------
    // Agent helpers
    // -------------------------------------------------------------------

    fn agent_mount_present(
        aot: &toml_edit::ArrayOfTables,
        mount: &super::agents::AgentMountDef,
    ) -> bool {
        aot.iter().any(|table| {
            table.get("host").and_then(|v| v.as_str()) == Some(mount.host)
                && table.get("container").and_then(|v| v.as_str()) == Some(mount.container)
        })
    }

    fn agent_is_enabled(&self, agent: &AgentDef) -> bool {
        let doc = self.state.active_doc();
        let aot = match doc.get("agent_mount").and_then(|i| i.as_array_of_tables()) {
            Some(aot) => aot,
            None => return false,
        };

        agent
            .mounts
            .iter()
            .all(|mount| Self::agent_mount_present(aot, mount))
    }

    fn agent_host_status(&self, agent_idx: usize) -> &HostStatus {
        &self.agent_host_status_cache[agent_idx]
    }

    fn toggle_agent(&mut self, agent_idx: usize) {
        if self.reject_if_merged() {
            return;
        }

        if agent_idx >= KNOWN_AGENTS.len() {
            return;
        }

        let agent = &KNOWN_AGENTS[agent_idx];

        // Check current state (immutable borrow scoped in block)
        let enabled = self.agent_is_enabled(agent);

        let doc = self.state.active_doc_mut();
        let mut changed = false;

        if enabled {
            // Remove matching mounts (reverse order to preserve indices)
            if let Some(aot) = doc["agent_mount"].as_array_of_tables_mut() {
                let mut to_remove = Vec::new();
                for (i, table) in aot.iter().enumerate() {
                    for m in agent.mounts {
                        if table.get("host").and_then(|v| v.as_str()) == Some(m.host)
                            && table.get("container").and_then(|v| v.as_str()) == Some(m.container)
                        {
                            to_remove.push(i);
                        }
                    }
                }
                to_remove.sort_unstable();
                to_remove.dedup();
                changed = !to_remove.is_empty();
                for i in to_remove.into_iter().rev() {
                    aot.remove(i);
                }
            }
        } else {
            // Add only mounts that are still missing for this agent.
            if doc.get("agent_mount").is_none() || doc["agent_mount"].as_array_of_tables().is_none()
            {
                doc["agent_mount"] =
                    toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
            }
            if let Some(aot) = doc["agent_mount"].as_array_of_tables_mut() {
                for m in agent.mounts {
                    if Self::agent_mount_present(aot, m) {
                        continue;
                    }
                    let mut entry = toml_edit::Table::new();
                    entry["host"] = toml_edit::value(m.host);
                    entry["container"] = toml_edit::value(m.container);
                    entry["kind"] = toml_edit::value(m.kind.to_string());
                    aot.push(entry);
                    changed = true;
                }
            }
        }

        if changed {
            self.state.modified = true;
            self.invalidate_cache();
        }
    }
}
