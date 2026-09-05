use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::config::VideoConfig;
use crate::worker::WorkerPool;

const DEBOUNCE_DURATION: Duration = Duration::from_secs(2);
const TICK_INTERVAL: Duration = Duration::from_millis(500);

pub struct ResidentWatcher {
    _watcher: RecommendedWatcher,
}

impl ResidentWatcher {
    pub fn start(cfg: Arc<VideoConfig>, pool: Arc<WorkerPool>) -> Result<Self, notify::Error> {
        let (tx, mut rx) = mpsc::unbounded_channel::<notify::Result<Event>>();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )?;

        for dir in &cfg.watch_dirs {
            if dir.exists() {
                info!("watching video directory: {}", dir.display());
                if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                    warn!("failed to watch {}: {e}", dir.display());
                }
            } else {
                warn!("watch directory does not exist: {}", dir.display());
            }
        }

        if cfg.scan_on_start {
            scan_existing(&cfg, &pool);
        }

        let cfg_clone = Arc::clone(&cfg);
        let pool_clone = Arc::clone(&pool);
        tokio::spawn(async move {
            let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
            let mut ticker = tokio::time::interval(TICK_INTERVAL);

            loop {
                tokio::select! {
                    Some(event_res) = rx.recv() => {
                        match event_res {
                            Ok(event) => {
                                if !matches!(
                                    event.kind,
                                    notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                                ) {
                                    continue;
                                }
                                for path in event.paths {
                                    if is_candidate_file(&path, &cfg_clone) {
                                        pending.insert(path, Instant::now());
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("notify error: {e}");
                            }
                        }
                    }
                    _ = ticker.tick() => {
                        let now = Instant::now();
                        let ready: Vec<PathBuf> = pending
                            .iter()
                            .filter(|(_, seen)| now.duration_since(**seen) >= DEBOUNCE_DURATION)
                            .map(|(p, _)| p.clone())
                            .collect();

                        for path in ready {
                            pending.remove(&path);
                            if path.exists() && is_candidate_file(&path, &cfg_clone) {
                                debug!("video watcher debounce ready, submitting: {}", path.display());
                                pool_clone.submit(path);
                            }
                        }
                    }
                }
            }
        });

        Ok(ResidentWatcher { _watcher: watcher })
    }
}

pub fn scan_existing(cfg: &VideoConfig, pool: &WorkerPool) {
    info!("scanning existing files in watch_dirs (scan_on_start=true)");
    for dir in &cfg.watch_dirs {
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if is_candidate_file(path, cfg) {
                info!("scan_on_start found video candidate: {}", path.display());
                pool.submit(path.to_path_buf());
            }
        }
    }
}

fn is_candidate_file(path: &Path, cfg: &VideoConfig) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if file_name.starts_with('.')
        || file_name.ends_with(".tmp")
        || file_name.ends_with(".part")
        || file_name.ends_with(".crdownload")
    {
        return false;
    }
    if path.starts_with(&cfg.video_dir) || path.starts_with(&cfg.work_dir) {
        return false;
    }
    true
}
