use crate::command::AppCommand;
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

pub fn handle_user(code: i32) -> Option<AppCommand> {
    let event = UserEvent::from_code(code);

    match event {
        UserEvent::BrowserWakeup => None,
        UserEvent::BrowserFrameReady => None,
        // Sent by download workers/interception purely to wake the loop; the
        // per-frame downloads poll in `App::run` picks up the new state.
        UserEvent::DownloadUpdate => None,
        // Sent by the hint-collection JS callback purely to wake the loop; the
        // main loop drains the collected rects.
        UserEvent::HintsReady => None,
        // Sent by the embedder-control delegate purely to wake the loop; the
        // main loop drains the pending/dismissed controls.
        UserEvent::ControlPending => None,
        // Sent by the self-update worker purely to wake the loop; the About tab
        // re-reads the updater snapshot each frame, so the wake just repaints.
        UserEvent::UpdateProgress => None,
        // Sent by the gamepad delegate purely to wake the loop; the main loop
        // drains the queued haptic requests.
        UserEvent::HapticPending => None,
    }
}

#[repr(i32)]
#[derive(Copy, Clone)]
pub enum UserEvent {
    BrowserWakeup = 0,
    BrowserFrameReady = 1,
    DownloadUpdate = 2,
    HintsReady = 3,
    ControlPending = 4,
    UpdateProgress = 5,
    HapticPending = 6,
}

impl UserEvent {
    pub const ALL: [UserEvent; 7] = [
        UserEvent::BrowserWakeup,
        UserEvent::BrowserFrameReady,
        UserEvent::DownloadUpdate,
        UserEvent::HintsReady,
        UserEvent::ControlPending,
        UserEvent::UpdateProgress,
        UserEvent::HapticPending,
    ];

    pub fn from_code(code: i32) -> UserEvent {
        Self::ALL[code as usize]
    }
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
