//! Bounded, debounced watcher path (F-12; PROJEKTPLAN §12.3).
//!
//! Filesystem events never trigger work directly: they only mark a scope as
//! "dirty" in a set. The owner rescans a dirty scope after it has been quiet
//! for [`WATCH_DEBOUNCE`] and at most every [`WATCH_MIN_RESCAN_INTERVAL`].
//! A set cannot overflow, and lost detail events (buffer overflow) simply
//! mark the scope for reconciliation. Events are hints, never history.
use crate::limits::{WATCH_DEBOUNCE, WATCH_MIN_RESCAN_INTERVAL};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Default)]
struct State {
    dirty: HashMap<i64, Instant>,
    last_scan: HashMap<i64, Instant>,
}

#[derive(Default)]
pub struct WatchHub {
    watchers: Vec<RecommendedWatcher>,
    state: Arc<Mutex<State>>,
    watched: Vec<i64>,
}

impl std::fmt::Debug for WatchHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchHub").field("watched", &self.watched).finish()
    }
}

fn ignored(path: &Path) -> bool {
    path.components().any(|c| {
        let c = c.as_os_str().to_string_lossy();
        c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case("__pycache__")
    })
}

impl WatchHub {
    /// Replaces all watchers. `wake` is called (cheaply, possibly often) to
    /// nudge the owner; it must not block.
    pub fn configure<W>(&mut self, scopes: &[(i64, PathBuf)], wake: W) -> Vec<i64>
    where
        W: Fn() + Send + Sync + 'static,
    {
        self.watchers.clear();
        self.watched.clear();
        let wake = Arc::new(wake);
        for (scope_id, root) in scopes {
            let state = Arc::clone(&self.state);
            let wake = Arc::clone(&wake);
            let scope_id = *scope_id;
            let handler = move |event: notify::Result<notify::Event>| {
                let relevant = match &event {
                    Ok(event) => event.need_rescan() || event.paths.iter().any(|p| !ignored(p)),
                    Err(_) => true, // lost events: reconcile the whole scope
                };
                if relevant {
                    if let Ok(mut state) = state.lock() {
                        state.dirty.insert(scope_id, Instant::now());
                    }
                    wake();
                }
            };
            let Ok(mut watcher) = notify::recommended_watcher(handler) else { continue };
            if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
                self.watchers.push(watcher);
                self.watched.push(scope_id);
            }
        }
        self.watched.clone()
    }

    pub fn stop(&mut self) {
        self.watchers.clear();
        self.watched.clear();
    }

    pub fn watched(&self) -> &[i64] {
        &self.watched
    }

    pub fn has_dirty(&self) -> bool {
        self.state.lock().map(|s| !s.dirty.is_empty()).unwrap_or(false)
    }

    /// Scopes that are quiet long enough and not rescanned too recently.
    pub fn take_due(&self) -> Vec<i64> {
        let Ok(mut state) = self.state.lock() else { return Vec::new() };
        let now = Instant::now();
        let due: Vec<i64> = state
            .dirty
            .iter()
            .filter(|(id, last_event)| {
                now.duration_since(**last_event) >= WATCH_DEBOUNCE
                    && state.last_scan.get(id).is_none_or(|t| now.duration_since(*t) >= WATCH_MIN_RESCAN_INTERVAL)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &due {
            state.dirty.remove(id);
            state.last_scan.insert(*id, now);
        }
        due
    }

    #[cfg(test)]
    fn mark(&self, id: i64, at: Instant) {
        self.state.lock().unwrap().dirty.insert(id, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn debounce_and_rate_limit() {
        let hub = WatchHub::default();
        hub.mark(1, Instant::now());
        assert!(hub.take_due().is_empty(), "not quiet yet");
        hub.mark(1, Instant::now() - WATCH_DEBOUNCE - Duration::from_millis(10));
        assert_eq!(hub.take_due(), vec![1]);
        hub.mark(1, Instant::now() - WATCH_DEBOUNCE - Duration::from_millis(10));
        assert!(hub.take_due().is_empty(), "rescanned too recently");
        assert!(hub.has_dirty(), "still remembered for later");
    }

    #[test]
    fn noise_paths_are_ignored() {
        assert!(ignored(Path::new(r"C:\p\.git\index")));
        assert!(!ignored(Path::new(r"C:\p\package.json")));
    }
}
