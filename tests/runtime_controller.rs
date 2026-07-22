use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use pi_gui::services::rpc::{ConnectionGeneration, SessionEpoch};
use pi_gui::services::runtime_worker::{
    AttemptGeneration, RuntimeConnection, RuntimePoll, RuntimeService, RuntimeStartFailure,
    RuntimeWorkerHandle, WorkerResult,
};
use pi_gui::state::runtime::{RuntimeEffect, StampedInput};

#[derive(Default)]
struct FakeService {
    connections: Mutex<HashMap<u64, Weak<FakeConnection>>>,
    stop_counts: Mutex<HashMap<u64, Arc<AtomicUsize>>>,
    first_delay: Duration,
}

impl FakeService {
    fn delayed(first_delay: Duration) -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            stop_counts: Mutex::new(HashMap::new()),
            first_delay,
        }
    }

    fn connection(&self, generation: u64) -> Option<Arc<FakeConnection>> {
        self.connections
            .lock()
            .expect("connections")
            .get(&generation)
            .and_then(Weak::upgrade)
    }

    fn stop_count(&self, generation: u64) -> usize {
        self.stop_counts
            .lock()
            .expect("stop counts")
            .get(&generation)
            .map_or(0, |count| count.load(Ordering::Acquire))
    }
}

impl RuntimeService for FakeService {
    fn connect(
        &self,
        generation: ConnectionGeneration,
    ) -> Result<Arc<dyn RuntimeConnection>, RuntimeStartFailure> {
        let stops = Arc::new(AtomicUsize::new(0));
        let connection = Arc::new(FakeConnection {
            stops: Arc::clone(&stops),
        });
        self.connections
            .lock()
            .expect("connections")
            .insert(generation.value(), Arc::downgrade(&connection));
        self.stop_counts
            .lock()
            .expect("stop counts")
            .insert(generation.value(), stops);
        if generation.value() == 1 {
            thread::sleep(self.first_delay);
        }
        Ok(connection)
    }
}

struct FakeConnection {
    stops: Arc<AtomicUsize>,
}

impl RuntimeConnection for FakeConnection {
    fn execute(&self, _effect: RuntimeEffect) -> Option<StampedInput> {
        None
    }

    fn poll(&self, _epoch: SessionEpoch, _timeout: Duration) -> RuntimePoll {
        thread::sleep(Duration::from_millis(2));
        RuntimePoll::Timeout
    }

    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::AcqRel);
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("condition did not become true");
}

#[test]
fn obsolete_delayed_startup_is_stopped_after_newer_attempt_connects() {
    let service = Arc::new(FakeService::delayed(Duration::from_millis(80)));
    let worker = RuntimeWorkerHandle::spawn(service.clone());
    let results = worker.results();
    let first = AttemptGeneration::new(1);
    let second = AttemptGeneration::new(2);
    assert!(worker.connect(first, ConnectionGeneration::new(1)));
    assert!(worker.connect(second, ConnectionGeneration::new(2)));

    loop {
        if matches!(
            results.recv_blocking().expect("worker result"),
            WorkerResult::Connected { attempt, generation }
                if attempt == second && generation == ConnectionGeneration::new(2)
        ) {
            break;
        }
    }

    wait_until(|| service.stop_count(1) == 1);
    assert_eq!(service.stop_count(2), 0);
    drop(worker);
}

#[test]
fn dropping_worker_during_delayed_startup_stops_late_connection() {
    let service = Arc::new(FakeService::delayed(Duration::from_millis(80)));
    let worker = RuntimeWorkerHandle::spawn(service.clone());
    assert!(worker.connect(AttemptGeneration::new(1), ConnectionGeneration::new(1)));
    drop(worker);

    wait_until(|| service.stop_count(1) == 1);
}

#[test]
fn shutdown_is_send_only_once_and_releases_active_connection() {
    let service = Arc::new(FakeService::default());
    let worker = RuntimeWorkerHandle::spawn(service.clone());
    let results = worker.results();
    let attempt = AttemptGeneration::new(1);
    assert!(worker.connect(attempt, ConnectionGeneration::new(1)));
    loop {
        if matches!(
            results.recv_blocking().expect("worker result"),
            WorkerResult::Connected { attempt: connected, .. } if connected == attempt
        ) {
            break;
        }
    }

    let connection = service.connection(1).expect("active connection");
    let weak = Arc::downgrade(&connection);
    assert!(worker.request_shutdown());
    assert!(!worker.request_shutdown());
    drop(connection);
    drop(worker);

    wait_until(|| weak.upgrade().is_none());
    assert_eq!(service.stop_count(1), 1);
}
