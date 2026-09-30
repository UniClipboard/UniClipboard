use global_hotkey::{hotkey::HotKey, GlobalHotKeyManager};
use gpui::Global;
use std::time::{Duration, Instant};
use uc_daemon_contract::api::dto::settings::SettingsDto;

pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
    sequences: Vec<Vec<u32>>,
    pending: Option<(u32, Instant)>,
    definitions: Vec<String>,
}
impl Global for Shortcuts {}

impl Shortcuts {
    pub fn new() -> anyhow::Result<Self> {
        let mut host = Self {
            manager: GlobalHotKeyManager::new()?,
            registered: vec![],
            sequences: vec![],
            pending: None,
            definitions: vec![],
        };
        host.replace(vec![std::env::var("UC_GPUI_SHORTCUT").unwrap_or_else(
            |_| uc_desktop::shortcuts::DEFAULT_QUICK_PANEL_SHORTCUT.into(),
        )])?;
        Ok(host)
    }

    pub fn configure(&mut self, settings: &SettingsDto) -> anyhow::Result<()> {
        if let Ok(value) = std::env::var("UC_GPUI_SHORTCUT") {
            return self.replace(vec![value]);
        }
        // Whether the panel is enabled is decided by the GUI, which starts and stops this
        // process. While it runs, the configured shortcut (or the platform default) stays
        // registered, so disabling can never leave it without a way to reopen.
        let bindings =
            uc_desktop::shortcuts::resolve_quick_panel_shortcuts(&settings.keyboard_shortcuts);
        self.replace(bindings)
    }

    fn replace(&mut self, bindings: Vec<String>) -> anyhow::Result<()> {
        if self.definitions == bindings {
            return Ok(());
        }
        let mut keys = vec![];
        let mut sequences = vec![];
        for binding in &bindings {
            let mut sequence = vec![];
            for segment in binding.split_whitespace() {
                let key: HotKey = segment.parse()?;
                sequence.push(key.id());
                if !keys.iter().any(|k: &HotKey| k.id() == key.id()) {
                    keys.push(key);
                }
            }
            anyhow::ensure!(
                !sequence.is_empty() && sequence.len() <= 2,
                "Shortcut must have one or two strokes"
            );
            sequences.push(sequence);
        }
        let additions = keys
            .iter()
            .filter(|key| !self.registered.iter().any(|old| old.id() == key.id()))
            .copied()
            .collect::<Vec<_>>();
        let mut applied = vec![];
        for key in additions {
            if let Err(error) = self.manager.register(key) {
                for added in applied {
                    if self.manager.unregister(added).is_err() {
                        tracing::warn!("Failed to roll back shortcut registration");
                    }
                }
                return Err(error.into());
            }
            applied.push(key);
        }
        for old in &self.registered {
            if !keys.iter().any(|key| key.id() == old.id()) {
                self.manager.unregister(*old)?;
            }
        }
        self.registered = keys;
        self.sequences = sequences;
        self.definitions = bindings;
        self.pending = None;
        Ok(())
    }

    pub fn pressed(&mut self, id: u32) -> bool {
        if let Some((expected, time)) = self.pending.take() {
            if expected == id && time.elapsed() < Duration::from_secs(1) {
                return true;
            }
        }
        for sequence in &self.sequences {
            if sequence[0] == id {
                if sequence.len() == 1 {
                    return true;
                }
                self.pending = Some((sequence[1], Instant::now()));
            }
        }
        false
    }
}
