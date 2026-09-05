use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use mop_plugin_common::paths::lexiclean;
use tracing::debug;

use crate::config::VideoConfig;
use crate::dispatch;
use crate::video::VideoConvertOptions;

pub struct WorkerPool {
    tx: Sender<PathBuf>,
    inflight: Arc<Mutex<HashSet<PathBuf>>>,
    _handles: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    pub fn new(workers: usize, cfg: Arc<VideoConfig>) -> Self {
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let rx: Arc<Mutex<Receiver<PathBuf>>> = Arc::new(Mutex::new(rx));
        let inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));

        let mut handles = Vec::new();
        for _ in 0..workers.max(1) {
            let rx = Arc::clone(&rx);
            let inflight = Arc::clone(&inflight);
            let cfg = Arc::clone(&cfg);
            handles.push(thread::spawn(move || worker_loop(rx, inflight, cfg)));
        }

        WorkerPool {
            tx,
            inflight,
            _handles: handles,
        }
    }

    /// Enqueue a path for processing. Returns false when already in flight.
    pub fn submit(&self, path: PathBuf) -> bool {
        let canon = normalize_path(&path);
        {
            let mut set = self.inflight.lock().unwrap();
            if !set.insert(canon.clone()) {
                debug!("dedup: already in flight {}", canon.display());
                return false;
            }
        }
        if self.tx.send(canon.clone()).is_err() {
            self.inflight.lock().unwrap().remove(&canon);
            return false;
        }
        true
    }
}

fn worker_loop(
    rx: Arc<Mutex<Receiver<PathBuf>>>,
    inflight: Arc<Mutex<HashSet<PathBuf>>>,
    cfg: Arc<VideoConfig>,
) {
    let opts = VideoConvertOptions {
        password: None,
        keep_work_dir_on_error: false,
        dry_run: false,
    };
    loop {
        let job = match rx.lock().unwrap().recv() {
            Ok(j) => j,
            Err(_) => break,
        };
        let _ = dispatch::process(&job, &cfg, &opts);
        inflight.lock().unwrap().remove(&job);
    }
}

fn normalize_path(p: &PathBuf) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| lexiclean(p))
}
