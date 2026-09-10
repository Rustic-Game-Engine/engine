use engine_core::logging::{Language, Severity, SourceLocation};
use engine_play::{ConsoleEvent, ProcessRole};
use std::collections::BTreeSet;

/// UI-neutral console row retaining every filtering/click-through dimension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleEntry {
    pub timestamp_unix_millis: i64,
    pub severity: Severity,
    pub subsystem: String,
    pub language: Option<Language>,
    pub process: String,
    pub process_id: u32,
    pub message: String,
    pub source: Option<SourceLocation>,
    pub stack_frames: Vec<SourceLocation>,
    pub duplicate_count: u32,
}

impl ConsoleEntry {
    pub fn from_runtime(event: ConsoleEvent) -> Self {
        let process = match event.process.role {
            ProcessRole::Editor => "editor",
            ProcessRole::Runtime => "runtime",
        };
        Self {
            timestamp_unix_millis: event.record.timestamp_unix_millis,
            severity: event.record.severity,
            subsystem: event.record.metadata.subsystem.as_str().to_owned(),
            language: event.record.metadata.language,
            process: process.to_owned(),
            process_id: event.process.process_id,
            message: event.record.message,
            source: event.record.metadata.source,
            stack_frames: event.stack_frames,
            duplicate_count: 1,
        }
    }

    fn collapse_key_matches(&self, other: &Self) -> bool {
        self.severity == other.severity
            && self.subsystem == other.subsystem
            && self.language == other.language
            && self.process == other.process
            && self.process_id == other.process_id
            && self.message == other.message
            && self.source == other.source
            && self.stack_frames == other.stack_frames
    }
}

#[derive(Clone, Debug)]
pub struct ConsoleFilter {
    pub minimum_severity: Severity,
    pub query: String,
    pub subsystems: BTreeSet<String>,
    pub languages: BTreeSet<String>,
    pub processes: BTreeSet<String>,
}

impl Default for ConsoleFilter {
    fn default() -> Self {
        Self {
            minimum_severity: Severity::Verbose,
            query: String::new(),
            subsystems: BTreeSet::new(),
            languages: BTreeSet::new(),
            processes: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeConsole {
    entries: Vec<ConsoleEntry>,
    pub filter: ConsoleFilter,
}

impl RuntimeConsole {
    pub fn push(&mut self, entry: ConsoleEntry) {
        if let Some(last) = self.entries.last_mut()
            && last.collapse_key_matches(&entry)
        {
            last.duplicate_count = last.duplicate_count.saturating_add(1);
            last.timestamp_unix_millis = entry.timestamp_unix_millis;
            return;
        }
        self.entries.push(entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn entries(&self) -> &[ConsoleEntry] {
        &self.entries
    }

    pub fn visible(&self) -> impl Iterator<Item = &ConsoleEntry> {
        let query = self.filter.query.trim().to_lowercase();
        self.entries.iter().filter(move |entry| {
            entry.severity >= self.filter.minimum_severity
                && (self.filter.subsystems.is_empty()
                    || self.filter.subsystems.contains(&entry.subsystem))
                && (self.filter.languages.is_empty()
                    || entry
                        .language
                        .as_ref()
                        .is_some_and(|language| self.filter.languages.contains(language.as_str())))
                && (self.filter.processes.is_empty()
                    || self.filter.processes.contains(&entry.process))
                && (query.is_empty()
                    || entry.message.to_lowercase().contains(&query)
                    || entry.subsystem.to_lowercase().contains(&query)
                    || entry.process.to_lowercase().contains(&query))
        })
    }

    pub fn copy_text(&self) -> String {
        self.visible()
            .map(|entry| {
                let duplicates = if entry.duplicate_count > 1 {
                    format!(" x{}", entry.duplicate_count)
                } else {
                    String::new()
                };
                format!(
                    "{} [{}] [{}:{}] {}{}",
                    entry.timestamp_unix_millis,
                    entry.severity,
                    entry.process,
                    entry.subsystem,
                    entry.message,
                    duplicates
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(message: &str) -> ConsoleEntry {
        ConsoleEntry {
            timestamp_unix_millis: 1,
            severity: Severity::Warning,
            subsystem: "script".to_owned(),
            language: Some(Language::Lua),
            process: "runtime".to_owned(),
            process_id: 42,
            message: message.to_owned(),
            source: Some(SourceLocation::new("scripts/main.lua", 7)),
            stack_frames: Vec::new(),
            duplicate_count: 1,
        }
    }

    #[test]
    fn duplicate_collapse_filter_copy_and_clear_are_data_backed() {
        let mut console = RuntimeConsole::default();
        console.push(entry("boom"));
        console.push(entry("boom"));
        assert_eq!(console.entries()[0].duplicate_count, 2);
        console.filter.query = "boom".to_owned();
        console.filter.processes.insert("runtime".to_owned());
        assert_eq!(console.visible().count(), 1);
        assert!(console.copy_text().contains("x2"));
        console.clear();
        assert!(console.entries().is_empty());
    }
}
