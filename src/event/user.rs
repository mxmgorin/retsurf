use sdl2::sys::{SDL_Event, SDL_PushEvent, SDL_UserEvent};
use std::cell::RefCell;

/// A queue the main loop drains once per pass. `push` also fires the wake that
/// makes the pass happen while the loop idles at the event pump — the pairing
/// lives here so a push site cannot forget its wake.
pub struct FrameQueue<T> {
    items: RefCell<Vec<T>>,
    wake: Option<UserEvent>,
    sender: UserEventSender,
}

impl<T> FrameQueue<T> {
    pub fn new(wake: UserEvent, sender: UserEventSender) -> Self {
        Self {
            items: RefCell::new(vec![]),
            wake: Some(wake),
            sender,
        }
    }

    /// A queue that deliberately wakes nothing: its items only matter once a
    /// pass happens anyway.
    pub fn silent(sender: UserEventSender) -> Self {
        Self {
            items: RefCell::new(vec![]),
            wake: None,
            sender,
        }
    }

    pub fn push(&self, item: T) {
        self.items.borrow_mut().push(item);
        if let Some(wake) = self.wake {
            self.sender.send(wake);
        }
    }

    /// Take and clear the items queued since the last call.
    pub fn take(&self) -> Vec<T> {
        std::mem::take(&mut self.items.borrow_mut())
    }
}

/// Why the loop was woken. Every variant is a pure wake — the per-frame drains
/// pick up whatever was queued — so the receive side never dispatches on it;
/// the names exist to document the senders.
#[repr(i32)]
#[derive(Copy, Clone)]
pub enum UserEvent {
    /// Servo's own event-loop waker.
    BrowserWakeup = 0,
    BrowserFrameReady = 1,
    /// Download workers and interception; the per-frame downloads poll reads it.
    DownloadUpdate = 2,
    /// The hint-collection JS callback; the loop drains the collected rects.
    HintsReady = 3,
    /// The embedder-control delegate; the loop drains pending/dismissed controls.
    ControlPending = 4,
    /// The self-update worker; the About tab re-reads the snapshot each frame.
    UpdateProgress = 5,
    /// The gamepad delegate; the loop drains the queued haptic requests.
    HapticPending = 6,
}

#[derive(Clone)]
pub struct UserEventSender {
    event_type: u32,
}

impl UserEventSender {
    pub fn new() -> Self {
        Self {
            event_type: unsafe { sdl2::sys::SDL_RegisterEvents(1) },
        }
    }

    pub fn send(&self, event: UserEvent) {
        unsafe {
            let mut evt = SDL_Event {
                user: SDL_UserEvent {
                    type_: self.event_type,
                    timestamp: 0,
                    windowID: 0,
                    code: event as i32,
                    data1: std::ptr::null_mut(),
                    data2: std::ptr::null_mut(),
                },
            };
            SDL_PushEvent(&mut evt);
        }
    }
}

impl servo::EventLoopWaker for UserEventSender {
    fn wake(&self) {
        self.send(UserEvent::BrowserWakeup);
    }

    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
}
