mod editing;
mod history;
mod key_dispatch;
mod menus;
mod navigation;
mod rendering;
mod selection;
mod types;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KillContinuation {
    NewEntry,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KillDirection {
    Forward,
    Backward,
}

pub use types::Editor;

#[cfg(test)]
mod tests;
