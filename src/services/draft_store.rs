//! A bounded, coalescing local-draft writer. No persistence work runs on GPUI.
//!
//! A session's latest pending checkpoint replaces older pending checkpoints. A
//! single writer serializes replacement, so an older save cannot win a race.
//! Files belong to Pideck, never to a project or Pi's session/authentication data.

use std::collections::BTreeMap;
use std::fs::File;
#[cfg(test)]
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use crate::state::drafts::StoredDraft;
use super::atomic_file;

const VERSION: u32 = 1;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_DRAFT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PENDING_OWNERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct DraftOwner {
    pub project: String,
    pub session: String,
}

#[derive(Serialize, Deserialize)]
struct Header {
    version: u32,
    owner: DraftOwner,
}

#[derive(Default)]
struct Queue {
    pending: BTreeMap<DraftOwner, Arc<StoredDraft>>,
    in_flight: Option<(DraftOwner, Arc<StoredDraft>)>,
    failures: BTreeMap<DraftOwner, (Arc<StoredDraft>, String)>,
    stopping: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub(crate) struct DraftStore {
    root: PathBuf,
    shared: Arc<Shared>,
}

impl DraftStore {
    pub(crate) fn new(root: PathBuf) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let worker_root = root.clone();
        std::thread::Builder::new().name("pideck-drafts".into())
            .spawn(move || writer_loop(worker_root, worker_shared))?;
        Ok(Self { root, shared })
    }

    pub(crate) fn enqueue(&self, owner: DraftOwner, draft: StoredDraft) -> Result<(), String> {
        let mut queue = self.shared.lock();
        if queue.stopping {
            return Err("Draft storage is closing; your current editor has not been cleared.".into());
        }
        let known = queue.pending.contains_key(&owner) || queue.failures.contains_key(&owner)
            || queue.in_flight.as_ref().is_some_and(|(active, _)| active == &owner);
        if queue.pending.len() + queue.failures.len() + usize::from(queue.in_flight.is_some()) >= MAX_PENDING_OWNERS && !known {
            return Err("Draft storage is busy. The editor is safe in memory; close will retry saving.".into());
        }
        queue.failures.remove(&owner);
        queue.pending.insert(owner, Arc::new(draft));
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Includes the newest queued state, not just the last version on disk.
    /// Call on an I/O worker: disk reads and undo-chain validation may be large.
    pub(crate) fn load(&self, owner: &DraftOwner) -> Result<Option<StoredDraft>, String> {
        let queued = {
            let queue = self.shared.lock();
            queue.pending.get(owner).cloned().or_else(|| {
                queue.in_flight.as_ref().and_then(|(active, draft)| {
                    (active == owner).then(|| Arc::clone(draft))
                })
            }).or_else(|| queue.failures.get(owner).map(|(draft, _)| Arc::clone(draft)))
        };
        // Copy potentially large attachments after releasing the queue mutex:
        // the GPUI thread must never wait for a recovery clone before enqueueing.
        if let Some(draft) = queued {
            return Ok(Some(draft.as_ref().clone()));
        }
        load_file(&self.root, owner)
    }

    /// Wait off the UI thread. Timeout does not cancel a write or erase a draft.
    pub(crate) fn flush(&self, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut queue = self.shared.lock();
        // One retry per explicit flush, including failed owners no longer open
        // in the runtime pool. Retain their data until a replacement succeeds.
        for (owner, (draft, _)) in std::mem::take(&mut queue.failures) {
            if !queue.in_flight.as_ref().is_some_and(|(active, _)| active == &owner) {
                queue.pending.entry(owner).or_insert(draft);
            }
        }
        self.shared.changed.notify_all();
        while !queue.pending.is_empty() || queue.in_flight.is_some() {
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                return Err("Draft saving has not finished. The window was kept open; retry closing.".into());
            };
            let (next, timed) = self.shared.changed.wait_timeout(queue, remaining)
                .unwrap_or_else(|error| error.into_inner());
            queue = next;
            if timed.timed_out() && (!queue.pending.is_empty() || queue.in_flight.is_some()) {
                return Err("Draft saving timed out. The window was kept open; retry closing.".into());
            }
        }
        match queue.failures.values().next() {
            Some((_, error)) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub(crate) fn last_error(&self) -> Option<String> {
        self.shared.lock().failures.values().next().map(|(_, error)| error.clone())
    }
}

impl Drop for DraftStore {
    fn drop(&mut self) {
        // Normal window closure has already awaited flush. Emergency teardown
        // never joins a disk worker on the event loop; queued saves may finish.
        self.shared.lock().stopping = true;
        self.shared.changed.notify_all();
    }
}

fn writer_loop(root: PathBuf, shared: Arc<Shared>) {
    loop {
        let (owner, draft) = {
            let mut queue = shared.lock();
            while queue.pending.is_empty() && !queue.stopping {
                queue = shared.changed.wait(queue).unwrap_or_else(|error| error.into_inner());
            }
            let Some((owner, draft)) = queue.pending.pop_first() else { return; };
            queue.in_flight = Some((owner.clone(), Arc::clone(&draft)));
            (owner, draft)
        };
        let result = save_file(&root, &owner, &draft);
        let mut queue = shared.lock();
        queue.in_flight = None;
        match result {
            Ok(()) => { queue.failures.remove(&owner); }
            Err(error) => {
                if !queue.pending.contains_key(&owner) {
                    queue.failures.insert(owner, (draft, error));
                }
            }
        }
        shared.changed.notify_all();
    }
}

fn path_for(root: &Path, owner: &DraftOwner) -> PathBuf {
    // Stable FNV-1a 128, not DefaultHasher (whose output isn't a file contract).
    // The complete owner is also checked in the header: collisions cannot load
    // another project's editor or overwrite a different session's checkpoint.
    let mut hash = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    for byte in owner.project.bytes().chain([0]).chain(owner.session.bytes()) {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    root.join(format!("{hash:032x}.draft.jsonl"))
}

fn read_header(reader: &mut impl BufRead, expected: &DraftOwner) -> Result<(), String> {
    let mut line = Vec::new();
    reader.take((MAX_HEADER_BYTES + 1) as u64).read_until(b'\n', &mut line)
        .map_err(|error| format!("Draft header could not be read: {error}"))?;
    if line.len() > MAX_HEADER_BYTES || line.last() != Some(&b'\n') {
        return Err("Invalid draft header. The original checkpoint was kept.".into());
    }
    let header: Header = serde_json::from_slice(&line)
        .map_err(|_| "Invalid draft header. The original checkpoint was kept.".to_owned())?;
    if header.version != VERSION {
        return Err(format!("Draft format {} is not supported. The original checkpoint was kept.", header.version));
    }
    if header.owner != *expected {
        return Err("Draft ownership did not match. Neither session was modified.".into());
    }
    Ok(())
}

fn load_file(root: &Path, owner: &DraftOwner) -> Result<Option<StoredDraft>, String> {
    let path = path_for(root, owner);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Draft could not be opened: {error}")),
    };
    let mut reader = BufReader::new(file);
    read_header(&mut reader, owner)
        .map_err(|error| format!("{error} File: {}", path.display()))?;
    let mut bytes = Vec::new();
    reader.take(MAX_DRAFT_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|error| format!("Draft could not be read: {error}"))?;
    if bytes.len() as u64 > MAX_DRAFT_BYTES {
        return Err("Draft exceeds the 64 MiB recovery limit. Its original file was kept.".into());
    }
    let mut draft: StoredDraft = serde_json::from_slice(&bytes)
        .map_err(|_| format!("Draft contents could not be decoded. The original checkpoint was kept. File: {}", path.display()))?;
    draft.validate().map_err(str::to_owned)?;
    Ok(Some(draft))
}

fn save_file(root: &Path, owner: &DraftOwner, draft: &StoredDraft) -> Result<(), String> {
    let path = path_for(root, owner);
    match File::open(&path) {
        Ok(file) => read_header(&mut BufReader::new(file), owner)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(format!("Draft destination could not be opened: {error}")),
    }
    let header = Header { version: VERSION, owner: owner.clone() };
    let mut bytes = serde_json::to_vec(&header).map_err(|error| error.to_string())?;
    if bytes.len() >= MAX_HEADER_BYTES {
        return Err("Draft owner paths are too long. The editor was not cleared.".into());
    }
    bytes.push(b'\n');
    let start = bytes.len();
    serde_json::to_writer(&mut bytes, draft).map_err(|error| error.to_string())?;
    if (bytes.len() - start) as u64 > MAX_DRAFT_BYTES {
        return Err("Draft exceeds the 64 MiB save limit. The previous file and editor were kept.".into());
    }
    atomic_file::write(&path, &bytes).map_err(|error| format!("Draft could not be saved: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::drafts::tests::draft;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("pideck-drafts-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
    }
    fn owner(session: &str) -> DraftOwner {
        DraftOwner { project: "synthetic-project".into(), session: session.into() }
    }

    #[test]
    fn sessions_keep_separate_text_and_undo() {
        let root = root();
        let store = DraftStore::new(root.clone()).unwrap();
        store.enqueue(owner("a"), draft("alpha")).unwrap();
        store.enqueue(owner("b"), draft("beta")).unwrap();
        store.flush(Duration::from_secs(5)).unwrap();
        let mut a = load_file(&root, &owner("a")).unwrap().unwrap();
        assert_eq!(a.editor.buffer.text(), "alpha");
        assert_eq!(load_file(&root, &owner("b")).unwrap().unwrap().editor.buffer.text(), "beta");
        assert!(a.editor.buffer.undo());
        assert_eq!(a.editor.buffer.text(), "");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newest_checkpoint_wins_and_read_sees_unflushed_state() {
        let root = root();
        let store = DraftStore::new(root.clone()).unwrap();
        for index in 0..200 {
            store.enqueue(owner("same"), draft(&format!("draft {index}"))).unwrap();
        }
        assert_eq!(store.load(&owner("same")).unwrap().unwrap().editor.buffer.text(), "draft 199");
        store.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(load_file(&root, &owner("same")).unwrap().unwrap().editor.buffer.text(), "draft 199");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_owner_remains_readable_and_flush_retries_after_repair() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let path = path_for(&root, &owner("a"));
        fs::create_dir(&path).unwrap();
        let store = DraftStore::new(root.clone()).unwrap();
        store.enqueue(owner("a"), draft("not lost on failure")).unwrap();
        assert!(store.flush(Duration::from_secs(5)).is_err());
        assert_eq!(store.load(&owner("a")).unwrap().unwrap().editor.buffer.text(), "not lost on failure");
        fs::remove_dir(&path).unwrap();
        store.flush(Duration::from_secs(5)).unwrap();
        assert_eq!(load_file(&root, &owner("a")).unwrap().unwrap().editor.buffer.text(), "not lost on failure");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_version_is_not_replaced() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let path = path_for(&root, &owner("a"));
        let original = b"{\"version\":999,\"owner\":{\"project\":\"synthetic-project\",\"session\":\"a\"}}\nfuture data";
        fs::write(&path, original).unwrap();
        assert!(load_file(&root, &owner("a")).is_err());
        assert!(save_file(&root, &owner("a"), &draft("replacement")).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mismatched_header_cannot_cross_session_boundary() {
        let root = root();
        save_file(&root, &owner("a"), &draft("private A")).unwrap();
        fs::copy(path_for(&root, &owner("a")), path_for(&root, &owner("b"))).unwrap();
        assert!(load_file(&root, &owner("b")).is_err());
        assert!(save_file(&root, &owner("b"), &draft("B")).is_err());
        assert_eq!(load_file(&root, &owner("a")).unwrap().unwrap().editor.buffer.text(), "private A");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_save_surfaces_to_flush_without_deleting_destination() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let destination = path_for(&root, &owner("a"));
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("valuable"), "keep").unwrap();
        let store = DraftStore::new(root.clone()).unwrap();
        store.enqueue(owner("a"), draft("not lost in editor")).unwrap();
        assert!(store.flush(Duration::from_secs(5)).is_err());
        assert_eq!(fs::read_to_string(destination.join("valuable")).unwrap(), "keep");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
