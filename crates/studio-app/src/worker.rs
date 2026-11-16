//! One heavy-operation thread: coalesced preview and a preserved export snapshot.
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
};
use studio_core::{CoreResult, Evaluated, Evaluator, Scene};
use studio_geometry::boolean::CancelToken;
pub struct Evaluation {
    pub revision: u64,
    pub result: CoreResult<Vec<Evaluated>>,
}
pub struct Exported {
    pub revision: u64,
    pub path: PathBuf,
    pub result: CoreResult<()>,
}
struct Request {
    revision: u64,
    scene: Scene,
    token: CancelToken,
}
struct Export {
    request: Request,
    path: PathBuf,
}
enum Job {
    Preview(Request),
    Export(Export),
}
#[derive(Default)]
struct State {
    pending: Option<Request>,
    result: Option<Evaluation>,
    cancel: Option<CancelToken>,
    export: Option<Export>,
    export_result: Option<Exported>,
    export_cancel: Option<CancelToken>,
    export_busy: bool,
    stop: bool,
}
pub struct GeometryWorker {
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl GeometryWorker {
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let thread_state = shared.clone();
        std::thread::Builder::new()
            .name("cubilo-geometry".into())
            .spawn(move || {
                let mut evaluator = Evaluator::default();
                loop {
                    let job = {
                        let (lock, ready) = &*thread_state;
                        let mut state = lock.lock().unwrap();
                        while !state.stop && state.pending.is_none() && state.export.is_none() {
                            state = ready.wait(state).unwrap();
                        }
                        if state.stop {
                            return;
                        }
                        if let Some(export) = state.export.take() {
                            Job::Export(export)
                        } else {
                            Job::Preview(state.pending.take().unwrap())
                        }
                    };
                    match job {
                        Job::Preview(request) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    evaluator.evaluate_with_token(&request.scene, &request.token)
                                }))
                                .unwrap_or_else(|_| {
                                    Err("형상 계산 중 오류가 발생했습니다. 원본은 유지됩니다."
                                        .into())
                                });
                            let mut state = thread_state.0.lock().unwrap();
                            if state.stop {
                                return;
                            }
                            if !request.token.is_cancelled() {
                                state.result = Some(Evaluation {
                                    revision: request.revision,
                                    result,
                                });
                                drop(state);
                                wake();
                            }
                        }
                        Job::Export(export) => {
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || -> CoreResult<()> {
                                    let mesh = studio_core::merge_evaluated(
                                        evaluator.evaluate_with_token(
                                            &export.request.scene,
                                            &export.request.token,
                                        )?,
                                    )?;
                                    studio_io::save_binary_stl_atomic(
                                        &export.path,
                                        &mesh,
                                        true,
                                        Some(&export.request.token),
                                    )
                                    .map_err(|e| e.to_string())
                                },
                            ))
                            .unwrap_or_else(|_| Err("내보내기 계산 중 오류가 발생했습니다".into()));
                            let mut state = thread_state.0.lock().unwrap();
                            state.export_busy = false;
                            state.export_result = Some(Exported {
                                revision: export.request.revision,
                                path: export.path,
                                result,
                            });
                            drop(state);
                            wake();
                        }
                    }
                }
            })
            .expect("geometry worker thread");
        Self { shared }
    }
    pub fn submit(&self, revision: u64, scene: Scene) {
        let (lock, ready) = &*self.shared;
        let mut state = lock.lock().unwrap();
        if let Some(token) = state.cancel.take() {
            token.cancel();
        }
        let token = CancelToken::new();
        state.cancel = Some(token.clone());
        state.pending = Some(Request {
            revision,
            scene,
            token,
        });
        state.result = None;
        ready.notify_one();
    }
    pub fn export(&self, revision: u64, scene: Scene, path: PathBuf) -> bool {
        let (lock, ready) = &*self.shared;
        let mut state = lock.lock().unwrap();
        if state.export_busy {
            return false;
        }
        if let Some(token) = state.cancel.take() {
            token.cancel();
        }
        if let Some(pending) = &mut state.pending {
            pending.token = CancelToken::new();
            state.cancel = Some(pending.token.clone());
        }
        let token = CancelToken::new();
        state.export_cancel = Some(token.clone());
        state.export_busy = true;
        state.export_result = None;
        state.export = Some(Export {
            request: Request {
                revision,
                scene,
                token,
            },
            path,
        });
        ready.notify_one();
        true
    }
    pub fn is_exporting(&self) -> bool {
        self.shared.0.lock().unwrap().export_busy
    }
    pub fn cancel_export(&self) {
        if let Some(token) = &self.shared.0.lock().unwrap().export_cancel {
            token.cancel();
        }
    }
    pub fn take(&self) -> Option<Evaluation> {
        self.shared.0.lock().unwrap().result.take()
    }
    pub fn take_export(&self) -> Option<Exported> {
        self.shared.0.lock().unwrap().export_result.take()
    }
}
impl Drop for GeometryWorker {
    fn drop(&mut self) {
        let (lock, ready) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.stop = true;
        if let Some(token) = &state.cancel {
            token.cancel();
        }
        if let Some(token) = &state.export_cancel {
            token.cancel();
        }
        state.pending = None;
        state.export = None;
        ready.notify_one();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{Command, Editor, Primitive};
    #[test]
    fn newest_scene_finishes_after_superseded_requests() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = GeometryWorker::new(move || {
            let _ = tx.send(());
        });
        let mut editor = Editor::default();
        for revision in 1..=20 {
            editor
                .apply(Command::Add(Primitive::box_default()))
                .unwrap();
            worker.submit(revision, editor.scene.clone());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = worker.take() {
                if result.revision == 20 {
                    assert_eq!(result.result.unwrap().len(), 20);
                    break;
                }
            }
            rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("latest result wakes UI");
        }
    }
    #[test]
    fn export_uses_queued_snapshot_while_preview_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = GeometryWorker::new(move || {
            let _ = tx.send(());
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.stl");
        let mut editor = Editor::default();
        editor
            .apply(Command::Add(Primitive::box_default()))
            .unwrap();
        let expected = editor.scene.export_mesh().unwrap().triangles().len();
        assert!(worker.export(1, editor.scene.clone(), path.clone()));
        editor
            .apply(Command::Add(Primitive::cylinder_default()))
            .unwrap();
        worker.submit(2, editor.scene.clone());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut saved = false;
        let mut latest = false;
        loop {
            if let Some(export) = worker.take_export() {
                assert_eq!(export.revision, 1);
                export.result.unwrap();
                let bytes = std::fs::read(&path).unwrap();
                assert_eq!(bytes.len(), 84 + expected * 50);
                saved = true;
            }
            if let Some(preview) = worker.take() {
                if preview.revision == 2 {
                    assert_eq!(preview.result.unwrap().len(), 2);
                    latest = true;
                }
            }
            if saved && latest {
                break;
            }
            rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("worker result");
        }
    }
}
