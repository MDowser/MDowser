// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `store::writer`: one serialized background writer for encrypted-store
//! saves, shared by `history` and `bookmarks`.
//!
//! - The producer builds the ciphertext and enqueues it — or hands the
//!   worker a `build` closure that produces it there (bookmarks: up to
//!   48 MiB) — and the worker does the file write + `fsync`, so neither a
//!   large store nor a slow/spun-down disk stalls a frame.
//! - ONE worker per store means writes can never reorder (a stale blob
//!   renamed over a newer one).
//! - The queue is BOUNDED to one pending blob: enqueueing while a blob is
//!   still queued replaces it (newest wins). A hung disk therefore cannot
//!   grow memory at the debounce rate.
//! - `flush` lets the worker WRITE the queued blob (never drops it: the
//!   caller cleared its `dirty` flag at enqueue time, so a dropped blob
//!   would silently lose the newest data), signals it to exit once the
//!   queue is empty, and waits at most the caller's timeout. A timeout
//!   returns `false` and KEEPS the worker: callers must not write
//!   independently (ordering), and the next save respawns a stopped worker.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// One queued encrypted write. The target file name is fixed when the
/// worker is spawned: one worker serves exactly one store, so a queued
/// blob can never be written under another store's name.
pub(crate) struct WriteJob {
    pub(crate) dir: PathBuf,
    /// Pre-built ciphertext. Ignored when `build` is set.
    pub(crate) bytes: Vec<u8>,
    /// Delete the store file instead of writing this job ("Delete history
    /// now", forget-on-exit). Serialized behind any in-flight write by the
    /// SAME worker, so a stalled write can no longer land after a forced
    /// exit-time wipe and resurrect the deleted ciphertext. `bytes` is
    /// ignored (the discard helper leaves it empty).
    pub(crate) discard: bool,
    /// Optional builder run ON THE WORKER in place of `bytes`: a store whose
    /// serialization + encryption is large (bookmarks, up to 48 MiB) hands
    /// the worker a cheap snapshot and lets it produce the ciphertext, so
    /// the UI thread neither copies nor encrypts megabytes inside a frame.
    /// `Err` is reported exactly like a failed write.
    #[allow(clippy::type_complexity)]
    pub(crate) build: Option<Box<dyn FnOnce(&std::path::Path) -> Result<Vec<u8>, String> + Send>>,
}

/// Serialized background writer for one encrypted store.
pub(crate) struct StoreWriter {
    shared: Arc<WorkerShared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

/// State shared with the writer thread.
struct WorkerShared {
    /// Store file name inside `job.dir` (`atomic_write` target).
    name: &'static str,
    /// Store label used in error messages ("could not save history: ...").
    label: &'static str,
    /// Newest pending job (older, unwritten blobs are dropped).
    job: Mutex<Option<WriteJob>>,
    /// Signals a new job or the stop request.
    cv: Condvar,
    /// Set by `flush`: the worker exits once the queue is empty.
    stop: AtomicBool,
    /// Set by the worker, WHILE HOLDING THE JOB LOCK, when it observes `stop`
    /// with an empty queue: from then on [`StoreWriter::push`] refuses jobs
    /// (reporting the race to the caller) instead of leaving them in a queue
    /// nobody reads.
    exited: AtomicBool,
    /// Queued + in-flight jobs (0, 1 or 2), so `busy` is exact.
    pending: AtomicUsize,
    last_error: Mutex<Option<String>>,
    /// Set by the worker when a write lands; consumed by the store's `tick`
    /// to clear a previous warning only on a CONFIRMED success.
    succeeded: AtomicBool,
    /// Set by the worker right before it exits (flush waits on `finished_cv`).
    finished: Mutex<bool>,
    finished_cv: Condvar,
}

impl StoreWriter {
    /// Spawns the worker for one store file (`name` inside each job's
    /// directory) whose failures are reported as `could not save {label}`.
    pub(crate) fn spawn(name: &'static str, label: &'static str) -> Self {
        let shared = Arc::new(WorkerShared {
            name,
            label,
            job: Mutex::new(None),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            pending: AtomicUsize::new(0),
            last_error: Mutex::new(None),
            succeeded: AtomicBool::new(false),
            finished: Mutex::new(false),
            finished_cv: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        // `Builder::spawn` (not `thread::spawn`) so a refused thread start
        // is a reported job failure instead of a panic on the UI thread:
        // the shared state is marked exited, `push` then returns the job,
        // and `queue_or_respawn` either finds a fresh thread or reports the
        // failure through the store's own error slot.
        let spawned = std::thread::Builder::new()
            .name(format!("mdowser-store-{label}"))
            .spawn(move || {
                loop {
                    let job = {
                        let mut slot = worker.job.lock().unwrap_or_else(|e| e.into_inner());
                        loop {
                            if let Some(job) = slot.take() {
                                break Some(job);
                            }
                            if worker.stop.load(Ordering::SeqCst) {
                                // Publish the exit decision INSIDE the lock: a
                                // concurrent `push` either lands before this
                                // check (and the job is taken above) or sees
                                // `exited` and reports the failure instead of
                                // leaving the job in a queue nobody reads.
                                worker.exited.store(true, Ordering::SeqCst);
                                break None;
                            }
                            slot = worker.cv.wait(slot).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    let Some(job) = job else { break };
                    // A discard job deletes the store file instead of writing:
                    // it runs HERE, behind any in-flight write, so the delete can
                    // never be overtaken by a stale rename (the forced exit-time
                    // wipe race). Like a write, it is panic-guarded. A job with
                    // a BUILDER (bookmarks) serializes + encrypts here too, so
                    // the UI thread never touches the large plaintext/ciphertext.
                    let build = job.build;
                    let dir = job.dir;
                    let discard = job.discard;
                    let queued = job.bytes;
                    let shared = &worker;
                    let outcome = crate::util::guard_panics(move || {
                        let bytes = match build {
                            Some(build) => match build(&dir) {
                                Ok(bytes) => bytes,
                                Err(msg) => {
                                    shared.record_build_error(msg);
                                    return;
                                }
                            },
                            None => queued,
                        };
                        shared.record(discard, &dir, &bytes);
                    });
                    if let Err(msg) = outcome {
                        let mut slot = worker.last_error.lock().unwrap_or_else(|e| e.into_inner());
                        *slot = Some(msg);
                        worker.succeeded.store(false, Ordering::SeqCst);
                    }
                    worker.pending.fetch_sub(1, Ordering::SeqCst);
                }
                let mut finished = worker.finished.lock().unwrap_or_else(|e| e.into_inner());
                *finished = true;
                worker.finished_cv.notify_all();
            });
        match spawned {
            Ok(handle) => Self {
                shared,
                handle: Some(handle),
            },
            Err(_) => {
                // The thread never started: publish the exit so `push`
                // refuses jobs (never a queue nobody reads).
                shared.exited.store(true, Ordering::SeqCst);
                Self {
                    shared,
                    handle: None,
                }
            }
        }
    }

    /// Queues one write, replacing a queued-but-unwritten blob (newest
    /// wins). `Err(job)` returns the job UNSENT when the worker had already
    /// exited: the caller must replace the worker and retry, never drop the
    /// blob (see [`queue_or_respawn`]).
    fn push(&self, job: WriteJob) -> Result<(), WriteJob> {
        let mut slot = self.shared.job.lock().unwrap_or_else(|e| e.into_inner());
        if self.shared.exited.load(Ordering::SeqCst) {
            return Err(job);
        }
        if slot.replace(job).is_none() {
            self.shared.pending.fetch_add(1, Ordering::SeqCst);
        }
        drop(slot);
        self.shared.cv.notify_one();
        Ok(())
    }

    /// True once [`Self::flush`] asked the worker to stop.
    #[cfg(test)]
    pub(crate) fn is_stopped(&self) -> bool {
        self.shared.stop.load(Ordering::SeqCst)
    }

    /// True while a write is queued or in flight.
    pub(crate) fn busy(&self) -> bool {
        self.shared.pending.load(Ordering::SeqCst) > 0
    }

    /// True when the worker thread can no longer make progress (it exited or
    /// panicked outside the guarded write). Stores use this to respawn a
    /// fresh writer instead of wedging every later save on a dead one: a
    /// dead worker never clears `pending`, so `busy` would stay true
    /// forever.
    pub(crate) fn worker_dead(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
    }

    /// Takes the error of the most recent failed write, if any. A poisoned
    /// lock still hands the message over: silently dropping it would hide
    /// "could not save history/bookmarks" from the user.
    pub(crate) fn take_error(&self) -> Option<String> {
        self.shared
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// True when a write has landed since the previous call (clears a
    /// previous warning only on a CONFIRMED success, never on enqueue).
    pub(crate) fn take_success(&self) -> bool {
        self.shared.succeeded.swap(false, Ordering::SeqCst)
    }

    /// Asks the worker to write everything still queued and exit, waiting
    /// at most `timeout` for it. `true` = the worker drained and was joined
    /// (a write that FAILED is still drained: its error stays in
    /// [`Self::take_error`] for the store's warning, and the caller may then
    /// write synchronously to retry); `false` = still draining (the worker is
    /// kept alive; the caller must not write independently).
    pub(crate) fn flush(&mut self, timeout: Duration) -> bool {
        let shared = Arc::clone(&self.shared);
        {
            let _slot = shared.job.lock().unwrap_or_else(|e| e.into_inner());
            // Do NOT drop a queued blob: the worker loop takes the job
            // before checking `stop`, so the newest data is written, then
            // the worker exits. The caller's store cleared `dirty` when it
            // enqueued, so discarding here used to lose the last save.
            shared.stop.store(true, Ordering::SeqCst);
            shared.cv.notify_all();
        }
        let start = Instant::now();
        let mut finished = shared.finished.lock().unwrap_or_else(|e| e.into_inner());
        while !*finished {
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                return false;
            };
            let (guard, _) = shared
                .finished_cv
                .wait_timeout(finished, remaining)
                .unwrap_or_else(|e| e.into_inner());
            finished = guard;
        }
        drop(finished);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        true
    }
}

impl WorkerShared {
    /// Records a failed job BUILDER (worker-side serialization/encryption):
    /// the store sees it exactly like a failed write.
    fn record_build_error(&self, message: String) {
        let mut slot = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(message);
        self.succeeded.store(false, Ordering::SeqCst);
    }

    /// Runs one job and records its outcome: a `discard` deletes the store
    /// file (best-effort: `NotFound` is success), anything else is a write.
    /// A later success invalidates an earlier failure notice (the file on
    /// disk is current); poisoned locks recover, because losing the notice
    /// would contradict the store's retry contract.
    fn record(&self, discard: bool, dir: &std::path::Path, bytes: &[u8]) {
        if discard {
            match crate::settings::remove_store_file(dir, self.name) {
                Ok(()) => {
                    let mut slot = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
                    *slot = None;
                    self.succeeded.store(true, Ordering::SeqCst);
                }
                Err(e) => {
                    let mut slot = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
                    *slot = Some(format!("could not delete {}: {e}", self.label));
                    self.succeeded.store(false, Ordering::SeqCst);
                }
            }
            return;
        }
        match crate::settings::atomic_write(dir, self.name, bytes) {
            Ok(()) => {
                let mut slot = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
                *slot = None;
                self.succeeded.store(true, Ordering::SeqCst);
            }
            Err(e) => {
                let mut slot = self.last_error.lock().unwrap_or_else(|e| e.into_inner());
                *slot = Some(format!("could not save {}: {e}", self.label));
                self.succeeded.store(false, Ordering::SeqCst);
            }
        }
    }
}

impl Drop for StoreWriter {
    /// Best-effort shutdown: asks the worker to write what is queued and
    /// exit, then joins ONLY if the thread already finished. A worker stuck
    /// in an `fsync` on a hung disk is left detached (a blocking `Drop`
    /// would freeze a frame or the exit path, which is the exact contract
    /// the stores exist to keep); its queued blob is still written when the
    /// disk wakes up, and the process exit reaps the thread.
    fn drop(&mut self) {
        {
            let _slot = self.shared.job.lock().unwrap_or_else(|e| e.into_inner());
            self.shared.stop.store(true, Ordering::SeqCst);
            self.shared.cv.notify_all();
        }
        if self
            .handle
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

/// Queues one job on `slot`, replacing a worker that had already exited.
///
/// A job handed to a stopped-and-drained worker would never run (`push`
/// refuses it), so the failure is retried on a fresh worker before the blob
/// is dropped. `false` only when two spawns in a row could not accept the
/// job; the caller keeps its dirty state and reports the failure.
pub(crate) fn queue_or_respawn(
    slot: &mut Option<StoreWriter>,
    name: &'static str,
    label: &'static str,
    job: WriteJob,
) -> bool {
    let mut job = job;
    for _ in 0..2 {
        if slot.as_ref().is_some_and(StoreWriter::worker_dead) {
            *slot = None;
        }
        let worker = slot.get_or_insert_with(|| StoreWriter::spawn(name, label));
        match worker.push(job) {
            Ok(()) => return true,
            // The worker exited before taking the job: replace it and retry.
            Err(back) => {
                job = back;
                *slot = None;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{queue_or_respawn, StoreWriter, WriteJob};
    use std::path::PathBuf;
    use std::time::Duration;

    /// Queues a write through the private `push` (tests bypass the
    /// respawn-aware helper: they assert on one worker).
    fn enqueue(writer: &StoreWriter, dir: &std::path::Path, bytes: &[u8]) {
        assert!(
            writer
                .push(WriteJob {
                    dir: dir.to_path_buf(),
                    bytes: bytes.to_vec(),
                    discard: false,
                    build: None,
                })
                .is_ok(),
            "a live worker accepts the job"
        );
    }

    /// Fresh, unique scratch directory per test.
    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdowser-storewriter-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// The newest queued blob must reach the disk on `flush`: the caller
    /// cleared its `dirty` flag when it enqueued, so a dropped blob would
    /// silently lose the last save (the exit path used to drop it).
    #[test]
    fn flush_writes_the_queued_blob_before_exiting() {
        let dir = tmp_dir("flush");
        let mut writer = StoreWriter::spawn("store.bin", "store");
        enqueue(&writer, &dir, b"newest");
        assert!(writer.flush(Duration::from_secs(5)), "local write drains");
        assert!(!writer.busy(), "nothing left queued");
        assert!(writer.is_stopped(), "flush asked the worker to exit");
        let landed = std::fs::read(dir.join("store.bin")).expect("file written");
        assert_eq!(landed, b"newest");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Replacing a queued-but-unwritten blob keeps the newest one (the
    /// bounded-queue contract): only one file write happens.
    #[test]
    fn enqueue_replaces_the_pending_blob_with_the_newest() {
        let dir = tmp_dir("replace");
        let mut writer = StoreWriter::spawn("store.bin", "store");
        // A first job occupies the worker; the second replaces none of it
        // (it is in flight). The third replaces the queued second.
        enqueue(&writer, &dir, b"first");
        enqueue(&writer, &dir, b"second");
        enqueue(&writer, &dir, b"third");
        assert!(writer.flush(Duration::from_secs(5)), "local writes drain");
        let landed = std::fs::read(dir.join("store.bin")).expect("file written");
        // Either the first or the second write won the race for the worker;
        // the LAST queued blob is what must land in the end.
        assert_eq!(landed, b"third", "newest wins");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stopped worker that has drained reports `worker_dead`, and `Drop`
    /// stops a live one without blocking: the stores rely on the first to
    /// respawn instead of wedging on a dead thread, and on the second to
    /// never freeze a frame or the exit path.
    #[test]
    fn stopped_worker_reports_dead_and_drop_never_blocks() {
        let dir = tmp_dir("dead");
        let mut writer = StoreWriter::spawn("store.bin", "store");
        enqueue(&writer, &dir, b"x");
        assert!(!writer.worker_dead(), "a live worker is not dead");
        assert!(writer.flush(Duration::from_secs(5)));
        // `flush` joined the thread: the handle is gone, which the stores
        // read as "stopped", not "dead" (they keep the writer until it has
        // drained, then drop it).
        assert!(writer.is_stopped());
        drop(writer);

        // Drop on a LIVE worker returns immediately (the queued blob is
        // written by the detached thread before it exits).
        let live = StoreWriter::spawn("store.bin", "store");
        let started = std::time::Instant::now();
        drop(live);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "Drop must never wait on the worker"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A write failure surfaces through `take_error`, and a later success
    /// clears it (the store warns only while the disk keeps rejecting).
    #[test]
    fn failed_writes_surface_and_recover() {
        let dir = tmp_dir("error");
        // A path that cannot be a file: `atomic_write` fails on it.
        let blocked = dir.join("blocked");
        std::fs::create_dir_all(blocked.join("store.bin")).expect("blocking dir");
        let mut writer = StoreWriter::spawn("store.bin", "store");
        enqueue(&writer, &blocked, b"x");
        assert!(writer.flush(Duration::from_secs(5)));
        let error = writer.take_error().expect("failure reported");
        assert!(error.contains("could not save store"), "{error}");
        // A successful write to a real directory clears the warning. `flush`
        // stopped the previous worker, so the store spawns a fresh one (the
        // documented contract of `is_stopped`).
        let mut writer = StoreWriter::spawn("store.bin", "store");
        enqueue(&writer, &dir, b"ok");
        assert!(writer.flush(Duration::from_secs(5)));
        assert!(writer.take_error().is_none());
        assert!(writer.take_success(), "confirmed success");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A job pushed at a worker that already exited is RETURNED, never left
    /// in a queue nobody reads, and `queue_or_respawn` recovers it on a
    /// fresh worker. Regression: the liveness check and the push used to
    /// race the exit, silently dropping the newest blob (or a "Delete
    /// history" discard, whose ciphertext then survived on disk).
    #[test]
    fn a_job_for_an_exited_worker_is_respawned_not_lost() {
        let dir = tmp_dir("respawn");
        let mut slot = Some(StoreWriter::spawn("store.bin", "store"));
        let writer = slot.as_mut().expect("worker");
        enqueue(writer, &dir, b"first");
        assert!(writer.flush(Duration::from_secs(5)));
        // The flush joined the thread: the worker is gone.
        assert!(
            writer
                .push(WriteJob {
                    dir: dir.clone(),
                    bytes: b"lost".to_vec(),
                    discard: false,
                    build: None,
                })
                .is_err(),
            "an exited worker refuses the job instead of swallowing it"
        );
        // The store's enqueue path respawns and lands the blob.
        let mut writer = slot.take();
        assert!(queue_or_respawn(
            &mut writer,
            "store.bin",
            "store",
            WriteJob {
                dir: dir.clone(),
                bytes: b"recovered".to_vec(),
                discard: false,
                build: None,
            },
        ));
        assert!(writer
            .as_mut()
            .expect("fresh worker")
            .flush(Duration::from_secs(5)));
        let landed = std::fs::read(dir.join("store.bin")).expect("file written");
        assert_eq!(landed, b"recovered");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A discard job deletes the store file instead of writing it, and it
    /// replaces a queued blob (the delete is strictly newer): the exit-time
    /// wipe runs on the SAME worker, behind any in-flight write, so a stale
    /// rename can never resurrect the deleted ciphertext.
    #[test]
    fn discard_deletes_the_store_file_and_replaces_a_queued_write() {
        let dir = tmp_dir("discard");
        std::fs::write(dir.join("store.bin"), b"stale").expect("seed store");
        let mut writer = StoreWriter::spawn("store.bin", "store");
        enqueue(&writer, &dir, b"newest");
        assert!(
            writer
                .push(WriteJob {
                    dir: dir.clone(),
                    bytes: Vec::new(),
                    discard: true,
                    build: None,
                })
                .is_ok(),
            "a live worker accepts the discard"
        );
        assert!(writer.flush(Duration::from_secs(5)));
        assert!(
            !dir.join("store.bin").exists(),
            "discard removed the store file"
        );
        // A missing target is not an error: `wipe` must not warn.
        assert!(
            writer.take_error().is_none(),
            "delete of a missing file is OK"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
