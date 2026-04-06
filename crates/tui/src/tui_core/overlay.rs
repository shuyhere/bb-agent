use crate::component::Component;

use super::{OverlayAnchor, OverlayEntry, OverlayOptions, TUI};

impl TUI {
    /// Show an overlay component with default (Bottom) anchor for legacy compat.
    /// Focus switches to the overlay. Returns the overlay's handle ID.
    pub fn show_overlay(&mut self, component: Box<dyn Component>) -> usize {
        self.show_overlay_with(
            component,
            OverlayOptions {
                anchor: OverlayAnchor::Bottom,
                ..Default::default()
            },
        )
    }

    /// Show an overlay component with explicit positioning options.
    /// Focus switches to the overlay unless `non_capturing` is set.
    /// Returns the overlay's handle ID (stack index at push time).
    pub fn show_overlay_with(
        &mut self,
        mut component: Box<dyn Component>,
        options: OverlayOptions,
    ) -> usize {
        let pre_focus = self.focus_index;
        let non_capturing = options.non_capturing;

        if !non_capturing {
            self.unfocus_current();
            component.set_focused(true);
        }

        let id = self.overlay_stack.len();
        self.overlay_stack.push(OverlayEntry {
            component,
            pre_focus,
            hidden: false,
            non_capturing,
            options,
        });

        if !non_capturing {
            self.focus_index = None;
        }
        id
    }

    /// Hide (pop) the topmost overlay and restore focus to the previous target.
    pub fn hide_overlay(&mut self) {
        if let Some(mut entry) = self.overlay_stack.pop() {
            entry.component.set_focused(false);
            self.focus_index = entry.pre_focus;
            if let Some(idx) = self.focus_index
                && let Some(child) = self.root.children.get_mut(idx)
            {
                child.set_focused(true);
            }
        }
    }

    /// Check if there are any visible (non-hidden) overlays.
    pub fn has_overlay(&self) -> bool {
        self.overlay_stack.iter().any(|entry| !entry.hidden)
    }

    /// Temporarily hide or show an overlay by its handle ID.
    pub fn set_overlay_hidden(&mut self, id: usize, hidden: bool) {
        let Some(entry) = self.overlay_stack.get(id) else {
            return;
        };
        if entry.hidden == hidden {
            return;
        }
        let non_capturing = entry.non_capturing;
        let pre_focus = entry.pre_focus;

        if hidden {
            self.overlay_stack[id].component.set_focused(false);
            self.overlay_stack[id].hidden = true;
            self.focus_index = pre_focus;
            if let Some(idx) = self.focus_index
                && let Some(child) = self.root.children.get_mut(idx)
            {
                child.set_focused(true);
            }
        } else {
            self.overlay_stack[id].hidden = false;
            if !non_capturing {
                self.unfocus_current();
                self.overlay_stack[id].component.set_focused(true);
                self.focus_index = None;
            }
        }
    }

    /// Unfocus whatever is currently focused (root child or topmost overlay).
    fn unfocus_current(&mut self) {
        if let Some(entry) = self
            .overlay_stack
            .iter_mut()
            .rev()
            .find(|entry| !entry.hidden && !entry.non_capturing)
        {
            entry.component.set_focused(false);
            return;
        }
        if let Some(idx) = self.focus_index
            && let Some(child) = self.root.children.get_mut(idx)
        {
            child.set_focused(false);
        }
    }

    /// Get a mutable reference to the topmost visible *capturing* overlay.
    /// Non-capturing overlays are skipped for input dispatch.
    pub(super) fn topmost_capturing_overlay_mut(&mut self) -> Option<&mut OverlayEntry> {
        self.overlay_stack
            .iter_mut()
            .rev()
            .find(|entry| !entry.hidden && !entry.non_capturing)
    }

    /// Downcast the topmost visible overlay to a concrete component type.
    pub fn topmost_overlay_as_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.overlay_stack
            .iter_mut()
            .rev()
            .find(|entry| !entry.hidden)
            .and_then(|entry| entry.component.as_any_mut().downcast_mut::<T>())
    }
}
