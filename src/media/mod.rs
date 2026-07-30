//! servo-media backend: real WebAudio output through SDL2.
//!
//! Servo's only real backend needs GStreamer, absent from the handheld firmwares,
//! so retsurf registers its own before Servo installs the dummy. The audio graph is
//! backend-independent, so all this adds is somewhere for the rendered blocks to go
//! (see [`sink`]). The rest keeps the dummy types: `<audio>`, MediaStream/WebRTC and
//! `decode_audio_data` want a demuxer, a capture stack and a file decoder, none of
//! them SDL2's job — so [`Backend::can_play_type`] answers no.

mod sink;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};

use sdl2::{AudioSubsystem, Sdl};
use servo_base::generic_channel::GenericCallback;
use servo_media::audio::context::{AudioContext, AudioContextOptions};
use servo_media::audio::decoder::AudioDecoder;
use servo_media::audio::sink::AudioSinkError;
use servo_media::audio::{AudioBackend, AudioStreamReader};
use servo_media::player::context::PlayerGLContext;
use servo_media::player::{audio, video, Player, PlayerEvent, StreamType};
use servo_media::streams::capture::MediaTrackConstraintSet;
use servo_media::streams::device_monitor::{MediaDeviceInfo, MediaDeviceMonitor};
use servo_media::streams::registry::{register_stream, unregister_stream, MediaStreamId};
use servo_media::streams::{MediaOutput, MediaSocket, MediaStream, MediaStreamType};
use servo_media::traits::{ClientContextId, MediaInstance};
use servo_media::webrtc::{WebRtcBackend, WebRtcController, WebRtcSignaller};
use servo_media::{Backend, BackendInit, ServoMedia, SupportsMediaType};
use servo_media_dummy::{
    DummyAudioDecoder, DummyMediaOutput, DummyPlayer, DummySocket, DummyStreamReader,
    DummyWebRtcController,
};

use sink::SdlAudioSink;

/// Registers the WebAudio backend, before Servo is built. The returned subsystem
/// must stay alive: dropping it closes every device the sinks opened. `None` leaves
/// Servo to install its own silent backend.
pub fn init(sdl: &Sdl) -> Option<AudioSubsystem> {
    let subsystem = match sdl.audio() {
        Ok(subsystem) => subsystem,
        Err(e) => {
            log::warn!("audio: SDL audio unavailable ({e}); pages will be silent");
            return None;
        }
    };
    log::info!("audio: SDL driver `{}`", subsystem.current_audio_driver());

    ServoMedia::init::<SdlMediaBackend>();
    // Blocks until the shared `OnceLock` is filled, so ours wins the race with
    // Servo's dummy.
    ServoMedia::get();
    Some(subsystem)
}

/// Live audio contexts of one Servo pipeline, so `suspend`/`resume`/`mute` can
/// reach them. `Weak`: an entry never keeps a closed context alive.
type Contexts = Vec<Weak<Mutex<AudioContext>>>;

struct SdlMediaBackend {
    contexts: Mutex<HashMap<ClientContextId, Contexts>>,
    /// Media-instance ids (`MediaInstance::get_id`), unique across the process.
    next_id: AtomicUsize,
}

impl SdlMediaBackend {
    /// Runs `f` over every live context of `id`, dropping the entries that died.
    fn with_contexts(&self, id: &ClientContextId, f: impl Fn(&AudioContext)) {
        let mut contexts = self.contexts.lock().expect("no panics under this lock");
        let Some(entry) = contexts.get_mut(id) else {
            return;
        };
        entry.retain(|weak| match weak.upgrade() {
            Some(context) => {
                f(&context.lock().expect("no panics under this lock"));
                true
            }
            None => false,
        });
    }
}

impl BackendInit for SdlMediaBackend {
    fn init() -> Box<dyn Backend> {
        Box::new(SdlMediaBackend {
            contexts: Mutex::new(HashMap::new()),
            next_id: AtomicUsize::new(0),
        })
    }
}

impl Backend for SdlMediaBackend {
    fn create_audio_context(
        &self,
        id: &ClientContextId,
        options: AudioContextOptions,
    ) -> Result<Arc<Mutex<AudioContext>>, AudioSinkError> {
        // An AudioContext announces its teardown here; liveness is tracked with
        // `Weak`s instead, so dropping the receiver makes its shutdown ack return.
        let (backend_chan, _) = mpsc::channel();
        let context = AudioContext::new::<Self>(
            self.next_id.fetch_add(1, Ordering::Relaxed),
            id,
            Arc::new(Mutex::new(backend_chan)),
            options,
        )?;
        let context = Arc::new(Mutex::new(context));

        let mut contexts = self.contexts.lock().expect("no panics under this lock");
        // Pipelines come and go; drop whatever they left behind before growing the map.
        contexts.retain(|_, entry| {
            entry.retain(|weak| weak.strong_count() > 0);
            !entry.is_empty()
        });
        contexts
            .entry(*id)
            .or_default()
            .push(Arc::downgrade(&context));
        Ok(context)
    }

    fn mute(&self, id: &ClientContextId, val: bool) {
        self.with_contexts(id, |context| {
            let _ = context.mute(val);
        });
    }

    /// Document no longer fully active: stopping the render thread pauses the device,
    /// which is what quiets a background page.
    fn suspend(&self, id: &ClientContextId) {
        self.with_contexts(id, |context| {
            let _ = context.suspend();
        });
    }

    fn resume(&self, id: &ClientContextId) {
        self.with_contexts(id, |context| {
            let _ = context.resume();
        });
    }

    /// No `Player`, so a page gets a fallback instead of a load that never completes.
    fn can_play_type(&self, _media_type: &str) -> SupportsMediaType {
        SupportsMediaType::No
    }

    fn create_player(
        &self,
        _id: &ClientContextId,
        _: StreamType,
        _: GenericCallback<PlayerEvent>,
        _: Option<Arc<Mutex<dyn video::VideoFrameRenderer>>>,
        _: Option<Arc<Mutex<dyn audio::AudioRenderer>>>,
        _: Box<dyn PlayerGLContext>,
    ) -> Arc<Mutex<dyn Player>> {
        Arc::new(Mutex::new(DummyPlayer))
    }

    fn create_audiostream(&self) -> MediaStreamId {
        SilentStream::register(MediaStreamType::Audio)
    }

    fn create_videostream(&self) -> MediaStreamId {
        SilentStream::register(MediaStreamType::Video)
    }

    fn create_audioinput_stream(&self, _: MediaTrackConstraintSet) -> Option<MediaStreamId> {
        Some(SilentStream::register(MediaStreamType::Audio))
    }

    fn create_videoinput_stream(&self, _: MediaTrackConstraintSet) -> Option<MediaStreamId> {
        Some(SilentStream::register(MediaStreamType::Video))
    }

    fn create_stream_and_socket(
        &self,
        ty: MediaStreamType,
    ) -> (Box<dyn MediaSocket>, MediaStreamId) {
        (Box::new(DummySocket), SilentStream::register(ty))
    }

    fn create_stream_output(&self) -> Box<dyn MediaOutput> {
        Box::new(DummyMediaOutput)
    }

    fn create_webrtc(&self, signaller: Box<dyn WebRtcSignaller>) -> WebRtcController {
        WebRtcController::new::<Self>(signaller)
    }

    fn get_device_monitor(&self) -> Box<dyn MediaDeviceMonitor> {
        Box::new(NoDeviceMonitor)
    }
}

impl AudioBackend for SdlMediaBackend {
    type Sink = SdlAudioSink;

    fn make_sink() -> Result<Self::Sink, AudioSinkError> {
        Ok(SdlAudioSink::default())
    }

    /// `decodeAudioData` needs a codec, not a device, so the promise never settles.
    fn make_decoder() -> Box<dyn AudioDecoder> {
        Box::new(DummyAudioDecoder)
    }

    /// `MediaStreamAudioSourceNode`. Nothing produces streams here, so it reads silence.
    fn make_streamreader(
        _id: MediaStreamId,
        _sample_rate: f32,
    ) -> Result<Box<dyn AudioStreamReader + Send>, AudioSinkError> {
        Ok(Box::new(DummyStreamReader))
    }
}

impl WebRtcBackend for SdlMediaBackend {
    type Controller = DummyWebRtcController;

    fn construct_webrtc_controller(
        _: Box<dyn WebRtcSignaller>,
        _: WebRtcController,
    ) -> Self::Controller {
        DummyWebRtcController
    }
}

/// A registered but empty media stream; nothing ever writes frames into it.
struct SilentStream {
    id: MediaStreamId,
    ty: MediaStreamType,
}

impl SilentStream {
    fn register(ty: MediaStreamType) -> MediaStreamId {
        register_stream(Arc::new(Mutex::new(Self {
            // Placeholder: `register_stream` overwrites it via `set_id` with the
            // id `Drop` unregisters.
            id: MediaStreamId::new(),
            ty,
        })))
    }
}

impl MediaStream for SilentStream {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_id(&mut self, id: MediaStreamId) {
        self.id = id;
    }

    fn ty(&self) -> MediaStreamType {
        self.ty
    }
}

impl Drop for SilentStream {
    fn drop(&mut self) {
        unregister_stream(&self.id);
    }
}

/// `navigator.mediaDevices.enumerateDevices()`: we expose no capture devices.
struct NoDeviceMonitor;

impl MediaDeviceMonitor for NoDeviceMonitor {
    fn enumerate_devices(&self) -> Option<Vec<MediaDeviceInfo>> {
        Some(vec![])
    }
}
