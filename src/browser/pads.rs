//! Which pads the page knows about, and under which index.
//!
//! Not SDL's instance id: that grows with every replug, while the Gamepad API
//! indexes a list of connected pads and the engine looks a pad up by that index.
//! The list lives here rather than in the event handler because a `Connected`
//! only reaches the document that is loaded when it is sent — every fresh
//! document has to be told again.

use super::AppBrowser;

struct Pad {
    instance_id: u32,
    /// What the page reports as `Gamepad.id`.
    name: String,
}

#[derive(Default)]
pub struct PadSlots {
    /// `None` is a slot a disconnect freed for reuse.
    slots: Vec<Option<Pad>>,
}

impl PadSlots {
    /// The slot for a newly connected pad, reusing the lowest free one.
    pub fn connect(&mut self, instance_id: u32, name: String) -> usize {
        if let Some(slot) = self.slot_of(instance_id) {
            return slot;
        }
        let pad = Pad { instance_id, name };
        match self.slots.iter().position(Option::is_none) {
            Some(slot) => {
                self.slots[slot] = Some(pad);
                slot
            }
            None => {
                self.slots.push(Some(pad));
                self.slots.len() - 1
            }
        }
    }

    pub fn slot_of(&self, instance_id: u32) -> Option<usize> {
        self.slots
            .iter()
            .position(|pad| pad.as_ref().is_some_and(|p| p.instance_id == instance_id))
    }

    /// The SDL instance in a slot — the reverse of [`Self::slot_of`], for
    /// playing a page's rumble on the device it named.
    pub fn instance_of(&self, slot: usize) -> Option<u32> {
        self.slots.get(slot)?.as_ref().map(|p| p.instance_id)
    }

    /// Free the slot, reporting which one it was.
    pub fn disconnect(&mut self, instance_id: u32) -> Option<usize> {
        let slot = self.slot_of(instance_id)?;
        self.slots[slot] = None;
        Some(slot)
    }

    /// Every live pad, for telling a freshly loaded document what is plugged in.
    pub fn live(&self) -> impl Iterator<Item = (usize, String)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(slot, pad)| pad.as_ref().map(|p| (slot, p.name.clone())))
    }
}

/// The instance a synthesized pad is announced under. SDL counts its own from
/// zero, so the top of the range is one it will never hand out.
const VIRTUAL_INSTANCE: u32 = u32::MAX;

/// What the page reports as `Gamepad.id` for it, so a game that names the pad
/// it is reading says where the input is really from.
const VIRTUAL_NAME: &str = "retsurf mapped pad";

impl AppBrowser {
    /// The slot a map's `pad.<button>` targets are sent on when no device
    /// produced them — a keyboard driving a game that reads only the Gamepad
    /// API. Announced on first use, so a map that asks for none adds no pad.
    pub fn mapped_pad_slot(&self) -> usize {
        if let Some(slot) = self.inner.pads.borrow().slot_of(VIRTUAL_INSTANCE) {
            return slot;
        }
        self.pad_connected(VIRTUAL_INSTANCE, VIRTUAL_NAME.to_string());
        self.inner
            .pads
            .borrow()
            .slot_of(VIRTUAL_INSTANCE)
            .expect("just announced")
    }

    /// Take the synthesized pad away again (leaving Game Mode). A no-op where
    /// it was never needed.
    pub fn drop_mapped_pad(&self) {
        if self.inner.pads.borrow().slot_of(VIRTUAL_INSTANCE).is_some() {
            self.pad_disconnected(VIRTUAL_INSTANCE);
        }
    }

    /// A pad the page should see. Sent now for the document that is loaded, and
    /// again from [`Self::announce_pads`] for every document that follows.
    pub fn pad_connected(&self, instance_id: u32, name: String) {
        let slot = self
            .inner
            .pads
            .borrow_mut()
            .connect(instance_id, name.clone());
        self.handle_input(servo::InputEvent::Gamepad(
            crate::event::gamepad_api::connected(slot, name, self.inner.haptics.get()),
        ));
    }

    pub fn pad_disconnected(&self, instance_id: u32) {
        let Some(slot) = self.inner.pads.borrow_mut().disconnect(instance_id) else {
            return;
        };
        self.handle_input(servo::InputEvent::Gamepad(
            crate::event::gamepad_api::disconnected(slot),
        ));
    }

    /// The slot a pad's input belongs to, or `None` for one never announced.
    pub fn pad_slot(&self, instance_id: u32) -> Option<usize> {
        self.inner.pads.borrow().slot_of(instance_id)
    }

    /// The SDL instance behind a slot, for playing a page's rumble on it.
    pub fn pad_instance(&self, slot: usize) -> Option<u32> {
        self.inner.pads.borrow().instance_of(slot)
    }

    /// Rumble requests queued since the last pass (see the delegate).
    pub fn take_haptic_requests(&self) -> Vec<servo::GamepadHapticEffectRequest> {
        self.inner.haptic_requests.take()
    }

    /// `[input] haptics`, applied live. Documents already loaded keep the
    /// capability they were told at `Connected`; the gate on requests is here.
    pub fn set_haptics(&self, on: bool) {
        self.inner.haptics.set(on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(name: &str) -> String {
        name.to_owned()
    }

    #[test]
    fn a_slot_is_reused_after_its_pad_goes() {
        let mut slots = PadSlots::default();
        assert_eq!(slots.connect(7, pad("one")), 0);
        assert_eq!(slots.connect(9, pad("two")), 1);
        assert_eq!(slots.disconnect(7), Some(0));
        // The replugged pad takes the free slot, not a third one: the page
        // indexes a list of connected pads.
        assert_eq!(slots.connect(11, pad("three")), 0);
        assert_eq!(slots.slot_of(9), Some(1));
    }

    #[test]
    fn connecting_the_same_pad_twice_keeps_its_slot() {
        let mut slots = PadSlots::default();
        assert_eq!(slots.connect(3, pad("one")), 0);
        assert_eq!(slots.connect(3, pad("one")), 0);
        assert_eq!(slots.disconnect(3), Some(0));
        assert_eq!(slots.disconnect(3), None);
    }

    #[test]
    fn a_slot_resolves_back_to_its_instance() {
        let mut slots = PadSlots::default();
        slots.connect(7, pad("one"));
        assert_eq!(slots.instance_of(0), Some(7));
        slots.disconnect(7);
        assert_eq!(slots.instance_of(0), None);
        assert_eq!(slots.instance_of(3), None);
    }

    #[test]
    fn live_lists_what_a_new_document_has_to_be_told() {
        let mut slots = PadSlots::default();
        slots.connect(4, pad("left"));
        slots.connect(5, pad("right"));
        slots.disconnect(4);
        let live: Vec<(usize, String)> = slots.live().collect();
        assert_eq!(live, vec![(1, pad("right"))]);
    }
}
