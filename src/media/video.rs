//! `<video>`: H.264 decode (openh264) and frame presentation for [`super::player`].
//!
//! The demux thread routes video packets here over a bounded channel; this
//! module's thread decodes, reorders by PTS (openh264 outputs in decode order,
//! so B-frames arrive out of presentation order) and presents against the
//! player's clock. Frames are kept as YUV and converted to the BGRA8 WebRender
//! wants only when actually shown.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use openh264::decoder::{DecodedYUV, Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;
use openh264::OpenH264API;
use servo_media::player::video::{Buffer, VideoFrame, VideoFrameData, VideoFrameRenderer};
use servo_media::player::PlayerEvent;
use yuv::{yuv420_to_bgra, YuvPlanarImage, YuvRange, YuvStandardMatrix};

use super::player::Shared;

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
    /// `extra_data` is the container's AVCDecoderConfigurationRecord.
    pub(crate) fn spawn(
        shared: Arc<Shared>,
        renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
        extra_data: &[u8],
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
            .spawn(move || video_thread(shared, renderer, rx, flush_, done_, headers));
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
struct Frame {
    pts_us: i64,
    width: usize,
    height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

impl Frame {
    fn from_decoded(yuv: &DecodedYUV, pts: f64) -> Self {
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
            pts_us: (pts * 1e6) as i64,
            width,
            height,
            y: tight(yuv.y(), sy, width, height),
            u: tight(yuv.u(), su, cw, ch),
            v: tight(yuv.v(), sv, cw, ch),
        }
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
    renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
    heap: BinaryHeap<Reverse<Frame>>,
    /// Post-seek: drop frames before this time.
    discard_until: Option<f64>,
    /// The first frame after start/seek shows immediately, clock regardless.
    presented_any: bool,
    last_position: f64,
    bgra: Vec<u8>,
}

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
            self.show(frame);
        }
    }

    fn show(&mut self, frame: Frame) {
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
        if let Some(video_frame) = self.convert(&frame) {
            self.renderer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .render(video_frame);
            self.presented_any = true;
            self.shared.send_event(PlayerEvent::VideoFrameUpdated);
            // Position events come from audio when there is any.
            if !self.shared.has_audio() && pts - self.last_position >= 0.25 {
                self.last_position = pts;
                self.shared.send_event(PlayerEvent::PositionChanged(pts));
            }
        }
    }

    fn convert(&mut self, frame: &Frame) -> Option<VideoFrame> {
        let (w, h) = (frame.width, frame.height);
        let planar = YuvPlanarImage {
            y_plane: &frame.y,
            y_stride: w as u32,
            u_plane: &frame.u,
            u_stride: w.div_ceil(2) as u32,
            v_plane: &frame.v,
            v_stride: w.div_ceil(2) as u32,
            width: w as u32,
            height: h as u32,
        };
        let matrix = if h >= 720 {
            YuvStandardMatrix::Bt709
        } else {
            YuvStandardMatrix::Bt601
        };
        self.bgra.clear();
        self.bgra.resize(w * h * 4, 0);
        if let Err(e) = yuv420_to_bgra(
            &planar,
            &mut self.bgra,
            (w * 4) as u32,
            YuvRange::Limited,
            matrix,
        ) {
            log::debug!("video: conversion failed: {e}");
            return None;
        }
        let data = Arc::new(std::mem::take(&mut self.bgra));
        VideoFrame::new(w as i32, h as i32, Arc::new(BgraBuffer(data)))
    }
}

fn video_thread(
    shared: Arc<Shared>,
    renderer: Arc<Mutex<dyn VideoFrameRenderer>>,
    rx: Receiver<VideoMsg>,
    flush: Arc<FlushPoint>,
    done: Arc<AtomicBool>,
    headers: Vec<u8>,
) {
    let mut decoder: Option<Decoder> = None;
    let mut epoch = 0u64;
    // openh264 emits pictures in display order with a reorder delay, so each
    // output gets the smallest PTS still outstanding, not its packet's.
    let mut pending_pts: BinaryHeap<Reverse<i64>> = BinaryHeap::new();
    let mut presenter = Presenter {
        shared: shared.clone(),
        renderer,
        heap: BinaryHeap::new(),
        discard_until: None,
        presented_any: false,
        last_position: f64::NEG_INFINITY,
        bgra: Vec::new(),
    };

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
            decoder = None;
            pending_pts.clear();
            presenter.reset(flush.target());
            done.store(false, Ordering::SeqCst);
        }

        match msg {
            VideoMsg::Packet {
                pts,
                annexb,
                keyframe,
                ..
            } => {
                log::trace!("video: packet {pts:.3}s keyframe={keyframe}");
                if decoder.is_none() {
                    if !keyframe {
                        continue;
                    }
                    // Mid-stream flushing corrupts B-frame reordering; frames
                    // are pulled out only by later packets and flush_remaining.
                    let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
                    match Decoder::with_api_config(OpenH264API::from_source(), config) {
                        Ok(mut fresh) => {
                            let _ = fresh.decode(&headers);
                            decoder = Some(fresh);
                        }
                        Err(e) => {
                            log::warn!("video: decoder init failed: {e}");
                            break;
                        }
                    }
                }
                let Some(active) = decoder.as_mut() else {
                    continue;
                };
                pending_pts.push(Reverse((pts * 1e6) as i64));
                match active.decode(&annexb) {
                    Ok(Some(yuv)) => {
                        let stamp = pending_pts.pop().map_or(pts, |Reverse(us)| us as f64 / 1e6);
                        presenter
                            .heap
                            .push(Reverse(Frame::from_decoded(&yuv, stamp)));
                    }
                    Ok(None) => {}
                    // Corrupt bitstream: rebuild at the next keyframe.
                    Err(e) => {
                        log::debug!("video: decode failed: {e}");
                        decoder = None;
                        pending_pts.clear();
                    }
                }
                presenter.present_due(epoch, &flush, false);
            }
            VideoMsg::Eos { .. } => {
                if let Some(active) = decoder.as_mut() {
                    if let Ok(frames) = active.flush_remaining() {
                        let mut last = 0.0;
                        for yuv in frames {
                            let stamp = pending_pts
                                .pop()
                                .map_or(last + 1.0 / 60.0, |Reverse(us)| us as f64 / 1e6);
                            last = stamp;
                            presenter
                                .heap
                                .push(Reverse(Frame::from_decoded(&yuv, stamp)));
                        }
                    }
                }
                decoder = None;
                pending_pts.clear();
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
