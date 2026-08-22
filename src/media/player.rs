//! `<audio>`: a servo-media [`Player`] that demuxes with symphonia and plays
//! through SDL2 (see [`super::device`]).
//!
//! Servo's element pushes raw response bytes in as the fetch progresses and the
//! protocol is driven from our side: nothing flows until the player emits
//! `NeedData` (the element's data source starts locked, and every new fetch —
//! including the one a seek starts — locks again), backpressure is `push_data`
//! returning `Err(EnoughData)` (the element cancels the fetch), and the only way
//! to get bytes at an offset is the `SeekData(offset, SeekLock)` handshake (the
//! element starts a `Range` fetch there and unlocks). `MetadataUpdated` then
//! `StateChanged(Paused)` are what make the element playable at all — ready
//! state moves HaveNothing -> HaveMetadata -> HaveEnoughData on exactly those.
//!
//! Trait methods run on the script thread under the element's mutex and must
//! never block; everything slow lives on one decoder thread per player, which
//! owns the SDL device and does the blocking `SeekData` handshakes. The SDL
//! audio thread only drains the PCM queue. H.264 video tracks are routed to
//! [`super::video`], which presents against this player's clock.

use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use servo_base::generic_channel::{self, GenericCallback};
use servo_media::player::metadata::Metadata;
use servo_media::player::video::VideoFrameRenderer;
use servo_media::player::{
    PlaybackState, Player, PlayerError, PlayerEvent, SeekLock, SeekLockMsg, StreamType,
};
use servo_media::streams::registry::MediaStreamId;
use servo_media::traits::{MediaInstance, MediaInstanceError};
use servo_media::SupportsMediaType;
use symphonia::core::codecs::audio::AudioDecoder as SymphoniaDecoder;
use symphonia::core::codecs::audio::AudioDecoderOptions as SymphoniaOptions;
use symphonia::core::codecs::video::well_known::extra_data::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG;
use symphonia::core::codecs::video::well_known::CODEC_ID_H264;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia::core::units::{Time, TimeBase, Timestamp};

use super::device::{Device, CHANNELS};
use super::video::VideoPipeline;

/// `push_data` refuses bytes past this much buffered input; the element cancels
/// the fetch and the decoder requests a refetch when it runs dry. Bounds what a
/// parked page can pin no matter how large the file is.
const BYTE_HIGH_WATER: usize = 16 * 1024 * 1024;

/// Already-read bytes kept behind the cursor so small backward seeks stay local
/// instead of costing a refetch round-trip.
const KEEP_BACK_BYTES: usize = 4 * 1024 * 1024;

/// Decoded PCM buffered ahead of the device; rides out refetch latency.
const PCM_TARGET_SECONDS: f64 = 1.0;

/// Minimum advance between `PositionChanged` events.
const POSITION_EVENT_SECONDS: f64 = 0.25;

/// `pending_seek` sentinel for "none"; a real seek target is never NaN because
/// the element clamps it into the seekable ranges first.
const NO_SEEK: u64 = u64::MAX;

/// Poison-tolerant lock: the SDL callback must not unwind across FFI, and a
/// panicked decoder thread must not take the script thread down with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait<'a, T>(cv: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where `PlayerEvent`s go; a trait so tests can collect them.
trait EventSink: Send + Sync {
    fn send(&self, event: PlayerEvent);
}

/// The element-side callback queues a task per event; failures mean the element
/// is gone, which the quit flag handles.
struct CallbackSink(Mutex<GenericCallback<PlayerEvent>>);

impl EventSink for CallbackSink {
    fn send(&self, event: PlayerEvent) {
        let _ = lock(&self.0).send(event);
    }
}

/// Raw response bytes, one contiguous region `[base, base + data.len())` of the
/// resource: fetches are linear and every refetch resets the region.
struct StreamBuf {
    base: u64,
    data: Vec<u8>,
    /// Absolute read cursor; may point outside the region until `read` resolves it.
    read_pos: u64,
    total_len: Option<u64>,
    /// The response answered 206, i.e. the server honors `Range`.
    response_seekable: bool,
    eos: bool,
    /// The fetch died from our `EnoughData` error; reads at the head must refetch.
    stalled: bool,
}

impl StreamBuf {
    fn head(&self) -> u64 {
        self.base + self.data.len() as u64
    }

    /// Drops read bytes beyond the keep-back window.
    fn evict(&mut self) {
        let consumed = (self.read_pos - self.base) as usize;
        if consumed > KEEP_BACK_BYTES {
            let n = consumed - KEEP_BACK_BYTES;
            self.data.drain(..n);
            self.base += n as u64;
        }
    }
}

/// Decoded interleaved-stereo PCM waiting for the device.
struct Pcm {
    queue: VecDeque<f32>,
    /// Frames pushed since `base_secs`; played = pushed - queued.
    pushed_frames: u64,
    /// Media time of the first frame pushed after the last seek.
    base_secs: f64,
    /// Media time of the newest decoded sample; `buffered()`'s fallback.
    decoded_secs: f64,
    last_emitted_pos: f64,
}

struct MetaState {
    duration_secs: Option<f64>,
    sample_rate: Option<u32>,
}

/// Media clock: audio position when an audio track exists, a pause-aware
/// wallclock otherwise (muted video-only files).
struct Clock {
    audio_rate: Option<u32>,
    /// Set while playing without audio.
    anchor: Option<Instant>,
    at: f64,
}

pub(crate) struct Shared {
    stream: Mutex<StreamBuf>,
    /// Wakes a read blocked on more bytes (`push_data`, `end_of_stream`, quit, seek).
    bytes_cv: Condvar,
    pcm: Mutex<Pcm>,
    /// Wakes the decoder: SDL drained the queue, or play/seek/stop changed state.
    work_cv: Condvar,
    meta: Mutex<MetaState>,
    /// f64 bits of the seek target; [`NO_SEEK`] when none. Latest seek wins.
    pending_seek: AtomicU64,
    /// Honor `pending_seek` in reads only once the probe is done — aborting a
    /// probe-time read would kill the player before it ever produced metadata.
    probed: AtomicBool,
    quit: AtomicBool,
    paused: AtomicBool,
    muted: AtomicBool,
    /// `set_audio_track(0, enabled)`; we always report exactly one track.
    track_enabled: AtomicBool,
    /// `set_video_track(0, enabled)`: off skips the paint, decode continues.
    video_track_enabled: AtomicBool,
    /// f64 bits; applied per-sample in the SDL callback.
    volume: AtomicU64,
    clock: Mutex<Clock>,
    events: Box<dyn EventSink>,
}

impl Shared {
    fn new(events: Box<dyn EventSink>) -> Self {
        Self {
            stream: Mutex::new(StreamBuf {
                base: 0,
                data: Vec::new(),
                read_pos: 0,
                total_len: None,
                response_seekable: false,
                eos: false,
                stalled: false,
            }),
            bytes_cv: Condvar::new(),
            pcm: Mutex::new(Pcm {
                queue: VecDeque::new(),
                pushed_frames: 0,
                base_secs: 0.0,
                decoded_secs: 0.0,
                last_emitted_pos: 0.0,
            }),
            work_cv: Condvar::new(),
            meta: Mutex::new(MetaState {
                duration_secs: None,
                sample_rate: None,
            }),
            pending_seek: AtomicU64::new(NO_SEEK),
            probed: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            muted: AtomicBool::new(false),
            track_enabled: AtomicBool::new(true),
            video_track_enabled: AtomicBool::new(true),
            volume: AtomicU64::new(1f64.to_bits()),
            clock: Mutex::new(Clock {
                audio_rate: None,
                anchor: None,
                at: 0.0,
            }),
            events,
        }
    }

    pub(crate) fn seek_pending(&self) -> bool {
        self.probed.load(Ordering::SeqCst) && self.pending_seek.load(Ordering::SeqCst) != NO_SEEK
    }

    fn take_pending_seek(&self) -> Option<f64> {
        let bits = self.pending_seek.swap(NO_SEEK, Ordering::SeqCst);
        (bits != NO_SEEK).then(|| f64::from_bits(bits))
    }

    fn wake_all(&self) {
        self.bytes_cv.notify_all();
        self.work_cv.notify_all();
    }

    pub(crate) fn is_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub(crate) fn has_audio(&self) -> bool {
        lock(&self.clock).audio_rate.is_some()
    }

    pub(crate) fn video_track_enabled(&self) -> bool {
        self.video_track_enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn send_event(&self, event: PlayerEvent) {
        self.events.send(event);
    }

    pub(crate) fn notify_work(&self) {
        self.work_cv.notify_all();
    }

    /// Current media time. Lock order: clock before pcm, never the reverse.
    pub(crate) fn clock_secs(&self) -> f64 {
        let clock = lock(&self.clock);
        if let Some(rate) = clock.audio_rate {
            let pcm = lock(&self.pcm);
            let played = pcm
                .pushed_frames
                .saturating_sub(pcm.queue.len() as u64 / u64::from(CHANNELS));
            return pcm.base_secs + played as f64 / f64::from(rate);
        }
        match clock.anchor {
            Some(anchor) => clock.at + anchor.elapsed().as_secs_f64(),
            None => clock.at,
        }
    }

    /// Freeze the wallclock (pause/stop); no-op under an audio clock.
    fn clock_freeze(&self) {
        let mut clock = lock(&self.clock);
        if let Some(anchor) = clock.anchor.take() {
            clock.at += anchor.elapsed().as_secs_f64();
        }
    }

    /// Resume the wallclock; the audio clock resumes by itself.
    fn clock_run(&self) {
        let mut clock = lock(&self.clock);
        if clock.audio_rate.is_none() && clock.anchor.is_none() {
            clock.anchor = Some(Instant::now());
        }
    }

    /// Jump the wallclock to a seek target, keeping its run state.
    fn clock_jump(&self, at: f64) {
        let mut clock = lock(&self.clock);
        clock.at = at;
        if clock.anchor.is_some() {
            clock.anchor = Some(Instant::now());
        }
    }
}

/// SDL's audio thread: drain the PCM queue scaled by volume, zero-fill the rest,
/// and wake the decoder to refill.
///
/// # Safety
///
/// `userdata` is the `Arc<Shared>` the decoder thread holds until after it closed
/// the device, so it is live for every call.
unsafe extern "C" fn player_callback(userdata: *mut c_void, stream: *mut u8, len: c_int) {
    let shared = unsafe { &*(userdata as *const Shared) };
    let out = unsafe {
        std::slice::from_raw_parts_mut(stream as *mut f32, len as usize / size_of::<f32>())
    };

    let silent =
        shared.muted.load(Ordering::Relaxed) || !shared.track_enabled.load(Ordering::Relaxed);
    let factor = if silent {
        0.0
    } else {
        f64::from_bits(shared.volume.load(Ordering::Relaxed)) as f32
    };

    let mut pcm = lock(&shared.pcm);
    let available = pcm.queue.len().min(out.len());
    for (dst, sample) in out.iter_mut().zip(pcm.queue.drain(..available)) {
        *dst = sample * factor;
    }
    out[available..].fill(0.0);
    drop(pcm);

    // Every tick, not just below a low-water mark: the decoder also paces
    // PositionChanged events off these wakes.
    shared.work_cv.notify_all();
}

/// The blocking byte source symphonia reads the resource through. Absence is
/// resolved at `read`: block if the live fetch will deliver it, otherwise run the
/// `SeekData` handshake. `io::Seek` only moves the cursor.
struct ByteReader {
    shared: Arc<Shared>,
    /// `StreamType::Seekable`: refetching at an offset is possible at all.
    seekable: bool,
}

impl ByteReader {
    /// Asks the element for a fetch delivering bytes from `offset`. Blocks until
    /// the element acknowledges through the `SeekLock`. `clear` drops the region
    /// (a jump); a resume at the head keeps it and stays contiguous.
    fn request_range(&self, offset: u64, clear: bool) -> io::Result<()> {
        {
            let mut stream = lock(&self.shared.stream);
            if clear {
                stream.base = offset;
                stream.data.clear();
            }
            stream.stalled = false;
            // The new fetch delivers a fresh body with its own end.
            stream.eos = false;
        }

        let (sender, receiver) = generic_channel::channel::<SeekLockMsg>()
            .ok_or_else(|| io::Error::other("seek-lock channel failed"))?;
        self.shared.events.send(PlayerEvent::SeekData(
            offset,
            SeekLock {
                lock_channel: sender,
            },
        ));
        let (ok, ack) = receiver
            .recv()
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        let _ = ack.send(());
        if !ok {
            return Err(io::Error::other("element refused the refetch"));
        }
        // The new fetch context starts locked; NeedData is what unlocks it.
        self.shared.events.send(PlayerEvent::NeedData);
        Ok(())
    }
}

impl io::Read for ByteReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut stream = lock(&self.shared.stream);
        loop {
            // EOF, not an error: the decoder loop tells quit and seek apart from
            // a real end by checking the flags before treating this as EOS.
            if self.shared.quit.load(Ordering::SeqCst) || self.shared.seek_pending() {
                return Ok(0);
            }

            let head = stream.head();
            if stream.read_pos >= stream.base && stream.read_pos < head {
                let start = (stream.read_pos - stream.base) as usize;
                let n = buf.len().min(stream.data.len() - start);
                buf[..n].copy_from_slice(&stream.data[start..start + n]);
                stream.read_pos += n as u64;
                stream.evict();
                return Ok(n);
            }

            if stream.read_pos == head {
                if stream.eos {
                    return Ok(0);
                }
                if !stream.stalled {
                    stream = wait(&self.shared.bytes_cv, stream);
                    continue;
                }
            }

            // A jump outside the region, or a dry head whose fetch we stalled.
            if !self.seekable {
                return Err(io::Error::from(io::ErrorKind::Unsupported));
            }
            let (offset, clear) = if stream.read_pos == head {
                (head, false)
            } else {
                (stream.read_pos, true)
            };
            drop(stream);
            self.request_range(offset, clear)?;
            stream = lock(&self.shared.stream);
        }
    }
}

impl io::Seek for ByteReader {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let mut stream = lock(&self.shared.stream);
        let new = match pos {
            io::SeekFrom::Start(p) => p as i128,
            io::SeekFrom::Current(d) => stream.read_pos as i128 + d as i128,
            io::SeekFrom::End(d) => match stream.total_len {
                Some(len) => len as i128 + d as i128,
                None => return Err(io::Error::from(io::ErrorKind::Unsupported)),
            },
        };
        if new < 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        stream.read_pos = new as u64;
        Ok(stream.read_pos)
    }
}

impl MediaSource for ByteReader {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        lock(&self.shared.stream).total_len
    }
}

pub struct SdlAudioPlayer {
    id: usize,
    stream_type: StreamType,
    shared: Arc<Shared>,
    // Script-thread-only state; the element serializes access through its mutex.
    can_resume: std::cell::Cell<bool>,
    playback_rate: std::cell::Cell<f64>,
    rate_warned: std::cell::Cell<bool>,
    stopped: std::cell::Cell<bool>,
}

impl SdlAudioPlayer {
    pub fn new(
        id: usize,
        stream_type: StreamType,
        observer: GenericCallback<PlayerEvent>,
        video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
    ) -> Self {
        Self::with_sink(
            id,
            stream_type,
            Box::new(CallbackSink(Mutex::new(observer))),
            video_renderer,
        )
    }

    fn with_sink(
        id: usize,
        stream_type: StreamType,
        events: Box<dyn EventSink>,
        video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
    ) -> Self {
        let shared = Arc::new(Shared::new(events));
        spawn_decoder(shared.clone(), stream_type, video_renderer);
        Self {
            id,
            stream_type,
            shared,
            can_resume: std::cell::Cell::new(false),
            playback_rate: std::cell::Cell::new(1.0),
            rate_warned: std::cell::Cell::new(false),
            stopped: std::cell::Cell::new(false),
        }
    }

    fn shut_down(&self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        self.shared.paused.store(true, Ordering::SeqCst);
        self.shared.clock_freeze();
        lock(&self.shared.pcm).queue.clear();
        self.shared.wake_all();
    }
}

impl Drop for SdlAudioPlayer {
    /// The decoder thread is detached, never joined: it may be blocked in a
    /// `SeekLock` handshake that only resolves once the element's task queue
    /// drops our event. It exits on its own once the flags and channels say so.
    fn drop(&mut self) {
        self.shut_down();
    }
}

impl Player for SdlAudioPlayer {
    fn play(&self) -> Result<(), PlayerError> {
        if !self.shared.paused.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.can_resume.set(false);
        self.shared.clock_run();
        self.shared.wake_all();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Playing));
        Ok(())
    }

    fn pause(&self) -> Result<(), PlayerError> {
        if self.shared.paused.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.can_resume.set(true);
        self.shared.clock_freeze();
        self.shared.wake_all();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Paused));
        Ok(())
    }

    fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    fn can_resume(&self) -> bool {
        self.can_resume.get()
    }

    fn stop(&self) -> Result<(), PlayerError> {
        if self.stopped.replace(true) {
            return Ok(());
        }
        self.can_resume.set(false);
        self.shut_down();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Stopped));
        Ok(())
    }

    fn seek(&self, time: f64) -> Result<(), PlayerError> {
        if self.stream_type != StreamType::Seekable {
            return Err(PlayerError::NonSeekableStream);
        }
        if matches!(lock(&self.shared.meta).duration_secs, Some(duration) if time > duration) {
            return Err(PlayerError::SeekOutOfRange);
        }
        self.shared
            .pending_seek
            .store(time.max(0.0).to_bits(), Ordering::SeqCst);
        self.shared.wake_all();
        Ok(())
    }

    fn seekable(&self) -> Vec<Range<f64>> {
        let response_seekable = lock(&self.shared.stream).response_seekable;
        if self.stream_type == StreamType::Seekable && response_seekable {
            if let Some(duration) = lock(&self.shared.meta).duration_secs {
                return vec![0.0..duration];
            }
        }
        self.buffered()
    }

    fn buffered(&self) -> Vec<Range<f64>> {
        let (base, head, total) = {
            let stream = lock(&self.shared.stream);
            (stream.base, stream.head(), stream.total_len)
        };
        let duration = lock(&self.shared.meta).duration_secs;
        if let (Some(total), Some(duration)) = (total.filter(|total| *total > 0), duration) {
            let scale = duration / total as f64;
            return vec![base as f64 * scale..head as f64 * scale];
        }
        // No sizes to scale by: report what has actually been decoded.
        let pcm = lock(&self.shared.pcm);
        if pcm.decoded_secs > pcm.base_secs {
            return vec![pcm.base_secs..pcm.decoded_secs];
        }
        vec![]
    }

    fn set_mute(&self, muted: bool) -> Result<(), PlayerError> {
        self.shared.muted.store(muted, Ordering::Relaxed);
        Ok(())
    }

    fn muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    fn set_volume(&self, volume: f64) -> Result<(), PlayerError> {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn volume(&self) -> f64 {
        f64::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    fn set_input_size(&self, size: u64) -> Result<(), PlayerError> {
        lock(&self.shared.stream).total_len = Some(size);
        Ok(())
    }

    fn set_seekable(&self, seekable: bool) -> Result<(), PlayerError> {
        lock(&self.shared.stream).response_seekable = seekable;
        Ok(())
    }

    /// Accepted but not applied: nothing here can time-stretch yet. Servo sets
    /// 1.0 before every play, so normal pages never hit the warning.
    fn set_playback_rate(&self, playback_rate: f64) -> Result<(), PlayerError> {
        self.playback_rate.set(playback_rate);
        if playback_rate != 1.0 && !self.rate_warned.replace(true) {
            log::warn!("audio: playbackRate {playback_rate} requested; playing at 1.0");
        }
        Ok(())
    }

    fn playback_rate(&self) -> f64 {
        self.playback_rate.get()
    }

    fn push_data(&self, data: Vec<u8>) -> Result<(), PlayerError> {
        if self.stopped.get() {
            return Err(PlayerError::BufferPushFailed);
        }
        let mut stream = lock(&self.shared.stream);
        if self.stream_type == StreamType::Seekable
            && stream.data.len() + data.len() > BYTE_HIGH_WATER
        {
            // The element cancels the fetch on this; the decoder refetches at the
            // head once it drains. The refused chunk is re-downloaded then.
            stream.stalled = true;
            return Err(PlayerError::EnoughData);
        }
        stream.data.extend_from_slice(&data);
        // A live stream has no refetch, so shed the oldest bytes instead.
        if stream.data.len() > BYTE_HIGH_WATER {
            let n = stream.data.len() - BYTE_HIGH_WATER;
            stream.data.drain(..n);
            stream.base += n as u64;
            stream.read_pos = stream.read_pos.max(stream.base);
        }
        drop(stream);
        self.shared.bytes_cv.notify_all();
        Ok(())
    }

    fn end_of_stream(&self) -> Result<(), PlayerError> {
        lock(&self.shared.stream).eos = true;
        self.shared.bytes_cv.notify_all();
        Ok(())
    }

    fn set_stream(&self, _: &MediaStreamId, _: bool) -> Result<(), PlayerError> {
        Err(PlayerError::SetStreamFailed)
    }

    fn render_use_gl(&self) -> bool {
        false
    }

    fn set_audio_track(&self, stream_index: i32, enabled: bool) -> Result<(), PlayerError> {
        // Exactly one track is ever reported, so only index 0 exists.
        if stream_index != 0 {
            return Err(PlayerError::SetTrackFailed);
        }
        self.shared.track_enabled.store(enabled, Ordering::Relaxed);
        Ok(())
    }

    fn set_video_track(&self, stream_index: i32, enabled: bool) -> Result<(), PlayerError> {
        if stream_index != 0 {
            return Err(PlayerError::SetTrackFailed);
        }
        self.shared
            .video_track_enabled
            .store(enabled, Ordering::Relaxed);
        Ok(())
    }
}

impl MediaInstance for SdlAudioPlayer {
    fn get_id(&self) -> usize {
        self.id
    }

    fn mute(&self, val: bool) -> Result<(), MediaInstanceError> {
        self.set_mute(val).map_err(|_| MediaInstanceError)
    }

    fn suspend(&self) -> Result<(), MediaInstanceError> {
        self.pause().map_err(|_| MediaInstanceError)
    }

    /// Only resumes a suspend-pause; a user-paused element stays paused.
    fn resume(&self) -> Result<(), MediaInstanceError> {
        if !self.can_resume() {
            return Ok(());
        }
        self.play().map_err(|_| MediaInstanceError)
    }
}

/// `canPlayType` and `<source type>` filtering. Probably for what plays
/// outright, Maybe for containers that may hide an unsupported codec, No for
/// the rest. `allow_video` is the `[video] enabled` switch.
pub fn can_play_type(media_type: &str, allow_video: bool) -> SupportsMediaType {
    let media_type = media_type.trim().to_ascii_lowercase();
    let mut parts = media_type.split(';');
    let essence = parts.next().unwrap_or("").trim();
    let codecs: Vec<&str> = parts
        .filter_map(|param| param.trim().strip_prefix("codecs="))
        .flat_map(|list| list.trim_matches('"').split(','))
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .collect();

    let container = match essence {
        "audio/mpeg" | "audio/mp3" | "audio/x-mp3" => Container::Plain,
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => Container::Plain,
        "audio/flac" | "audio/x-flac" => Container::Plain,
        "audio/aac" | "audio/aacp" => Container::Plain,
        "audio/ogg" | "application/ogg" => Container::Ogg,
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => Container::Mp4,
        "video/mp4" | "video/x-m4v" if allow_video => Container::Mp4Video,
        _ => return SupportsMediaType::No,
    };

    if codecs.is_empty() {
        return match container {
            Container::Plain => SupportsMediaType::Probably,
            // The container may hide a codec we have no decoder for
            // (opus in ogg, alac in audio mp4, hevc/av1 in video mp4).
            Container::Ogg | Container::Mp4 | Container::Mp4Video => SupportsMediaType::Maybe,
        };
    }
    let supported = |codec: &&str| match container {
        Container::Plain => true,
        Container::Ogg => matches!(*codec, "vorbis" | "flac"),
        Container::Mp4 => *codec == "mp4a" || codec.starts_with("mp4a.40"),
        Container::Mp4Video => {
            *codec == "mp4a"
                || codec.starts_with("mp4a.40")
                || *codec == "avc1"
                || codec.starts_with("avc1.")
        }
    };
    if codecs.iter().all(supported) {
        SupportsMediaType::Probably
    } else {
        SupportsMediaType::No
    }
}

enum Container {
    /// The MIME essence pins the codec.
    Plain,
    Ogg,
    Mp4,
    Mp4Video,
}

fn spawn_decoder(
    shared: Arc<Shared>,
    stream_type: StreamType,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) {
    let result = thread::Builder::new()
        .name("media-player".into())
        .spawn(move || decoder_thread(shared, stream_type, video_renderer));
    if let Err(e) = result {
        log::warn!("audio: could not spawn the player decoder thread: {e}");
    }
}

/// Everything slow: probe, decode, device, seeks, refetch handshakes.
fn decoder_thread(
    shared: Arc<Shared>,
    stream_type: StreamType,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) {
    // Unlock the element's initial fetch; nothing arrives before this.
    shared.events.send(PlayerEvent::NeedData);

    let seekable = stream_type == StreamType::Seekable;
    let source = ByteReader {
        shared: shared.clone(),
        seekable,
    };

    match run_pipeline(&shared, source, seekable, video_renderer) {
        Ok(()) => {}
        Err(message) => {
            // A quit mid-read surfaces as a decode error; it is not one.
            if !shared.quit.load(Ordering::SeqCst) {
                log::warn!("audio: player failed: {message}");
                shared.events.send(PlayerEvent::Error(message));
            }
        }
    }
    // The device (owned inside run_pipeline) is already closed here, so the SDL
    // callback can no longer observe `shared`.
}

/// Audio-track half of the pipeline.
struct AudioPipe {
    track_id: u32,
    time_base: Option<TimeBase>,
    decoder: Box<dyn SymphoniaDecoder>,
    rate: u32,
    channels: usize,
    target_samples: usize,
    /// Accurate seeks land before the target; trim decoded output up to it.
    discard_until: Option<f64>,
}

/// Video-track half: conversion and handoff to the video thread.
struct VideoPipe {
    track_id: u32,
    time_base: Option<TimeBase>,
    pipeline: VideoPipeline,
    /// The channel filled up; drop packets until the next IDR fits.
    lagging: bool,
}

/// Duration in seconds from the track's own units.
fn track_duration_secs(track: &Track) -> Option<f64> {
    let (tb, duration) = (track.time_base?, track.duration?);
    Some(
        tb.calc_time_saturating(Timestamp::new(duration.get() as i64))
            .as_secs_f64(),
    )
}

fn run_pipeline(
    shared: &Arc<Shared>,
    source: ByteReader,
    seekable: bool,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) -> Result<(), String> {
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("probe failed: {e}"))?;

    // Audio setup; the decoder is built here, rate/channels are pinned by the
    // preroll below.
    let audio_track = reader
        .first_track_known_codec(TrackType::Audio)
        .and_then(|track| {
            let params = track
                .codec_params
                .as_ref()
                .and_then(CodecParameters::audio)?;
            let decoder = symphonia::default::get_codecs()
                .make_audio_decoder(params, &SymphoniaOptions::default())
                .map_err(|e| log::warn!("audio: decoder init failed: {e}"))
                .ok()?;
            Some((
                track.id,
                track.time_base,
                decoder,
                params.sample_rate,
                track.num_frames,
                track_duration_secs(track),
            ))
        });

    // Video setup: H.264 only, and only when the element renders pictures.
    let video = video_renderer
        .filter(|_| crate::media::settings().video)
        .and_then(|renderer| {
            let track = reader.tracks().iter().find(|track| {
                track
                    .codec_params
                    .as_ref()
                    .and_then(CodecParameters::video)
                    .is_some_and(|params| params.codec == CODEC_ID_H264)
            })?;
            let params = track
                .codec_params
                .as_ref()
                .and_then(CodecParameters::video)?;
            let extra = params
                .extra_data
                .iter()
                .find(|data| data.id == VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG)?;
            let pipeline = VideoPipeline::spawn(shared.clone(), renderer, &extra.data)?;
            Some((
                VideoPipe {
                    track_id: track.id,
                    time_base: track.time_base,
                    pipeline,
                    lagging: false,
                },
                (params.width.unwrap_or(0), params.height.unwrap_or(0)),
                track_duration_secs(track),
            ))
        });
    let (video_dims, video_duration) = match &video {
        Some((_, dims, duration)) => (Some(*dims), *duration),
        None => (None, None),
    };
    let mut video = video.map(|(pipe, ..)| pipe);
    if audio_track.is_none() && video.is_none() {
        return Err("no decodable track".into());
    }

    let title = reader.metadata().skip_to_latest().and_then(|revision| {
        revision.media.tags.iter().find_map(|tag| match &tag.std {
            Some(StandardTag::TrackTitle(title)) => Some(title.to_string()),
            _ => None,
        })
    });

    // Audio preroll: one decoded packet pins the true sample rate before
    // anything is announced. Video packets met on the way are routed onward.
    let mut interleaved = Vec::new();
    let mut audio: Option<AudioPipe> = None;
    let mut audio_duration = None;
    if let Some((track_id, time_base, mut decoder, params_rate, num_frames, duration)) = audio_track
    {
        let (rate, channels, first_secs) = loop {
            let packet = match reader.next_packet() {
                Ok(Some(packet)) if packet.track_id == track_id => packet,
                Ok(Some(packet)) => {
                    route_video_packet(&mut video, &packet, true);
                    continue;
                }
                Ok(None) => return Err("stream ended before any audio decoded".into()),
                Err(e) => return Err(format!("demux failed: {e}")),
            };
            match decoder.decode(&packet) {
                Ok(buffer) if buffer.frames() > 0 => {
                    let rate = buffer.spec().rate();
                    let channels = buffer.num_planes();
                    buffer.copy_to_vec_interleaved::<f32>(&mut interleaved);
                    let secs = packet_secs(&packet.pts, time_base, f64::from(rate));
                    break (rate, channels, secs);
                }
                Ok(_) => continue,
                Err(SymphoniaError::DecodeError(e)) => {
                    log::debug!("audio: skipping malformed packet: {e}");
                    continue;
                }
                Err(e) => return Err(format!("decode failed: {e}")),
            }
        };
        audio_duration = num_frames
            .map(|frames| frames as f64 / f64::from(params_rate.unwrap_or(rate)))
            .or(duration);
        lock(&shared.clock).audio_rate = Some(rate);
        audio = Some(AudioPipe {
            track_id,
            time_base,
            decoder,
            rate,
            channels,
            target_samples: (PCM_TARGET_SECONDS * f64::from(rate)) as usize * CHANNELS as usize,
            discard_until: None,
        });
        let mut pcm = lock(&shared.pcm);
        push_stereo(&mut pcm, &interleaved, channels);
        pcm.decoded_secs =
            first_secs + interleaved.len() as f64 / channels as f64 / f64::from(rate);
    }

    let duration_secs = match (audio_duration, video_duration) {
        (Some(a), Some(v)) => Some(a.max(v)),
        (a, v) => a.or(v),
    };
    {
        let mut meta = lock(&shared.meta);
        meta.duration_secs = duration_secs;
        meta.sample_rate = audio.as_ref().map(|a| a.rate);
    }
    let (response_seekable, is_live) = {
        let stream = lock(&shared.stream);
        (stream.response_seekable, stream.total_len.is_none())
    };
    let (width, height) = video_dims.unwrap_or((0, 0));
    shared.events.send(PlayerEvent::MetadataUpdated(Metadata {
        duration: duration_secs.map(std::time::Duration::from_secs_f64),
        width: u32::from(width),
        height: u32::from(height),
        format: if video.is_some() { "video" } else { "audio" }.into(),
        is_seekable: seekable && response_seekable,
        video_tracks: video
            .iter()
            .map(|_| format!("video/h264/{width}x{height}"))
            .collect(),
        audio_tracks: audio
            .iter()
            .map(|a| format!("audio/{}ch/{}Hz", a.channels, a.rate))
            .collect(),
        is_live,
        title,
    }));
    // Prerolled: this is what moves the element to HaveEnoughData -> canplay.
    shared
        .events
        .send(PlayerEvent::StateChanged(PlaybackState::Paused));
    shared.probed.store(true, Ordering::SeqCst);

    let mut device: Option<Device> = None;
    let mut device_paused = true;
    let mut open_failed = false;
    // The demuxer ran dry for good; cleared only by a seek (`loop` attribute).
    let mut at_eof = false;
    let mut eos_sent = false;
    let mut eos_announced = false;

    'main: loop {
        if shared.quit.load(Ordering::SeqCst) {
            break;
        }

        if let Some(target) = shared.take_pending_seek() {
            if let Some(a) = &mut audio {
                let mut pcm = lock(&shared.pcm);
                pcm.queue.clear();
                pcm.pushed_frames = 0;
                pcm.base_secs = target;
                pcm.decoded_secs = target;
                pcm.last_emitted_pos = target;
                drop(pcm);
                a.decoder.reset();
            }
            shared.clock_jump(target);
            if let Some(v) = &mut video {
                v.pipeline.flush.flush_to(target);
                v.lagging = false;
            }
            match seek_to(&mut *reader, target, &audio, &mut video) {
                Ok(()) => {
                    if let Some(a) = &mut audio {
                        a.discard_until = Some(target);
                    }
                }
                // Report done anyway: silence beats an element stuck `seeking`.
                Err(e) => log::warn!("audio: seek to {target:.2}s failed: {e}"),
            }
            at_eof = false;
            eos_sent = false;
            eos_announced = false;
            shared.events.send(PlayerEvent::SeekDone(target));
        }

        // The device follows the flags; the decoder thread is its only owner.
        // Nothing left to play also pauses it, or an ended element would keep
        // the hardware powered ticking silence.
        if let Some(a) = &audio {
            let queue_empty = lock(&shared.pcm).queue.is_empty();
            let want_play = !(shared.paused.load(Ordering::SeqCst) || (at_eof && queue_empty));
            if want_play && device.is_none() && !open_failed {
                let userdata = Arc::as_ptr(shared) as *mut c_void;
                match Device::open(a.rate as f32, player_callback, userdata) {
                    Ok(opened) => device = Some(opened),
                    Err(e) => {
                        // Decoding continues; the queue fills and everything parks.
                        log::warn!("audio: could not open playback device: {e}");
                        open_failed = true;
                    }
                }
            }
            if device_paused == want_play {
                if let Some(device) = &device {
                    device.set_paused(!want_play);
                    device_paused = !want_play;
                }
            }
        }

        if at_eof {
            if !eos_sent {
                eos_sent = true;
                if let Some(v) = &video {
                    v.pipeline.send_eos();
                }
            }
            let mut pcm = lock(&shared.pcm);
            let audio_drained = pcm.queue.is_empty();
            if let Some(a) = &audio {
                let playing = !(shared.paused.load(Ordering::SeqCst) || (at_eof && audio_drained));
                emit_position(shared, &mut pcm, a.rate, playing);
            }
            let video_done = video.as_ref().is_none_or(|v| v.pipeline.is_done());
            if audio_drained && video_done && !eos_announced {
                eos_announced = true;
                if audio.is_some() {
                    // Land currentTime on the clip's end, not the last 250 ms
                    // tick, before the element hears `ended`.
                    let end = pcm.decoded_secs;
                    if end > pcm.last_emitted_pos {
                        pcm.last_emitted_pos = end;
                        shared.events.send(PlayerEvent::PositionChanged(end));
                    }
                }
                drop(pcm);
                shared.events.send(PlayerEvent::EndOfStream);
                continue 'main;
            }
            // Draining (the callback wakes us) or fully idle (a trait call does).
            let _woken = wait(&shared.work_cv, pcm);
            continue 'main;
        }

        if let Some(a) = &audio {
            let mut pcm = lock(&shared.pcm);
            let playing = !shared.paused.load(Ordering::SeqCst);
            emit_position(shared, &mut pcm, a.rate, playing);
            if pcm.queue.len() >= a.target_samples {
                // Full: wait for the SDL callback (or a state change), then
                // re-evaluate everything.
                let _woken = wait(&shared.work_cv, pcm);
                continue 'main;
            }
        }

        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                at_eof = true;
                continue 'main;
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Also how a read aborted by quit or a pending seek surfaces;
                // the loop top sorts the three cases apart.
                if !shared.quit.load(Ordering::SeqCst) && !shared.seek_pending() {
                    at_eof = true;
                }
                continue 'main;
            }
            Err(e) => {
                if shared.quit.load(Ordering::SeqCst) {
                    break 'main;
                }
                shared
                    .events
                    .send(PlayerEvent::Error(format!("demux failed: {e}")));
                break 'main;
            }
        };

        if video
            .as_ref()
            .is_some_and(|v| v.track_id == packet.track_id)
        {
            let has_audio = audio.is_some();
            route_video_packet(&mut video, &packet, has_audio);
            continue 'main;
        }
        let Some(a) = &mut audio else {
            continue 'main;
        };
        if packet.track_id != a.track_id {
            continue 'main;
        }

        let buffer = match a.decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(e)) => {
                log::debug!("audio: skipping malformed packet: {e}");
                continue 'main;
            }
            // A mid-stream format change cannot continue on one device config.
            Err(SymphoniaError::ResetRequired) => {
                log::info!("audio: stream parameters changed mid-play; ending");
                at_eof = true;
                continue 'main;
            }
            Err(e) => {
                shared
                    .events
                    .send(PlayerEvent::Error(format!("decode failed: {e}")));
                break 'main;
            }
        };
        if buffer.frames() == 0 {
            continue 'main;
        }
        if buffer.spec().rate() != a.rate {
            log::info!("audio: sample rate changed mid-play; ending");
            at_eof = true;
            continue 'main;
        }

        let pkt_start = packet_secs(&packet.pts, a.time_base, f64::from(a.rate));
        let frames = buffer.frames();
        buffer.copy_to_vec_interleaved::<f32>(&mut interleaved);
        let mut samples = interleaved.as_slice();

        // An accurate seek lands before the target; trim up to it.
        if let Some(target) = a.discard_until {
            let pkt_end = pkt_start + frames as f64 / f64::from(a.rate);
            if pkt_end <= target {
                continue 'main;
            }
            let skip = (((target - pkt_start) * f64::from(a.rate)).max(0.0) as usize).min(frames);
            samples = &samples[skip * a.channels..];
            a.discard_until = None;
        }

        let mut pcm = lock(&shared.pcm);
        push_stereo(&mut pcm, samples, a.channels);
        pcm.decoded_secs = pkt_start + frames as f64 / f64::from(a.rate);
    }

    // Close (joins SDL's audio thread) before `shared`'s Arc can drop: the
    // callback userdata points into it.
    drop(device);
    Ok(())
}

/// Seek to `target`. Video must restart at an IDR, but symphonia's mp4 seek is
/// sample-accurate: scan backward in steps until the first video packet after
/// the seek point is a keyframe (or the file start), and hand it onward. Audio
/// packets consumed by the scan are pre-target; the audio trim covers them.
fn seek_to(
    reader: &mut dyn FormatReader,
    target: f64,
    audio: &Option<AudioPipe>,
    video: &mut Option<VideoPipe>,
) -> Result<(), SymphoniaError> {
    const KEYFRAME_SCAN_STEP: f64 = 2.0;

    let seek_track = video
        .as_ref()
        .map(|v| v.track_id)
        .or(audio.as_ref().map(|a| a.track_id));
    let mut attempt = target;
    loop {
        let time = Time::try_from_secs_f64(attempt.max(0.0)).unwrap_or(Time::ZERO);
        reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: seek_track,
            },
        )?;
        if video.is_none() {
            return Ok(());
        }
        loop {
            let packet = match reader.next_packet()? {
                Some(packet) => packet,
                None => return Ok(()),
            };
            let Some(v) = video.as_ref() else {
                return Ok(());
            };
            if packet.track_id != v.track_id {
                continue;
            }
            if attempt <= 0.0 || v.pipeline.is_keyframe(&packet.data) {
                let has_audio = audio.is_some();
                route_video_packet(video, &packet, has_audio);
                return Ok(());
            }
            attempt -= KEYFRAME_SCAN_STEP;
            break;
        }
    }
}

/// Hands a video packet to the video thread. With audio, a full channel means
/// video is lagging: drop until the next IDR fits, audio stays smooth. Without
/// audio the blocking send IS the demux pacing.
fn route_video_packet(
    video: &mut Option<VideoPipe>,
    packet: &symphonia::core::packet::Packet,
    has_audio: bool,
) {
    let Some(v) = video else {
        return;
    };
    if v.track_id != packet.track_id {
        return;
    }
    // MPEG's conventional timescale as the no-timebase fallback.
    let pts = packet_secs(&packet.pts, v.time_base, 90_000.0);
    let accepted = v
        .pipeline
        .send_sample(pts, &packet.data, !has_audio, v.lagging);
    if has_audio {
        if accepted == v.lagging {
            log::debug!(
                "video: {} at {pts:.3}s",
                if accepted { "resynced" } else { "lagging" }
            );
        }
        v.lagging = !accepted;
    }
}

fn packet_secs(pts: &Timestamp, time_base: Option<TimeBase>, fallback_hz: f64) -> f64 {
    match time_base {
        Some(tb) => tb.calc_time_saturating(*pts).as_secs_f64(),
        // Audio timebases are essentially always 1/rate; video 1/90000.
        None => pts.get() as f64 / fallback_hz,
    }
}

/// Interleaved anything -> interleaved stereo: mono duplicates, wider keeps the
/// front pair.
fn push_stereo(pcm: &mut Pcm, samples: &[f32], channels: usize) {
    match channels {
        1 => {
            for &sample in samples {
                pcm.queue.push_back(sample);
                pcm.queue.push_back(sample);
            }
        }
        2 => pcm.queue.extend(samples.iter().copied()),
        n => {
            for frame in samples.chunks_exact(n) {
                pcm.queue.push_back(frame[0]);
                pcm.queue.push_back(frame[1]);
            }
        }
    }
    pcm.pushed_frames += samples.len() as u64 / channels.max(1) as u64;
}

fn emit_position(shared: &Shared, pcm: &mut Pcm, rate: u32, playing: bool) {
    if !playing {
        return;
    }
    let played = pcm
        .pushed_frames
        .saturating_sub(pcm.queue.len() as u64 / u64::from(CHANNELS));
    let position = pcm.base_secs + played as f64 / f64::from(rate);
    if position - pcm.last_emitted_pos >= POSITION_EVENT_SECONDS {
        pcm.last_emitted_pos = position;
        shared.events.send(PlayerEvent::PositionChanged(position));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::decoder::synth_wav;
    use std::io::{Read, Seek, SeekFrom};
    use std::time::{Duration, Instant};

    const RATE: u32 = 44_100;
    const CLIP_SECONDS: u32 = 2;

    struct Collector(Arc<Mutex<Vec<PlayerEvent>>>);

    impl EventSink for Collector {
        fn send(&self, event: PlayerEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn player(stream_type: StreamType) -> (SdlAudioPlayer, Arc<Mutex<Vec<PlayerEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let player =
            SdlAudioPlayer::with_sink(0, stream_type, Box::new(Collector(events.clone())), None);
        (player, events)
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn has(events: &Mutex<Vec<PlayerEvent>>, pred: impl Fn(&PlayerEvent) -> bool) -> bool {
        events.lock().unwrap().iter().any(pred)
    }

    /// A player whose decoder thread has already failed out on junk input, so
    /// nothing consumes the byte buffer while a test inspects it.
    fn dead_player(stream_type: StreamType) -> SdlAudioPlayer {
        let (player, events) = player_pair_for(stream_type);
        player.push_data(b"not audio at all".to_vec()).unwrap();
        player.end_of_stream().unwrap();
        wait_for("probe failure", || {
            has(&events, |e| matches!(e, PlayerEvent::Error(_)))
        });
        lock(&player.shared.stream).eos = false;
        player
    }

    fn player_pair_for(stream_type: StreamType) -> (SdlAudioPlayer, Arc<Mutex<Vec<PlayerEvent>>>) {
        player(stream_type)
    }

    #[test]
    fn can_play_type_maps_decoder_coverage() {
        let cases = [
            ("audio/mpeg", SupportsMediaType::Probably),
            ("audio/wav", SupportsMediaType::Probably),
            ("audio/flac", SupportsMediaType::Probably),
            ("audio/aac", SupportsMediaType::Probably),
            ("audio/ogg", SupportsMediaType::Maybe),
            ("audio/ogg; codecs=vorbis", SupportsMediaType::Probably),
            ("audio/ogg; codecs=opus", SupportsMediaType::No),
            ("audio/mp4", SupportsMediaType::Maybe),
            (
                "audio/mp4; codecs=\"mp4a.40.2\"",
                SupportsMediaType::Probably,
            ),
            ("audio/mp4; codecs=alac", SupportsMediaType::No),
            ("AUDIO/MPEG", SupportsMediaType::Probably),
            ("video/mp4", SupportsMediaType::Maybe),
            (
                "video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\"",
                SupportsMediaType::Probably,
            ),
            ("video/mp4; codecs=\"hvc1.1.6\"", SupportsMediaType::No),
            ("video/webm", SupportsMediaType::No),
            ("audio/webm", SupportsMediaType::No),
            ("", SupportsMediaType::No),
        ];
        for (mime, expected) in cases {
            assert_eq!(can_play_type(mime, true), expected, "for {mime:?}");
        }
        // The [video] switch takes video/* back to No; audio is unaffected.
        assert_eq!(can_play_type("video/mp4", false), SupportsMediaType::No);
        assert_eq!(
            can_play_type("audio/mpeg", false),
            SupportsMediaType::Probably
        );
    }

    /// The whole startup choreography: NeedData first (it unlocks the element's
    /// data source), then metadata, then the preroll Paused that makes the
    /// element playable.
    #[test]
    fn announces_metadata_then_preroll() {
        let (player, events) = player(StreamType::Seekable);
        player.set_seekable(true).unwrap();
        let wav = synth_wav(RATE, 2, RATE * CLIP_SECONDS);
        player.set_input_size(wav.len() as u64).unwrap();
        for chunk in wav.chunks(64 * 1024) {
            player.push_data(chunk.to_vec()).unwrap();
        }
        player.end_of_stream().unwrap();

        wait_for("preroll state", || {
            has(&events, |e| {
                matches!(e, PlayerEvent::StateChanged(PlaybackState::Paused))
            })
        });
        let events = events.lock().unwrap();
        assert!(
            matches!(events.first(), Some(PlayerEvent::NeedData)),
            "NeedData must come before anything else"
        );
        let metadata = events
            .iter()
            .find_map(|e| match e {
                PlayerEvent::MetadataUpdated(m) => Some(m.clone()),
                _ => None,
            })
            .expect("metadata was announced");
        let duration = metadata
            .duration
            .expect("wav length is known")
            .as_secs_f64();
        assert!(
            (duration - f64::from(CLIP_SECONDS)).abs() < 0.05,
            "duration {duration}"
        );
        assert!(metadata.is_seekable);
        assert_eq!(metadata.audio_tracks.len(), 1);
        assert!(metadata.video_tracks.is_empty());
        drop(events);

        let buffered = player.buffered();
        assert_eq!(buffered.len(), 1);
        assert!(buffered[0].start.abs() < 0.01);
        assert!((buffered[0].end - f64::from(CLIP_SECONDS)).abs() < 0.05);
        assert_eq!(player.seekable(), buffered);
    }

    /// Playing decodes the clip into the queue; draining it (as the device
    /// would) advances the reported position and ends with one EndOfStream.
    #[test]
    fn plays_to_the_end() {
        let (player, events) = player(StreamType::Seekable);
        let wav = synth_wav(RATE, 2, RATE * CLIP_SECONDS);
        player.set_input_size(wav.len() as u64).unwrap();
        player.push_data(wav).unwrap();
        player.end_of_stream().unwrap();
        wait_for("preroll", || {
            has(&events, |e| {
                matches!(e, PlayerEvent::StateChanged(PlaybackState::Paused))
            })
        });

        assert!(player.paused());
        player.play().unwrap();
        assert!(!player.paused());

        // No SDL in tests: stand in for the device by draining the queue.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !has(&events, |e| matches!(e, PlayerEvent::EndOfStream)) {
            assert!(Instant::now() < deadline, "never reached end of stream");
            {
                let mut pcm = lock(&player.shared.pcm);
                let tenth_second = (RATE as usize / 10) * CHANNELS as usize;
                let n = pcm.queue.len().min(tenth_second);
                pcm.queue.drain(..n);
            }
            player.shared.work_cv.notify_all();
            thread::sleep(Duration::from_millis(2));
        }

        let events = events.lock().unwrap();
        let positions: Vec<f64> = events
            .iter()
            .filter_map(|e| match e {
                PlayerEvent::PositionChanged(p) => Some(*p),
                _ => None,
            })
            .collect();
        assert!(positions.len() >= 2, "expected progress, got {positions:?}");
        assert!(positions.windows(2).all(|w| w[0] <= w[1]));
        assert!(*positions.last().unwrap() > 1.0);
        let ends = events
            .iter()
            .filter(|e| matches!(e, PlayerEvent::EndOfStream))
            .count();
        assert_eq!(ends, 1);
    }

    #[test]
    fn seek_reports_done_and_validates() {
        let (player, events) = player(StreamType::Seekable);
        let wav = synth_wav(RATE, 2, RATE * CLIP_SECONDS);
        player.set_input_size(wav.len() as u64).unwrap();
        player.push_data(wav).unwrap();
        player.end_of_stream().unwrap();
        wait_for("preroll", || {
            has(&events, |e| {
                matches!(e, PlayerEvent::StateChanged(PlaybackState::Paused))
            })
        });

        assert_eq!(player.seek(10.0), Err(PlayerError::SeekOutOfRange));
        player.seek(0.5).unwrap();
        wait_for("seek done", || {
            has(
                &events,
                |e| matches!(e, PlayerEvent::SeekDone(p) if (p - 0.5).abs() < 1e-9),
            )
        });

        let (stream_player, _) = player_pair_for(StreamType::Stream);
        assert_eq!(stream_player.seek(0.1), Err(PlayerError::NonSeekableStream));
    }

    /// Seekable input applies backpressure past the cap — the element cancels
    /// the fetch on this error and the refetch protocol takes over.
    #[test]
    fn push_backpressure_on_seekable_input() {
        let player = dead_player(StreamType::Seekable);
        let already = lock(&player.shared.stream).data.len();
        player
            .push_data(vec![0; BYTE_HIGH_WATER - already])
            .unwrap();
        assert_eq!(player.push_data(vec![0]), Err(PlayerError::EnoughData));
        assert!(lock(&player.shared.stream).stalled);
    }

    /// A live stream has no refetch, so the oldest bytes are shed instead.
    #[test]
    fn push_evicts_on_live_input() {
        let player = dead_player(StreamType::Stream);
        let already = lock(&player.shared.stream).data.len();
        player
            .push_data(vec![0; BYTE_HIGH_WATER - already])
            .unwrap();
        player.push_data(vec![0; 1024]).unwrap();
        let stream = lock(&player.shared.stream);
        assert_eq!(stream.data.len(), BYTE_HIGH_WATER);
        assert_eq!(stream.base, 1024);
        assert!(!stream.stalled);
    }

    /// The refetch handshake end to end: a read outside the buffered region
    /// emits SeekData, waits for the unlock, re-emits NeedData for the new
    /// fetch context, and serves the bytes that then arrive.
    #[test]
    fn jump_read_runs_the_seek_data_handshake() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::new(Shared::new(Box::new(Collector(events.clone()))));
        lock(&shared.stream).total_len = Some(1000);

        let reader_shared = shared.clone();
        let reader = thread::spawn(move || {
            let mut reader = ByteReader {
                shared: reader_shared,
                seekable: true,
            };
            reader.seek(SeekFrom::Start(500)).unwrap();
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf).unwrap();
            buf
        });

        wait_for("SeekData", || {
            has(&events, |e| matches!(e, PlayerEvent::SeekData(500, _)))
        });
        let seek_lock = events
            .lock()
            .unwrap()
            .iter()
            .find_map(|e| match e {
                PlayerEvent::SeekData(_, lock) => Some(lock.clone()),
                _ => None,
            })
            .expect("just waited for it");
        // Blocks until the reader acks, exactly like the element's fetch_request.
        seek_lock.unlock(true);
        wait_for("NeedData after the handshake", || {
            let events = events.lock().unwrap();
            matches!(events.last(), Some(PlayerEvent::NeedData))
        });

        {
            let mut stream = lock(&shared.stream);
            assert_eq!(stream.base, 500, "the region reset to the jump target");
            stream.data.extend_from_slice(&[1, 2, 3, 4]);
        }
        shared.bytes_cv.notify_all();
        assert_eq!(reader.join().unwrap(), [1, 2, 3, 4]);
    }
}
