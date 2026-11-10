//! The H.264 decoder of one `<video>` stream: the handheld's hardware decoder
//! through libpmhwdec ([`super::hwdec`]) when it takes the stream, openh264
//! otherwise.
//!
//! The hardware is tried at the first key frame. It may refuse the stream then
//! (no hardware, too big, the parameter sets) or at any later access unit
//! (`UNSUPPORTED`); either way software takes over for the rest of the stream.
//! Mid-stream, the access units since the last key frame are kept while the
//! hardware decodes, so openh264 decodes them again and goes on without a gap;
//! the pictures the hardware already gave out are not given out twice.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use openh264::decoder::{Decoder, DecoderConfig, Flush};
use openh264::OpenH264API;

use super::hwdec::{self, HwDecoder, Lib};
use super::video::Frame;

/// The most access-unit bytes kept for a mid-stream hand-off; a longer GOP
/// makes software start at the next key frame instead.
const GOP_BYTES: usize = 16 << 20;

enum Active {
    /// Waiting for a key frame to start a decoder.
    None,
    Hw {
        dec: HwDecoder,
        /// The parameter sets go in front of the next access unit (after a
        /// flush the decoder may have dropped them).
        headers_pending: bool,
    },
    Sw(Decoder),
}

pub(crate) struct H264Decoder {
    /// SPS/PPS, Annex-B.
    headers: Vec<u8>,
    width: u32,
    height: u32,
    /// The hardware; `None` once it refused this stream (or there is none).
    hw: Option<&'static Lib>,
    active: Active,
    /// Skip to the next key frame (after a seek or end of stream).
    need_keyframe: bool,
    /// Both decoders emit pictures in display order with a reorder delay, so
    /// each output gets the smallest PTS still outstanding, not its packet's:
    /// the MP4 demuxer's packet times are in decode order (the hardware hands
    /// back exactly the time it was given with each access unit).
    pending_pts: BinaryHeap<Reverse<i64>>,
    /// On the hardware: the access units since the last key frame.
    gop: Vec<(Vec<u8>, i64)>,
    gop_bytes: usize,
    gop_complete: bool,
    /// The pts of pictures the hardware gave out since that key frame: software
    /// decoding the same units again drops these.
    shown: Vec<i64>,
}

impl H264Decoder {
    /// `width`/`height` from the container (0 if unknown); `hw` is
    /// [`Lib::get`] outside tests.
    pub(crate) fn new(headers: Vec<u8>, width: u32, height: u32, hw: Option<&'static Lib>) -> Self {
        Self {
            headers,
            width,
            height,
            hw,
            active: Active::None,
            need_keyframe: false,
            pending_pts: BinaryHeap::new(),
            gop: Vec::new(),
            gop_bytes: 0,
            gop_complete: false,
            shown: Vec::new(),
        }
    }

    /// The decoder in use: "openh264", a libpmhwdec backend name, or `None`.
    pub(crate) fn backend(&self) -> Option<String> {
        match &self.active {
            Active::None => None,
            Active::Hw { dec, .. } => Some(dec.backend()),
            Active::Sw(_) => Some("openh264".into()),
        }
    }

    /// One access unit (Annex-B). `Err` = no decoder can be built at all.
    pub(crate) fn decode(
        &mut self,
        annexb: &[u8],
        pts_us: i64,
        keyframe: bool,
        out: &mut dyn FnMut(Frame),
    ) -> Result<(), String> {
        if self.need_keyframe {
            if !keyframe {
                return Ok(());
            }
            self.need_keyframe = false;
        }
        if matches!(self.active, Active::None) {
            if !keyframe {
                return Ok(());
            }
            self.start()?;
        }
        if let Active::Hw { .. } = self.active {
            if keyframe {
                self.gop.clear();
                self.gop_bytes = 0;
                self.gop_complete = true;
                self.shown.clear();
            }
            self.keep(annexb, pts_us);
            let Active::Hw {
                dec,
                headers_pending,
            } = &mut self.active
            else {
                unreachable!()
            };
            let with_headers;
            let data = if *headers_pending {
                with_headers = [self.headers.as_slice(), annexb].concat();
                &with_headers
            } else {
                annexb
            };
            let pending = &mut self.pending_pts;
            // A picture the decoder dropped without a word would leave its time
            // behind and push every later one back; no stream holds more than
            // the 16 pictures of H.264's largest DPB in flight.
            if pending.len() >= MAX_PENDING {
                pending.pop();
            }
            pending.push(Reverse(pts_us));
            let shown = &mut self.shown;
            let mut emit = |pic: hwdec::Nv12| out(hw_frame(&pic, pending, shown));
            let r = dec.send(data, pts_us, &mut emit);
            if r == hwdec::UNSUPPORTED {
                return self.to_software(out);
            }
            *headers_pending = false;
            dec.drain(&mut emit);
            if r == hwdec::ERROR {
                // Its time is the newest outstanding, so nothing drained took it.
                log::debug!("video: hardware dropped the unit at {pts_us} us");
                pending.retain(|&Reverse(us)| us != pts_us);
            }
            return Ok(());
        }
        self.decode_sw(annexb, pts_us, out);
        Ok(())
    }

    /// End of stream: every picture still queued, then wait for a key frame.
    pub(crate) fn finish(&mut self, out: &mut dyn FnMut(Frame)) {
        match &mut self.active {
            Active::None => {}
            Active::Hw {
                dec,
                headers_pending,
            } => {
                let (pending, shown) = (&mut self.pending_pts, &mut self.shown);
                let mut emit = |pic: hwdec::Nv12| out(hw_frame(&pic, pending, shown));
                dec.send(&[], 0, &mut emit);
                dec.drain(&mut emit);
                dec.flush();
                *headers_pending = true;
                self.need_keyframe = true;
            }
            Active::Sw(dec) => {
                let mut frames = Vec::new();
                if let Ok(decoded) = dec.flush_remaining() {
                    let mut last = 0;
                    for yuv in decoded {
                        let stamp = self.pending_pts.pop().map_or(last + 16_667, |Reverse(us)| us);
                        last = stamp;
                        frames.push(Frame::from_decoded(&yuv, stamp));
                    }
                }
                self.active = Active::None;
                for frame in frames {
                    self.emit_sw(frame, out);
                }
            }
        }
        self.pending_pts.clear();
        self.forget_gop();
    }

    /// A seek: drop everything queued, start again at the next key frame.
    pub(crate) fn reset(&mut self) {
        match &mut self.active {
            Active::Hw {
                dec,
                headers_pending,
            } => {
                dec.flush();
                *headers_pending = true;
                self.need_keyframe = true;
            }
            _ => self.active = Active::None,
        }
        self.pending_pts.clear();
        self.forget_gop();
        self.shown.clear();
    }

    fn forget_gop(&mut self) {
        self.gop.clear();
        self.gop_bytes = 0;
        self.gop_complete = false;
    }

    fn keep(&mut self, annexb: &[u8], pts_us: i64) {
        if !self.gop_complete {
            return;
        }
        if self.gop_bytes + annexb.len() > GOP_BYTES {
            self.forget_gop();
            return;
        }
        self.gop_bytes += annexb.len();
        self.gop.push((annexb.to_vec(), pts_us));
    }

    /// At a key frame: the hardware if it takes the stream, else openh264.
    fn start(&mut self) -> Result<(), String> {
        if let Some(lib) = self.hw {
            match self.open_hw(lib) {
                Some(dec) => {
                    log::info!(
                        "video: H.264 {}x{} on the hardware decoder ({})",
                        self.width,
                        self.height,
                        dec.backend()
                    );
                    self.active = Active::Hw {
                        dec,
                        headers_pending: false,
                    };
                    return Ok(());
                }
                None => self.hw = None,
            }
        }
        self.active = Active::Sw(self.new_sw()?);
        Ok(())
    }

    fn open_hw(&self, lib: &'static Lib) -> Option<HwDecoder> {
        let Some(backend) = lib.supports(hwdec::CODEC_H264, self.width, self.height) else {
            log::info!("video: no hardware H.264 decoder for {}x{}", self.width, self.height);
            return None;
        };
        let Some(mut dec) = HwDecoder::open(lib, hwdec::CODEC_H264, self.width, self.height)
        else {
            log::info!("video: {backend} would not open {}x{} H.264", self.width, self.height);
            return None;
        };
        // The parameter sets now: a stream the hardware does not take (High 10,
        // interlaced, ...) is refused here, before any picture.
        let mut none = |_: hwdec::Nv12| {};
        if !self.headers.is_empty() && dec.send(&self.headers, 0, &mut none) == hwdec::UNSUPPORTED
        {
            log::info!("video: {backend} refuses this H.264 stream");
            return None;
        }
        Some(dec)
    }

    fn new_sw(&self) -> Result<Decoder, String> {
        // Mid-stream flushing corrupts B-frame reordering; frames are pulled
        // out only by later packets and flush_remaining.
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let mut fresh = Decoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| e.to_string())?;
        let _ = fresh.decode(&self.headers);
        Ok(fresh)
    }

    /// The hardware gave up at the unit just kept: software from the GOP's
    /// key frame, or from the next key frame if the GOP was not kept whole.
    fn to_software(&mut self, out: &mut dyn FnMut(Frame)) -> Result<(), String> {
        if let Active::Hw { mut dec, .. } = std::mem::replace(&mut self.active, Active::None) {
            // The pictures still in its reorder queue.
            let (pending, shown) = (&mut self.pending_pts, &mut self.shown);
            let mut emit = |pic: hwdec::Nv12| out(hw_frame(&pic, pending, shown));
            dec.send(&[], 0, &mut emit);
            dec.drain(&mut emit);
        }
        self.hw = None;
        self.pending_pts.clear();
        log::info!("video: the hardware decoder gave up on the stream; openh264 goes on");
        if !self.gop_complete {
            self.forget_gop();
            self.need_keyframe = true;
            return Ok(());
        }
        self.active = Active::Sw(self.new_sw()?);
        for (unit, pts) in std::mem::take(&mut self.gop) {
            self.decode_sw(&unit, pts, out);
        }
        self.forget_gop();
        Ok(())
    }

    fn decode_sw(&mut self, annexb: &[u8], pts_us: i64, out: &mut dyn FnMut(Frame)) {
        let Active::Sw(dec) = &mut self.active else {
            return;
        };
        self.pending_pts.push(Reverse(pts_us));
        match dec.decode(annexb) {
            Ok(Some(yuv)) => {
                let stamp = self.pending_pts.pop().map_or(pts_us, |Reverse(us)| us);
                let frame = Frame::from_decoded(&yuv, stamp);
                self.emit_sw(frame, out);
            }
            Ok(None) => {}
            // Corrupt bitstream: rebuild at the next keyframe.
            Err(e) => {
                log::debug!("video: decode failed: {e}");
                self.active = Active::None;
                self.pending_pts.clear();
            }
        }
    }

    /// Software pictures the hardware already gave out (after a hand-off) are
    /// dropped once.
    fn emit_sw(&mut self, frame: Frame, out: &mut dyn FnMut(Frame)) {
        if let Some(at) = self.shown.iter().position(|&pts| pts == frame.pts_us) {
            self.shown.swap_remove(at);
            return;
        }
        out(frame);
    }
}

/// The most packet times outstanding on the hardware: H.264's largest DPB.
const MAX_PENDING: usize = 16;

/// A hardware picture, stamped with the smallest outstanding time (see
/// `pending_pts`) and remembered for a hand-off to software.
fn hw_frame(pic: &hwdec::Nv12, pending: &mut BinaryHeap<Reverse<i64>>, shown: &mut Vec<i64>) -> Frame {
    let stamp = pending.pop().map_or(pic.pts, |Reverse(us)| us);
    shown.push(stamp);
    Frame::from_nv12(pic, stamp)
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
