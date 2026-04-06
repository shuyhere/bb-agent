use crate::component::Component;
use crossterm::event::{KeyCode, KeyEvent};
use std::any::Any;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK_MS: u64 = 80;

type ColorFn = Arc<dyn Fn(&str) -> String + Send + Sync>;
type RenderFn = Arc<dyn Fn() + Send + Sync>;
type AbortFn = Arc<dyn Fn() + Send + Sync>;

struct LoaderState {
    current_frame: usize,
    message: String,
}

/// Loader component that updates every 80ms with spinning animation.
pub struct Loader {
    state: Arc<Mutex<LoaderState>>,
    spinner_color_fn: ColorFn,
    message_color_fn: ColorFn,
    request_render: Option<RenderFn>,
    running: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Loader {
    pub fn new(
        request_render: Option<RenderFn>,
        spinner_color_fn: impl Fn(&str) -> String + Send + Sync + 'static,
        message_color_fn: impl Fn(&str) -> String + Send + Sync + 'static,
        message: impl Into<String>,
    ) -> Self {
        let loader = Self {
            state: Arc::new(Mutex::new(LoaderState {
                current_frame: 0,
                message: message.into(),
            })),
            spinner_color_fn: Arc::new(spinner_color_fn),
            message_color_fn: Arc::new(message_color_fn),
            request_render,
            running: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        };
        loader.start();
        loader
    }

    pub fn plain(message: impl Into<String>) -> Self {
        Self::new(None, |s| s.to_string(), |s| s.to_string(), message)
    }

    pub fn start(&self) {
        self.update_display();
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);
        let request_render = self.request_render.clone();
        let frames_len = FRAMES.len();

        let handle = thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(TICK_MS));
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(mut state) = state.lock() {
                    state.current_frame = (state.current_frame + 1) % frames_len;
                }
                if let Some(cb) = &request_render {
                    cb();
                }
            }
        });

        if let Ok(mut worker) = self.worker.lock() {
            *worker = Some(handle);
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            let _ = handle.join();
        }
    }

    pub fn set_message(&self, message: impl Into<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.message = message.into();
        }
        self.update_display();
    }

    fn update_display(&self) {
        if let Some(cb) = &self.request_render {
            cb();
        }
    }

    fn line(&self) -> String {
        let (frame, message) = if let Ok(state) = self.state.lock() {
            (FRAMES[state.current_frame], state.message.clone())
        } else {
            (FRAMES[0], String::from("Loading..."))
        };

        format!(
            "{} {}",
            (self.spinner_color_fn)(frame),
            (self.message_color_fn)(&message)
        )
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Component for Loader {
    fn render(&self, _width: u16) -> Vec<String> {
        vec![String::new(), self.line()]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Loader that can be cancelled with Escape.
pub struct CancellableLoader {
    loader: Loader,
    aborted: Arc<AtomicBool>,
    on_abort: Option<AbortFn>,
}

impl CancellableLoader {
    pub fn new(
        request_render: Option<RenderFn>,
        spinner_color_fn: impl Fn(&str) -> String + Send + Sync + 'static,
        message_color_fn: impl Fn(&str) -> String + Send + Sync + 'static,
        message: impl Into<String>,
    ) -> Self {
        Self {
            loader: Loader::new(request_render, spinner_color_fn, message_color_fn, message),
            aborted: Arc::new(AtomicBool::new(false)),
            on_abort: None,
        }
    }

    pub fn plain(message: impl Into<String>) -> Self {
        Self::new(None, |s| s.to_string(), |s| s.to_string(), message)
    }

    pub fn set_on_abort(&mut self, on_abort: impl Fn() + Send + Sync + 'static) {
        self.on_abort = Some(Arc::new(on_abort));
    }

    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.loader.stop();
    }

    pub fn set_message(&self, message: impl Into<String>) {
        self.loader.set_message(message);
    }
}

impl Component for CancellableLoader {
    fn render(&self, width: u16) -> Vec<String> {
        self.loader.render(width)
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if key.code == KeyCode::Esc
            && !self.aborted.swap(true, Ordering::SeqCst)
            && let Some(on_abort) = &self.on_abort
        {
            on_abort();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
