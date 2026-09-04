use super::source::ByteReader;
use super::*;
use crate::media::decoder::synth_wav;
use crate::media::device::CHANNELS;
use std::io::{Read, Seek, SeekFrom};
use std::thread;
use std::time::{Duration, Instant};

const RATE: u32 = 44_100;
const CLIP_SECONDS: u32 = 2;

struct Collector(Arc<Mutex<Vec<PlayerEvent>>>);

impl EventSink for Collector {
    fn send(&self, event: PlayerEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn new_player(stream_type: StreamType) -> (SdlAudioPlayer, Arc<Mutex<Vec<PlayerEvent>>>) {
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
    let (player, events) = new_player(stream_type);
    player.push_data(b"not audio at all".to_vec()).unwrap();
    player.end_of_stream().unwrap();
    wait_for("probe failure", || {
        has(&events, |e| matches!(e, PlayerEvent::Error(_)))
    });
    lock(&player.shared.stream).eos = false;
    player
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
    let (player, events) = new_player(StreamType::Seekable);
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
    let (player, events) = new_player(StreamType::Seekable);
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
    let (player, events) = new_player(StreamType::Seekable);
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

    let (stream_player, _) = new_player(StreamType::Stream);
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
