//! `<video>`: H.264 decode ([`super::decode`]: the hardware decoder through
//! libpmhwdec, else openh264) and frame presentation for [`super::player`].
//!
//! The demux thread routes video packets here over a bounded channel; this
//! module's thread decodes, reorders by PTS (B-frames arrive out of
//! presentation order) and presents against the player's clock. Frames are kept as YUV and converted to the BGRA8 WebRender
//! wants only when actually shown.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use openh264::decoder::DecodedYUV;
use openh264::formats::YUVSource;
use servo_media::player::video::{Buffer, VideoFrame, VideoFrameData, VideoFrameRenderer};
use servo_media::player::PlayerEvent;
use yuv::{
    yuv420_to_bgra, yuv_nv12_to_bgra, YuvBiPlanarImage, YuvConversionMode, YuvPlanarImage, YuvRange,
    YuvStandardMatrix,
};

use super::decode::H264Decoder;
use super::glpaint::{GlPainter, Painted};
use super::hwdec::{self, Lib};
use super::shared::POSITION_EVENT_SECONDS;
use super::Shared;

/// Compressed packets buffered ahead of the decoder; the demux thread's send
/// blocking on a full channel is the video-only pacing. Sized to hold a whole
/// GOP so a post-seek catch-up burst never triggers the lag-drop policy.
const CHANNEL_PACKETS: usize = 256;

/// How long a full channel may stall the demuxer before video is declared
/// lagging; the audio side has ~1 s of PCM buffered to ride this out.
const SEND_PATIENCE: Duration = Duration::from_millis(250);

/// Decoded frames held for B-frame reordering and pacing.
const MAX_AHEAD_FRAMES: usize = 8;

/// A frame this far behind the clock is decoded but not shown.
const LATE_SECONDS: f64 = 0.1;

/// Pace-loop sleep bounds; the long one is the paused-clock poll.
const PACE_MIN: Duration = Duration::from_millis(2);
const PACE_MAX: Duration = Duration::from_millis(50);
const PACE_PAUSED: Duration = Duration::from_millis(100);

pub(crate) enum VideoMsg {
    Packet {
        epoch: u64,
        pts: f64,
        annexb: Vec<u8>,
        keyframe: bool,
    },
    Eos {
        epoch: u64,
    },
}

/// Seek marker shared between the demux thread (bumps) and the video thread
/// (drops stale packets, resyncs the decoder).
pub(crate) struct FlushPoint {
    epoch: AtomicU64,
    target_bits: AtomicU64,
}

impl FlushPoint {
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    pub(crate) fn flush_to(&self, target: f64) {
        self.target_bits.store(target.to_bits(), Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    fn target(&self) -> f64 {
        f64::from_bits(self.target_bits.load(Ordering::SeqCst))
    }
}

/// The demux thread's handle to the video thread.
pub(crate) struct VideoPipeline {
    tx: SyncSender<VideoMsg>,
    pub(crate) flush: Arc<FlushPoint>,
    done: Arc<AtomicBool>,
    format: AvccFormat,
}

impl VideoPipeline {
    /// `extra_data` is the container's AVCDecoderConfigurationRecord;
    /// `width`/`height` the track's (0 if unknown).
    pub(crate) fn spawn(
        shared: Arc<Shared>,
        renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
        extra_data: &[u8],
        (width, height): (u32, u32),
    ) -> Option<Self> {
        let format = AvccFormat::parse(extra_data)?;
        let (tx, rx) = mpsc::sync_channel(CHANNEL_PACKETS);
        let flush = Arc::new(FlushPoint {
            epoch: AtomicU64::new(0),
            target_bits: AtomicU64::new(0f64.to_bits()),
        });
        let done = Arc::new(AtomicBool::new(false));

        let headers = format.headers.clone();
        let (flush_, done_) = (flush.clone(), done.clone());
        let spawned = thread::Builder::new()
            .name("video-player".into())
            .spawn(move || {
                let decoder = H264Decoder::new(headers, width, height, Lib::get());
                video_thread(shared, renderer, rx, flush_, done_, decoder)
            });
        if let Err(e) = spawned {
            log::warn!("video: could not spawn the decoder thread: {e}");
            return None;
        }
        Some(Self {
            tx,
            flush,
            done,
            format,
        })
    }

    /// Converts an MP4 sample and queues it. `block` is the video-only pacing
    /// mode; without it a full channel reports `false` so the caller can drop
    /// until the next keyframe (`need_keyframe` re-syncs after such a drop).
    pub(crate) fn send_sample(
        &self,
        pts: f64,
        sample: &[u8],
        block: bool,
        need_keyframe: bool,
    ) -> bool {
        let (annexb, keyframe) = self.format.to_annexb(sample);
        if need_keyframe && !keyframe {
            return false;
        }
        let mut msg = VideoMsg::Packet {
            epoch: self.flush.epoch(),
            pts,
            annexb,
            keyframe,
        };
        if block {
            return self.tx.send(msg).is_ok();
        }
        let deadline = std::time::Instant::now() + SEND_PATIENCE;
        loop {
            match self.tx.try_send(msg) {
                Ok(()) => return true,
                Err(TrySendError::Full(back)) => {
                    if std::time::Instant::now() >= deadline {
                        return false;
                    }
                    msg = back;
                    thread::sleep(PACE_MIN);
                }
                // Thread gone; pretend sent so the demuxer stops caring.
                Err(TrySendError::Disconnected(_)) => return true,
            }
        }
    }

    /// `true` once every frame of the current epoch has been presented.
    pub(crate) fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Whether an MP4 sample carries an IDR; the demuxer's seek scan needs it.
    pub(crate) fn is_keyframe(&self, sample: &[u8]) -> bool {
        self.format.to_annexb(sample).1
    }

    pub(crate) fn send_eos(&self) {
        let _ = self.tx.send(VideoMsg::Eos {
            epoch: self.flush.epoch(),
        });
    }
}

/// AVCC framing: length-prefix size and Annex-B SPS/PPS from avcC.
pub(crate) struct AvccFormat {
    len_size: usize,
    headers: Vec<u8>,
}

const START_CODE: [u8; 4] = [0, 0, 0, 1];

impl AvccFormat {
    /// ISO 14496-15 AVCDecoderConfigurationRecord.
    pub(crate) fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[0] != 1 {
            return None;
        }
        let len_size = (data[4] & 0x03) as usize + 1;
        let mut headers = Vec::new();
        let mut pos = 6;
        let mut copy_units = |count: usize, pos: &mut usize| -> Option<()> {
            for _ in 0..count {
                let len = u16::from_be_bytes([*data.get(*pos)?, *data.get(*pos + 1)?]) as usize;
                *pos += 2;
                let unit = data.get(*pos..*pos + len)?;
                headers.extend_from_slice(&START_CODE);
                headers.extend_from_slice(unit);
                *pos += len;
            }
            Some(())
        };
        copy_units((data[5] & 0x1F) as usize, &mut pos)?;
        let pps_count = *data.get(pos)? as usize;
        pos += 1;
        copy_units(pps_count, &mut pos)?;
        (!headers.is_empty()).then_some(Self { len_size, headers })
    }

    /// Length-prefixed NALs -> start codes; reports whether an IDR is present.
    fn to_annexb(&self, sample: &[u8]) -> (Vec<u8>, bool) {
        let mut out = Vec::with_capacity(sample.len() + 8);
        let mut keyframe = false;
        let mut pos = 0;
        while pos + self.len_size <= sample.len() {
            let mut len = 0usize;
            for &byte in &sample[pos..pos + self.len_size] {
                len = len << 8 | byte as usize;
            }
            pos += self.len_size;
            if len == 0 || pos + len > sample.len() {
                break;
            }
            keyframe |= sample[pos] & 0x1F == 5;
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(&sample[pos..pos + len]);
            pos += len;
        }
        (out, keyframe)
    }
}

/// A decoded frame waiting for its presentation time; tight YUV 420 planes.
pub(crate) struct Frame {
    pub(crate) pts_us: i64,
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) y: Vec<u8>,
    chroma: Chroma,
    /// Where the planes go back when the frame is dropped (hardware frames).
    pool: Option<Arc<PlanePool>>,
}

/// A hardware frame's planes, tightly packed: Y rows, then interleaved UV rows.
pub(crate) struct Nv12Planes<'a> {
    pub(crate) y: &'a [u8],
    pub(crate) uv: &'a [u8],
}

impl Frame {
    /// The planes when the chroma is interleaved (a hardware picture).
    pub(crate) fn nv12(&self) -> Option<Nv12Planes<'_>> {
        match &self.chroma {
            Chroma::Interleaved(uv) => Some(Nv12Planes { y: &self.y, uv }),
            Chroma::Planar { .. } => None,
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.give(std::mem::take(&mut self.y));
            if let Chroma::Interleaved(uv) = &mut self.chroma {
                pool.give(std::mem::take(uv));
            }
        }
    }
}

/// Plane buffers of dropped hardware frames, written again by the next ones: a
/// fresh 1080p copy faulted all of its 3 MB in, page by page (12,000 faults a
/// second, about half of the copy's 14 ms on an H700).
#[derive(Default)]
pub(crate) struct PlanePool(Mutex<Vec<Vec<u8>>>);

/// Free buffers kept: a Y and a UV plane each for a few frames.
const PLANE_POOL: usize = 8;

impl PlanePool {
    /// An empty buffer holding at least `len` bytes: the smallest that fits.
    fn take(&self, len: usize) -> Vec<u8> {
        let mut free = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let fit = free
            .iter()
            .enumerate()
            .filter(|(_, buf)| buf.capacity() >= len)
            .min_by_key(|(_, buf)| buf.capacity())
            .map(|(at, _)| at);
        match fit {
            Some(at) => {
                let mut buf = free.swap_remove(at);
                buf.clear();
                buf
            }
            None => Vec::with_capacity(len),
        }
    }

    fn give(&self, buf: Vec<u8>) {
        let mut free = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if free.len() < PLANE_POOL && buf.capacity() > 0 {
            free.push(buf);
        }
    }
}

/// Each decoder's own chroma layout, kept to the conversion: splitting the
/// hardware's interleaved plane cost 15 ms a 1080p frame on an A53 (RG Cube XX).
enum Chroma {
    /// openh264: U and V planes.
    Planar { u: Vec<u8>, v: Vec<u8> },
    /// The hardware: NV12, U and V interleaved.
    Interleaved(Vec<u8>),
}

impl Frame {
    pub(crate) fn from_decoded(yuv: &DecodedYUV, pts_us: i64) -> Self {
        let (width, height) = yuv.dimensions();
        let (sy, su, sv) = yuv.strides();
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        let tight = |src: &[u8], stride: usize, w: usize, h: usize| {
            let mut out = Vec::with_capacity(w * h);
            for row in 0..h {
                out.extend_from_slice(&src[row * stride..row * stride + w]);
            }
            out
        };
        Self {
            pts_us,
            width,
            height,
            y: tight(yuv.y(), sy, width, height),
            chroma: Chroma::Planar {
                u: tight(yuv.u(), su, cw, ch),
                v: tight(yuv.v(), sv, cw, ch),
            },
            pool: None,
        }
    }

    /// A hardware picture: the planes are read once, row by row (they may be
    /// uncached); the chroma stays interleaved, as the conversion takes it.
    /// `pts_us` as for [`Self::from_decoded`].
    pub(crate) fn from_nv12(pic: &hwdec::Nv12, pts_us: i64, pool: &Arc<PlanePool>) -> Self {
        let (width, height) = (pic.width, pic.height);
        let rows = |plane: &[u8], stride: usize, w: usize, h: usize| {
            let mut out = pool.take(w * h);
            for row in 0..h {
                out.extend_from_slice(&plane[row * stride..row * stride + w]);
            }
            out
        };
        Self {
            pts_us,
            width,
            height,
            y: rows(pic.y, pic.y_stride, width, height),
            chroma: Chroma::Interleaved(rows(
                pic.uv,
                pic.uv_stride,
                width.div_ceil(2) * 2,
                height.div_ceil(2),
            )),
            pool: Some(pool.clone()),
        }
    }

    /// FNV-1a over the planes: trace logs compare decoders picture by picture.
    pub(crate) fn checksum(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut add = |bytes: &mut dyn Iterator<Item = &u8>| {
            for byte in bytes {
                hash = (hash ^ *byte as u64).wrapping_mul(0x100_0000_01b3);
            }
        };
        add(&mut self.y.iter());
        // U then V either way, so both decoders' frames hash alike.
        match &self.chroma {
            Chroma::Planar { u, v } => {
                add(&mut u.iter());
                add(&mut v.iter());
            }
            Chroma::Interleaved(uv) => {
                add(&mut uv.iter().step_by(2));
                add(&mut uv.iter().skip(1).step_by(2));
            }
        }
        hash
    }

    fn pts(&self) -> f64 {
        self.pts_us as f64 / 1e6
    }
}

impl PartialEq for Frame {
    fn eq(&self, other: &Self) -> bool {
        self.pts_us == other.pts_us
    }
}
impl Eq for Frame {}
impl PartialOrd for Frame {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Frame {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pts_us.cmp(&other.pts_us)
    }
}

struct BgraBuffer(Arc<Vec<u8>>);

impl Buffer for BgraBuffer {
    fn to_vec(&self) -> Option<VideoFrameData> {
        Some(VideoFrameData::Raw(self.0.clone()))
    }
}

struct Presenter {
    shared: Arc<Shared>,
    /// Frames due now, to the paint thread.
    paint: SyncSender<Paint>,
    heap: BinaryHeap<Reverse<Frame>>,
    /// Post-seek: drop frames before this time.
    discard_until: Option<f64>,
    /// The first frame after start/seek shows immediately, clock regardless.
    presented_any: bool,
    last_position: f64,
}

/// A frame for the paint thread, with the seek epoch it belongs to.
struct Paint {
    frame: Frame,
    epoch: u64,
}

/// The paint thread: BGRA conversion and the renderer hand-off, overlapping the
/// next frame's decode (1080p on an A53: 23 ms of decode and copy, 15 ms of
/// conversion; one after the other they missed 30 fps).
struct Painter {
    shared: Arc<Shared>,
    renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
    flush: Arc<FlushPoint>,
    /// BGRA buffers handed to the renderer, written again once it lets go: a
    /// fresh 8 MB buffer a 1080p frame meant zeroing it and faulting its pages in.
    bgra: Vec<Arc<Vec<u8>>>,
}

/// Frames waiting for the paint thread: one, so a slow paint holds the decode
/// back instead of piling frames up.
const PAINT_QUEUE: usize = 1;

/// Buffers kept for reuse; more are allocated while all of them are in use.
const BGRA_POOL: usize = 6;

/// Frames the renderer may hold before the next is skipped. WebRender keeps a
/// frame's pixels until its compositor has uploaded them, and that upload is the
/// slowest step at 1080p on an H700 (RG 34XXSP, RG Cube XX): handed 30 frames a
/// second, it fell behind and the process grew by 50 MB a second.
const MAX_HELD: usize = 4;

/// Longest run of skips: a renderer that only lets go of a frame when the next
/// one arrives must not freeze the picture.
const MAX_HELD_WAIT: Duration = Duration::from_millis(250);

enum Pace {
    /// Nothing due yet (or interrupted); go back to the channel.
    Yield,
    Drained,
}

impl Presenter {
    fn reset(&mut self, target: f64) {
        self.heap.clear();
        self.discard_until = Some(target);
        self.presented_any = false;
        self.last_position = f64::NEG_INFINITY;
    }

    /// Presents every frame that is due, sleeping only while over the
    /// reorder/pacing depth (or when draining at EOS).
    fn present_due(&mut self, epoch: u64, flush: &FlushPoint, drain: bool) -> Pace {
        loop {
            let Some(Reverse(frame)) = self.heap.peek() else {
                return Pace::Drained;
            };
            let pts = frame.pts();
            let must_flush = drain || self.heap.len() > MAX_AHEAD_FRAMES || !self.presented_any;
            loop {
                if self.shared.is_quit() || flush.epoch() != epoch || self.shared.seek_pending() {
                    return Pace::Yield;
                }
                let clock = self.shared.clock_secs();
                if pts <= clock || (!self.presented_any && !drain) {
                    break;
                }
                if !must_flush {
                    return Pace::Yield;
                }
                let wait = if self.shared.is_paused() {
                    PACE_PAUSED
                } else {
                    Duration::from_secs_f64(pts - clock).clamp(PACE_MIN, PACE_MAX)
                };
                thread::sleep(wait);
            }
            let Some(Reverse(frame)) = self.heap.pop() else {
                return Pace::Drained;
            };
            self.show(frame, epoch);
        }
    }

    fn show(&mut self, frame: Frame, epoch: u64) {
        let pts = frame.pts();
        if let Some(target) = self.discard_until {
            if pts < target - 1e-3 {
                return;
            }
            self.discard_until = None;
        }
        // Late frames were still decoded (references); just skip the paint.
        if self.presented_any && pts < self.shared.clock_secs() - LATE_SECONDS {
            log::debug!("video: late frame {pts:.3}s skipped");
            return;
        }
        if !self.shared.video_track_enabled() {
            return;
        }
        log::trace!("video: present {pts:.3}s");
        if self.paint.send(Paint { frame, epoch }).is_err() {
            return;
        }
        self.presented_any = true;
        // Position events come from audio when there is any.
        if !self.shared.has_audio() && pts - self.last_position >= POSITION_EVENT_SECONDS {
            self.last_position = pts;
            self.shared.send_event(PlayerEvent::PositionChanged(pts));
        }
    }
}

impl Painter {
    fn run(mut self, rx: Receiver<Paint>) {
        let mut last_sent = Instant::now();
        // The GPU route for hardware frames (see `glpaint`), made on the first
        // one: its GL context lives on this thread.
        let mut gpu: Option<GlPainter> = None;
        let mut gpu_tried = false;
        while let Ok(Paint { frame, epoch }) = rx.recv() {
            // Queued before a seek: the element has moved on.
            if self.shared.is_quit() || self.flush.epoch() != epoch {
                continue;
            }
            let held = self.bgra.iter().filter(|buf| Arc::strong_count(buf) > 1).count();
            if held >= MAX_HELD && last_sent.elapsed() < MAX_HELD_WAIT {
                log::debug!("video: renderer behind, frame {:.3}s skipped", frame.pts());
                continue;
            }
            last_sent = Instant::now();
            if frame.nv12().is_some() && !gpu_tried {
                gpu_tried = true;
                gpu = GlPainter::new();
            }
            if let Some(painter) = gpu.as_mut() {
                match painter.paint(&frame) {
                    Ok(Painted::Frame(video_frame)) => {
                        self.renderer
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .render(video_frame);
                        self.shared.send_event(PlayerEvent::VideoFrameUpdated);
                        continue;
                    }
                    Ok(Painted::Busy) => {
                        log::debug!("video: renderer behind, frame {:.3}s skipped", frame.pts());
                        continue;
                    }
                    Err(e) => {
                        log::warn!("video: GPU painting failed ({e}); converting on the CPU");
                        gpu = None;
                    }
                }
            }
            if let Some(video_frame) = self.convert(&frame) {
                self.renderer
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .render(video_frame);
                self.shared.send_event(PlayerEvent::VideoFrameUpdated);
            }
        }
    }

    fn convert(&mut self, frame: &Frame) -> Option<VideoFrame> {
        let (w, h) = (frame.width, frame.height);
        let matrix = if h >= 720 {
            YuvStandardMatrix::Bt709
        } else {
            YuvStandardMatrix::Bt601
        };
        let size = w * h * 4;
        let free = self
            .bgra
            .iter()
            .position(|buf| buf.len() == size && Arc::strong_count(buf) == 1);
        let mut data = match free {
            Some(at) => self.bgra.swap_remove(at),
            None => Arc::new(vec![0; size]),
        };
        // Ours alone (checked above, or just made): the conversion writes every byte.
        let Some(bgra) = Arc::get_mut(&mut data) else {
            return None;
        };
        let stride = (w * 4) as u32;
        let (cw, range) = (w.div_ceil(2) as u32, YuvRange::Limited);
        let done = match &frame.chroma {
            Chroma::Planar { u, v } => {
                let planar = YuvPlanarImage {
                    y_plane: &frame.y,
                    y_stride: w as u32,
                    u_plane: u,
                    u_stride: cw,
                    v_plane: v,
                    v_stride: cw,
                    width: w as u32,
                    height: h as u32,
                };
                yuv420_to_bgra(&planar, bgra, stride, range, matrix)
            }
            Chroma::Interleaved(uv) => {
                let image = YuvBiPlanarImage {
                    y_plane: &frame.y,
                    y_stride: w as u32,
                    uv_plane: uv,
                    uv_stride: cw * 2,
                    width: w as u32,
                    height: h as u32,
                };
                let mode = YuvConversionMode::Balanced;
                yuv_nv12_to_bgra(&image, bgra, stride, range, matrix, mode)
            }
        };
        if let Err(e) = done {
            log::debug!("video: conversion failed: {e}");
            return None;
        }
        if self.bgra.len() >= BGRA_POOL {
            self.bgra.remove(0);
        }
        self.bgra.push(data.clone());
        VideoFrame::new(w as i32, h as i32, Arc::new(BgraBuffer(data)))
    }
}

fn video_thread(
    shared: Arc<Shared>,
    renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
    rx: Receiver<VideoMsg>,
    flush: Arc<FlushPoint>,
    done: Arc<AtomicBool>,
    mut decoder: H264Decoder,
) {
    let mut epoch = 0u64;
    let (paint, paint_rx) = mpsc::sync_channel(PAINT_QUEUE);
    let painter = Painter {
        shared: shared.clone(),
        renderer,
        flush: flush.clone(),
        bgra: Vec::new(),
    };
    // Without the thread nothing is painted (the sends fail); decoding goes on.
    if let Err(e) = thread::Builder::new()
        .name("video-paint".into())
        .spawn(move || painter.run(paint_rx))
    {
        log::warn!("video: could not spawn the paint thread: {e}");
    }
    let mut presenter = Presenter {
        shared: shared.clone(),
        paint,
        heap: BinaryHeap::new(),
        discard_until: None,
        presented_any: false,
        last_position: f64::NEG_INFINITY,
    };
    let mut backend: Option<String> = None;

    while let Ok(msg) = rx.recv() {
        if shared.is_quit() {
            break;
        }
        let msg_epoch = match &msg {
            VideoMsg::Packet { epoch, .. } | VideoMsg::Eos { epoch } => *epoch,
        };
        if msg_epoch < flush.epoch() {
            continue;
        }
        if msg_epoch > epoch {
            // A seek: resync at the next IDR, drop everything queued.
            epoch = msg_epoch;
            decoder.reset();
            presenter.reset(flush.target());
            done.store(false, Ordering::SeqCst);
        }

        let heap = &mut presenter.heap;
        let mut queue = |frame: Frame| {
            // Its own target, off unless asked for (`video_checksum=trace`):
            // hashing every picture costs real time on a handheld.
            if log::log_enabled!(target: "video_checksum", log::Level::Trace) {
                log::trace!(
                    target: "video_checksum",
                    "video: decoded {:.3}s {:016x}",
                    frame.pts(),
                    frame.checksum()
                );
            }
            heap.push(Reverse(frame));
        };
        match msg {
            VideoMsg::Packet {
                pts,
                annexb,
                keyframe,
                ..
            } => {
                log::trace!("video: packet {pts:.3}s keyframe={keyframe}");
                let pts_us = (pts * 1e6) as i64;
                if let Err(e) = decoder.decode(&annexb, pts_us, keyframe, &mut queue) {
                    log::warn!("video: decoder init failed: {e}");
                    break;
                }
                let now = decoder.backend();
                if now.is_some() && now != backend {
                    log::info!("video: decoding on {}", now.as_deref().unwrap_or("?"));
                    backend = now;
                }
                presenter.present_due(epoch, &flush, false);
            }
            VideoMsg::Eos { .. } => {
                decoder.finish(&mut queue);
                if matches!(presenter.present_due(epoch, &flush, true), Pace::Drained) {
                    done.store(true, Ordering::SeqCst);
                    shared.notify_work();
                }
            }
        }
    }
    // Never leave the demuxer waiting on a dead pipeline.
    done.store(true, Ordering::SeqCst);
    shared.notify_work();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dropped hardware frame hands its planes back, and the next frame of
    /// the same size is copied into them instead of into fresh memory.
    #[test]
    fn dropped_hardware_frames_hand_their_planes_back() {
        let pool = Arc::<PlanePool>::default();
        let frame = Frame {
            pts_us: 0,
            width: 4,
            height: 2,
            y: pool.take(8),
            chroma: Chroma::Interleaved(pool.take(4)),
            pool: Some(pool.clone()),
        };
        let y = frame.y.as_ptr();
        drop(frame);
        assert_eq!(pool.0.lock().unwrap().len(), 2);
        // The smallest buffer that fits: the UV plane's stays for the next UV.
        let again = pool.take(8);
        assert_eq!(again.as_ptr(), y);
        assert!(again.is_empty());
        assert_eq!(pool.take(4).capacity(), 4);
        assert!(pool.0.lock().unwrap().is_empty());
    }

    /// The pool keeps a bounded number of free buffers.
    #[test]
    fn the_plane_pool_is_bounded() {
        let pool = PlanePool::default();
        for _ in 0..PLANE_POOL + 3 {
            pool.give(vec![0; 16]);
        }
        assert_eq!(pool.0.lock().unwrap().len(), PLANE_POOL);
    }

    /// A hardware picture (strided NV12) and the same picture from openh264
    /// (tight planes) hash alike, so the checksum logs of the two compare.
    #[test]
    fn nv12_and_planar_frames_hash_alike() {
        let (w, h, stride) = (6usize, 4usize, 8usize);
        let y: Vec<u8> = (0..stride * h).map(|n| n as u8).collect();
        let uv: Vec<u8> = (0..stride * h / 2).map(|n| 100 + n as u8).collect();
        let pic = hwdec::Nv12 {
            width: w,
            height: h,
            pts: 0,
            y: &y,
            y_stride: stride,
            uv: &uv,
            uv_stride: stride,
        };
        let hw = Frame::from_nv12(&pic, 7, &Arc::default());
        assert_eq!(hw.pts_us, 7);
        let chroma = |skip: usize| -> Vec<u8> {
            (0..h / 2)
                .flat_map(|r| (0..w / 2).map(move |c| (r, c)))
                .map(|(r, c)| uv[r * stride + c * 2 + skip])
                .collect()
        };
        let sw = Frame {
            pts_us: 7,
            width: w,
            height: h,
            y: (0..h).flat_map(|r| y[r * stride..r * stride + w].to_vec()).collect(),
            chroma: Chroma::Planar {
                u: chroma(0),
                v: chroma(1),
            },
            pool: None,
        };
        assert_eq!(hw.y, sw.y);
        assert_eq!(hw.checksum(), sw.checksum());
    }

    /// avcC with one SPS `[0x67, 1, 2]` and one PPS `[0x68, 3]`, 4-byte lengths.
    fn avcc() -> Vec<u8> {
        let mut out = vec![1, 0x42, 0, 30, 0xFF, 0xE1];
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&[0x67, 1, 2]);
        out.push(1);
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&[0x68, 3]);
        out
    }

    #[test]
    fn parses_avcc_headers() {
        let format = AvccFormat::parse(&avcc()).unwrap();
        assert_eq!(format.len_size, 4);
        assert_eq!(
            format.headers,
            [&START_CODE[..], &[0x67, 1, 2], &START_CODE[..], &[0x68, 3]].concat()
        );
        assert!(AvccFormat::parse(&[]).is_none());
        assert!(AvccFormat::parse(&[2, 0, 0, 0, 0xFF, 0xE0, 0]).is_none());
    }

    #[test]
    fn rewrites_lengths_and_flags_idr() {
        let format = AvccFormat::parse(&avcc()).unwrap();
        // One non-IDR NAL (type 1), then an IDR (type 5).
        let mut sample = Vec::new();
        sample.extend_from_slice(&2u32.to_be_bytes());
        sample.extend_from_slice(&[0x41, 0xAA]);
        sample.extend_from_slice(&3u32.to_be_bytes());
        sample.extend_from_slice(&[0x65, 0xBB, 0xCC]);

        let (annexb, keyframe) = format.to_annexb(&sample);
        assert!(keyframe);
        assert_eq!(
            annexb,
            [
                &START_CODE[..],
                &[0x41, 0xAA],
                &START_CODE[..],
                &[0x65, 0xBB, 0xCC]
            ]
            .concat()
        );

        let (only_p, keyframe) = format.to_annexb(&sample[..6]);
        assert!(!keyframe);
        assert_eq!(only_p, [&START_CODE[..], &[0x41, 0xAA]].concat());
    }

    /// Truncated or lying length prefixes must not panic or read past the end.
    #[test]
    fn tolerates_malformed_samples() {
        let format = AvccFormat::parse(&avcc()).unwrap();
        let mut lying = Vec::new();
        lying.extend_from_slice(&100u32.to_be_bytes());
        lying.push(0x65);
        let (out, keyframe) = format.to_annexb(&lying);
        assert!(out.is_empty());
        assert!(!keyframe);
        assert_eq!(format.to_annexb(&[0, 0]).0, Vec::<u8>::new());
    }
}
