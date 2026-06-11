//! Modal page prompts: the egui-rendered stand-ins for native controls a page
//! can open — `<select>` pickers and the JS simple dialogs (`alert`, `confirm`,
//! `prompt`). Servo hands these to the embedder (see
//! [`crate::browser::delegate`]); the queue here owns the pending controls and
//! resolves the front one, which [`crate::ui`] draws as a modal overlay.
//!
//! Navigation works over *slots* — the focusable items of the front control:
//! a select's enabled options (plus a trailing **OK** for multi-selects), or a
//! dialog's buttons (**OK**, then **Cancel**). The selection is a slot index;
//! the renderer maps slots back to rows with the same flattening order.

use servo::{EmbedderControl, EmbedderControlId, SelectElement, SimpleDialog};
use std::collections::VecDeque;

pub struct Prompt {
    /// Pending controls, oldest first; the front one is shown. Dropping a
    /// control answers it with its default response (Servo's `Drop` impls),
    /// so anything removed from the queue is automatically resolved.
    queue: VecDeque<EmbedderControl>,
    /// The focused slot of the front control.
    selected: usize,
    /// Option ids chosen in the front select control. Kept here (not pushed
    /// into the control) until **OK**, so cancelling discards the toggles.
    chosen: Vec<usize>,
    /// Edit buffer for a `prompt()` dialog's text field. The on-screen
    /// keyboard types into it via [`crate::osk::OskTarget::Prompt`].
    input: String,
}

impl Prompt {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            selected: 0,
            chosen: Vec::new(),
            input: String::new(),
        }
    }

    #[inline]
    pub fn visible(&self) -> bool {
        !self.queue.is_empty()
    }

    /// The shown control, if any.
    #[inline]
    pub fn front(&self) -> Option<&EmbedderControl> {
        self.queue.front()
    }

    /// Queue a control from the page; it shows once it reaches the front.
    pub fn push(&mut self, control: EmbedderControl) {
        self.queue.push_back(control);
        log::debug!("prompt: control queued (len={})", self.queue.len());
        if self.queue.len() == 1 {
            self.reset_for_front();
        }
    }

    /// Drop the control Servo retracted (page navigated, element removed, …);
    /// dropping it sends its default response.
    pub fn dismiss(&mut self, id: EmbedderControlId) {
        let was_front = self.front().is_some_and(|c| c.id() == id);
        let before = self.queue.len();
        self.queue.retain(|c| c.id() != id);
        if self.queue.len() != before {
            log::debug!("prompt: control dismissed by servo (len={})", self.queue.len());
        }
        if was_front {
            self.reset_for_front();
        }
    }

    #[inline]
    pub fn selected_slot(&self) -> usize {
        self.selected
    }

    /// Whether `id` is among the front select's chosen options (the ☑ / •
    /// markers).
    #[inline]
    pub fn is_chosen(&self, id: usize) -> bool {
        self.chosen.contains(&id)
    }

    /// Whether the front control has a text field (a `prompt()` dialog) — the
    /// on-screen keyboard then types into [`Prompt::input_mut`].
    pub fn has_text_field(&self) -> bool {
        matches!(
            self.front(),
            Some(EmbedderControl::SimpleDialog(SimpleDialog::Prompt(_)))
        )
    }

    /// The `prompt()` dialog's edit buffer (bound to the overlay's text field).
    #[inline]
    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    /// Move the focused slot by one step (any direction counts — the slots
    /// form a single sequence whether laid out as a list or a button row).
    pub fn move_sel(&mut self, dx: i32, dy: i32) {
        let count = self.slot_count();
        if count == 0 {
            return;
        }
        let delta = if dy != 0 { dy } else { dx };
        self.selected =
            (self.selected as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// Focus slot `index` directly (a mouse click lands here before
    /// [`Prompt::activate`]).
    pub fn set_selected(&mut self, index: usize) {
        if index < self.slot_count() {
            self.selected = index;
        }
    }

    /// Activate the focused slot: choose / toggle a select option (a toggle
    /// keeps the picker open), or press the focused dialog button. Resolving
    /// the control pops it; the next queued one (if any) takes over.
    pub fn activate(&mut self) {
        let selected = self.selected;
        match self.queue.front_mut() {
            Some(EmbedderControl::SelectElement(select)) => {
                let ids = slot_ids(select);
                let multiple = select.allow_select_multiple();
                if let Some(&id) = ids.get(selected) {
                    if multiple {
                        // Toggle and stay open; OK (the slot past the options)
                        // submits.
                        match self.chosen.iter().position(|&c| c == id) {
                            Some(i) => _ = self.chosen.remove(i),
                            None => self.chosen.push(id),
                        }
                        return;
                    }
                    self.chosen = vec![id];
                }
                // A chosen single option, or multi's OK slot (also the only
                // slot when nothing is selectable): submit what's chosen.
                let Some(EmbedderControl::SelectElement(mut select)) = self.queue.pop_front()
                else {
                    unreachable!("front was a select element");
                };
                select.select(std::mem::take(&mut self.chosen));
                select.submit();
                self.reset_for_front();
            }
            Some(EmbedderControl::SimpleDialog(_)) => {
                let Some(EmbedderControl::SimpleDialog(dialog)) = self.queue.pop_front() else {
                    unreachable!("front was a simple dialog");
                };
                // Slot 0 is OK, slot 1 Cancel (alerts only have OK).
                match dialog {
                    SimpleDialog::Alert(d) => d.confirm(),
                    SimpleDialog::Confirm(d) if selected == 0 => d.confirm(),
                    SimpleDialog::Confirm(d) => d.dismiss(),
                    SimpleDialog::Prompt(mut d) if selected == 0 => {
                        d.set_current_value(&self.input);
                        d.confirm();
                    }
                    SimpleDialog::Prompt(d) => d.dismiss(),
                }
                self.reset_for_front();
            }
            _ => {}
        }
    }

    /// Dismiss the front control with its default response (**B** / Esc / ✖):
    /// dropping it answers it — a select keeps its original selection, a
    /// dialog cancels (alerts just confirm; that's their only answer).
    pub fn cancel(&mut self) {
        if self.queue.pop_front().is_some() {
            self.reset_for_front();
        }
    }

    /// How many focusable slots the front control has.
    fn slot_count(&self) -> usize {
        match self.front() {
            Some(EmbedderControl::SelectElement(select)) => {
                slot_ids(select).len() + select.allow_select_multiple() as usize
            }
            Some(EmbedderControl::SimpleDialog(SimpleDialog::Alert(_))) => 1,
            Some(EmbedderControl::SimpleDialog(_)) => 2,
            _ => 0,
        }
    }

    /// Prime the per-control state for a new front: land the focus on the
    /// already-selected option (or OK), seed the chosen set and edit buffer.
    fn reset_for_front(&mut self) {
        self.selected = 0;
        self.chosen.clear();
        self.input.clear();
        // Direct field access (not `self.front()`) so the borrow stays on
        // `queue` and the sibling fields can be assigned.
        match self.queue.front() {
            Some(EmbedderControl::SelectElement(select)) => {
                self.chosen = select.selected_options();
                let ids = slot_ids(select);
                self.selected = ids
                    .iter()
                    .position(|id| self.chosen.contains(id))
                    .unwrap_or(0);
            }
            Some(EmbedderControl::SimpleDialog(SimpleDialog::Prompt(p))) => {
                self.input = p.current_value().to_string();
            }
            _ => {}
        }
    }
}

/// A select's enabled option ids in display order — the slot list both the
/// navigation here and the renderer's rows are built over. Disabled options
/// and group labels aren't slots.
pub fn slot_ids(select: &SelectElement) -> Vec<usize> {
    let mut ids = Vec::new();
    for entry in select.options() {
        match entry {
            servo::SelectElementOptionOrOptgroup::Option(o) => {
                if !o.is_disabled {
                    ids.push(o.id);
                }
            }
            servo::SelectElementOptionOrOptgroup::Optgroup { options, .. } => {
                ids.extend(options.iter().filter(|o| !o.is_disabled).map(|o| o.id));
            }
        }
    }
    ids
}
