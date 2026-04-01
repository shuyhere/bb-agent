use bb_tui::component::Component;
use bb_tui::components::Loader;
use std::any::Any;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StatusLoaderStyle {
    Accent,
    Warning,
}

pub(super) struct StatusLoaderComponent {
    pub(super) style: StatusLoaderStyle,
    loader: Loader,
}

impl StatusLoaderComponent {
    pub(super) fn new(style: StatusLoaderStyle, message: impl Into<String>) -> Self {
        let message = message.into();
        let loader = match style {
            StatusLoaderStyle::Accent => Loader::new(
                None,
                |s| format!("[38;2;178;148;187m{s}[0m"),
                |s| format!("[90m{s}[0m"),
                message,
            ),
            StatusLoaderStyle::Warning => Loader::new(
                None,
                |s| format!("[33m{s}[0m"),
                |s| format!("[90m{s}[0m"),
                message,
            ),
        };
        Self { style, loader }
    }

    pub(super) fn set_message(&self, message: impl Into<String>) {
        self.loader.set_message(message);
    }

    pub(super) fn stop(&self) {
        self.loader.stop();
    }
}

impl Component for StatusLoaderComponent {
    fn render(&self, width: u16) -> Vec<String> {
        self.loader.render(width)
    }

    fn invalidate(&mut self) {
        self.loader.invalidate();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
