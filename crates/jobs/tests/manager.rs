// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::UserFacing;
use jobs::{
    Error, Reporter, Task, TaskError, TaskFuture, TaskId, TaskManager, TaskOutcome, TaskState,
    TaskView,
};
use serde_json::json;

const KIND: &str = "fake";

/// Shared observations of fake tasks.
#[derive(Default)]
struct Log {
    started: Mutex<Vec<String>>,
    running: AtomicUsize,
    max_running: AtomicUsize,
}

/// What a fake task does once started.
#[derive(Clone)]
enum Behaviour {
    /// Succeeds right away.
    Succeed,
    /// Fails with a hint.
    Fail,
    /// Loops on checkpoints until the gate opens.
    WaitFor(Arc<AtomicBool>),
}

struct FakeTask {
    name: String,
    behaviour: Behaviour,
    log: Arc<Log>,
}

impl FakeTask {
    fn boxed(name: &str, behaviour: Behaviour, log: &Arc<Log>) -> Box<dyn Task> {
        Box::new(Self {
            name: name.to_owned(),
            behaviour,
            log: Arc::clone(log),
        })
    }
}

impl Task for FakeTask {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn title(&self) -> String {
        format!("Fake {}", self.name)
    }

    fn spec(&self) -> serde_json::Value {
        json!({ "name": self.name })
    }

    fn pausable(&self) -> bool {
        true
    }

    fn run(self: Box<Self>, reporter: Reporter) -> TaskFuture {
        Box::pin(async move {
            let log = &self.log;
            log.started.lock().unwrap().push(self.name.clone());
            let now = log.running.fetch_add(1, Ordering::SeqCst) + 1;
            log.max_running.fetch_max(now, Ordering::SeqCst);
            reporter.stage("Working");
            let result = match &self.behaviour {
                Behaviour::Succeed => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    Ok(TaskOutcome::new(format!("{} done", self.name)))
                }
                Behaviour::Fail => Err(TaskError::failed(
                    "disk full",
                    Some("Free some space.".to_owned()),
                )),
                Behaviour::WaitFor(gate) => loop {
                    if let Err(cancelled) = reporter.checkpoint().await {
                        break Err(cancelled.into());
                    }
                    if gate.load(Ordering::SeqCst) {
                        break Ok(TaskOutcome::new("opened"));
                    }
                    reporter.fraction(Some(0.5));
                    tokio::time::sleep(Duration::from_millis(5)).await;
                },
            };
            log.running.fetch_sub(1, Ordering::SeqCst);
            result
        })
    }
}

fn state_file(dir: &Path) -> PathBuf {
    dir.join("state/tasks.json")
}

fn manager(path: PathBuf) -> TaskManager {
    TaskManager::new(path, Arc::new(|| {})).unwrap()
}

/// Polls until task `id` matches `predicate`, or panics after 5 s.
async fn wait_for(
    manager: &TaskManager,
    id: TaskId,
    predicate: impl Fn(&TaskView) -> bool,
) -> TaskView {
    for _ in 0..500 {
        if let Some(view) = manager.get(id)
            && predicate(&view)
        {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "task {id} never reached the expected state: {:?}",
        manager.get(id)
    );
}

async fn wait_state(manager: &TaskManager, id: TaskId, state: TaskState) -> TaskView {
    wait_for(manager, id, |v| v.state == state).await
}

#[tokio::test]
async fn runs_tasks_one_at_a_time_in_fifo_order() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(state_file(dir.path()));
    let log = Arc::new(Log::default());
    let ids: Vec<TaskId> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| manager.submit(FakeTask::boxed(name, Behaviour::Succeed, &log)))
        .collect();
    for id in &ids {
        let view = wait_state(&manager, *id, TaskState::Done).await;
        assert!(view.finished_at.is_some());
    }
    assert_eq!(*log.started.lock().unwrap(), ["a", "b", "c", "d"]);
    assert_eq!(log.max_running.load(Ordering::SeqCst), 1);
    let first = manager.get(ids[0]).unwrap();
    assert_eq!(first.outcome.as_deref(), Some("a done"));
    assert_eq!(first.title, "Fake a");
    assert_eq!(first.kind, KIND);
}

#[tokio::test]
async fn pauses_resumes_and_cancels() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(state_file(dir.path()));
    let log = Arc::new(Log::default());
    let gate = Arc::new(AtomicBool::new(false));
    let first = manager.submit(FakeTask::boxed(
        "first",
        Behaviour::WaitFor(gate.clone()),
        &log,
    ));
    let second = manager.submit(FakeTask::boxed("second", Behaviour::Succeed, &log));

    wait_state(&manager, first, TaskState::Running).await;
    manager.pause(first).unwrap();
    assert!(matches!(
        manager.pause(first),
        Err(Error::InvalidState { .. })
    ));
    gate.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Paused at a checkpoint; it keeps its slot, so `second` waits.
    assert_eq!(manager.get(first).unwrap().state, TaskState::Paused);
    assert_eq!(manager.get(second).unwrap().state, TaskState::Queued);

    manager.resume(first).unwrap();
    wait_state(&manager, first, TaskState::Done).await;
    wait_state(&manager, second, TaskState::Done).await;

    // Cancel a running task and a queued one.
    let blocked = Arc::new(AtomicBool::new(false));
    let running = manager.submit(FakeTask::boxed(
        "running",
        Behaviour::WaitFor(blocked),
        &log,
    ));
    let queued = manager.submit(FakeTask::boxed("queued", Behaviour::Succeed, &log));
    wait_state(&manager, running, TaskState::Running).await;
    manager.cancel(queued).unwrap();
    assert_eq!(manager.get(queued).unwrap().state, TaskState::Cancelled);
    manager.pause(running).unwrap();
    manager.cancel(running).unwrap();
    manager.cancel(running).unwrap();
    let view = wait_state(&manager, running, TaskState::Cancelled).await;
    assert!(!view.is_cancelling);
    assert!(matches!(
        manager.cancel(running),
        Err(Error::InvalidState { .. })
    ));
    assert!(!log.started.lock().unwrap().contains(&"queued".to_owned()));
}

#[tokio::test]
async fn failure_keeps_summary_and_hint() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(state_file(dir.path()));
    let log = Arc::new(Log::default());
    let id = manager.submit(FakeTask::boxed("x", Behaviour::Fail, &log));
    let view = wait_state(&manager, id, TaskState::Failed).await;
    let error = view.error.unwrap();
    assert_eq!(error.summary, "disk full");
    assert_eq!(error.hint.as_deref(), Some("Free some space."));
}

#[test]
fn user_facing_errors_convert_to_task_errors() {
    struct DiskFull;
    impl UserFacing for DiskFull {
        fn summary(&self) -> String {
            "not enough disk space".into()
        }
        fn hint(&self) -> Option<String> {
            Some("Free 200 GB.".into())
        }
    }
    let error: TaskError = DiskFull.into();
    assert_eq!(
        error,
        TaskError::failed("not enough disk space", Some("Free 200 GB.".into()))
    );
    assert_eq!(TaskError::from(jobs::Cancelled), TaskError::Cancelled);
}

#[tokio::test]
async fn reload_marks_unfinished_tasks_interrupted_and_retries_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(dir.path());
    let log = Arc::new(Log::default());
    let (done, running, queued) = {
        let manager = manager(path.clone());
        let done = manager.submit(FakeTask::boxed("done", Behaviour::Succeed, &log));
        wait_state(&manager, done, TaskState::Done).await;
        let gate = Arc::new(AtomicBool::new(false));
        let running = manager.submit(FakeTask::boxed("running", Behaviour::WaitFor(gate), &log));
        let queued = manager.submit(FakeTask::boxed("queued", Behaviour::Succeed, &log));
        wait_state(&manager, running, TaskState::Running).await;
        manager.flush().await.unwrap();
        (done, running, queued)
    };

    // A new manager on the same file sees the earlier tasks.
    let reloaded = manager(path);
    let states: Vec<_> = reloaded.list().iter().map(|v| (v.id, v.state)).collect();
    assert_eq!(
        states,
        [
            (done, TaskState::Done),
            (running, TaskState::Interrupted),
            (queued, TaskState::Interrupted),
        ]
    );
    assert_eq!(
        reloaded.get(done).unwrap().outcome.as_deref(),
        Some("done done")
    );

    // Without a factory it cannot be retried; a finished task can be removed.
    assert!(matches!(
        reloaded.retry(running),
        Err(Error::NoFactory { .. })
    ));
    assert!(matches!(
        reloaded.retry(done),
        Err(Error::InvalidState { .. })
    ));
    reloaded.remove(done).unwrap();

    let factory_log = Arc::clone(&log);
    reloaded.register_factory(KIND, move |spec| {
        let name = spec["name"]
            .as_str()
            .ok_or_else(|| TaskError::failed("bad spec", None))?;
        Ok(FakeTask::boxed(name, Behaviour::Succeed, &factory_log))
    });
    let retried = reloaded.retry(running).unwrap();
    assert!(retried > queued, "new ids never reuse stored ones");
    assert!(reloaded.get(running).is_none());
    let view = wait_state(&reloaded, retried, TaskState::Done).await;
    assert_eq!(view.outcome.as_deref(), Some("running done"));
    assert!(matches!(
        reloaded.remove(TaskId(9999)),
        Err(Error::UnknownTask { .. })
    ));
}

#[tokio::test]
async fn retry_reports_factory_errors() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(state_file(dir.path()));
    let log = Arc::new(Log::default());
    let id = manager.submit(FakeTask::boxed("x", Behaviour::Fail, &log));
    wait_state(&manager, id, TaskState::Failed).await;
    manager.register_factory(KIND, |_| {
        Err(TaskError::failed(
            "engine folder is gone",
            Some("Pick it again.".into()),
        ))
    });
    let error = manager.retry(id).unwrap_err();
    assert!(matches!(error, Error::Recreate { .. }));
    assert_eq!(error.hint().as_deref(), Some("Pick it again."));
    assert_eq!(manager.get(id).unwrap().state, TaskState::Failed);
}

#[tokio::test]
async fn corrupt_state_file_is_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(dir.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{ not json").unwrap();
    let manager = manager(path.clone());
    assert_eq!(manager.list(), []);
    let backup = path.with_file_name("tasks.json.corrupt");
    assert_eq!(fs::read_to_string(backup).unwrap(), "{ not json");

    // The manager keeps working and writes a valid file again.
    let log = Arc::new(Log::default());
    let id = manager.submit(FakeTask::boxed("x", Behaviour::Succeed, &log));
    wait_state(&manager, id, TaskState::Done).await;
    manager.flush().await.unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["tasks"][0]["state"], "Done");
}

#[tokio::test]
async fn state_changes_are_persisted_without_flush() {
    let dir = tempfile::tempdir().unwrap();
    let path = state_file(dir.path());
    let manager = manager(path.clone());
    let log = Arc::new(Log::default());
    let id = manager.submit(FakeTask::boxed("x", Behaviour::Succeed, &log));
    wait_state(&manager, id, TaskState::Done).await;
    for _ in 0..500 {
        if let Ok(bytes) = fs::read(&path) {
            let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            if saved["tasks"][0]["state"] == "Done" {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the state file never recorded the finished task");
}

#[tokio::test]
async fn on_change_is_called() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let manager = TaskManager::new(
        state_file(dir.path()),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    let log = Arc::new(Log::default());
    let id = manager.submit(FakeTask::boxed("x", Behaviour::Succeed, &log));
    assert!(calls.load(Ordering::SeqCst) >= 1, "submit notifies");
    wait_state(&manager, id, TaskState::Done).await;
    // Submit, start and finish at least.
    assert!(calls.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn keeps_only_the_latest_finished_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(state_file(dir.path()));
    let log = Arc::new(Log::default());
    let total = jobs::MAX_FINISHED_TASKS + 5;
    let ids: Vec<TaskId> = (0..total)
        .map(|i| manager.submit(FakeTask::boxed(&i.to_string(), Behaviour::Succeed, &log)))
        .collect();
    let newest = *ids.last().unwrap();
    wait_state(&manager, newest, TaskState::Done).await;
    let list = manager.list();
    assert_eq!(list.len(), jobs::MAX_FINISHED_TASKS);
    assert_eq!(list[0].id, ids[5]);
}

#[test]
fn new_outside_a_runtime_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let result = TaskManager::new(state_file(dir.path()), Arc::new(|| {}));
    assert!(matches!(result, Err(Error::NoRuntime)));
}
