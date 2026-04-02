use super::*;

pub(super) struct SharedContainer {
    inner: Arc<Mutex<Container>>,
}

impl SharedContainer {
    pub(super) fn new(inner: Arc<Mutex<Container>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedContainer {
    fn render(&self, width: u16) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.render(width))
            .unwrap_or_else(|_| vec!["<container unavailable>".to_string()])
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_input(key);
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.invalidate();
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}




pub(super) struct SharedEditorWrapper {
    inner: Arc<Mutex<Editor>>,
}

impl SharedEditorWrapper {
    pub(super) fn new(inner: Arc<Mutex<Editor>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedEditorWrapper {
    fn render(&self, width: u16) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.render(width))
            .unwrap_or_else(|_| vec!["<editor unavailable>".to_string()])
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_input(key);
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.invalidate();
        }
    }

    fn set_focused(&mut self, focused: bool) {
        if let Ok(mut inner) = self.inner.lock() {
            bb_tui::component::Focusable::set_focused(&mut *inner, focused);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub(super) struct SharedComponentWrapper {
    inner: Arc<Mutex<Box<dyn Component>>>,
}

impl SharedComponentWrapper {
    pub(super) fn new(inner: Arc<Mutex<Box<dyn Component>>>) -> Self {
        Self { inner }
    }
}

impl Component for SharedComponentWrapper {
    fn render(&self, width: u16) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.render(width))
            .unwrap_or_else(|_| vec!["<component unavailable>".to_string()])
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_input(key);
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.invalidate();
        }
    }

    fn set_focused(&mut self, focused: bool) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.set_focused(focused);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
