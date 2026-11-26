//! Audio notes: recording the microphone into a WAV attachment, and playing it back.
//!
//! Recordings are 16 kHz mono 16-bit PCM WAV: plenty for speech and about 1.9 MB a
//! minute, so a lesson-long recording stays a manageable attachment.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;

use crate::i18n::t;

/// The sample rate recordings are stored at.
pub const TARGET_RATE: u32 = 16_000;

/// "1:05", or "1:02:05" past an hour.
pub fn format_time(ms: u64) -> String {
    let seconds = ms / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// A 44-byte header for 16-bit mono PCM. The sizes are filled in by `finish_header`.
fn write_header(out: &mut impl Write, rate: u32, data_bytes: u32) -> io::Result<()> {
    out.write_all(b"RIFF")?;
    out.write_all(&data_bytes.saturating_add(36).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&1u16.to_le_bytes())?; // mono
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * 2).to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())
}

/// Fills in the sizes once the length is known. A file left with the placeholder
/// (Folio closed mid-recording) is still readable: see `parse_header`.
fn finish_header(out: &mut (impl Write + Seek), data_bytes: u64) -> io::Result<()> {
    let data = u32::try_from(data_bytes).unwrap_or(u32::MAX);
    out.seek(SeekFrom::Start(4))?;
    out.write_all(&data.saturating_add(36).to_le_bytes())?;
    out.seek(SeekFrom::Start(40))?;
    out.write_all(&data.to_le_bytes())?;
    out.flush()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WavInfo {
    rate: u32,
    channels: u16,
    data_offset: usize,
    data_len: usize,
}

/// Reads a 16-bit PCM header from the start of a file `file_len` bytes long.
fn parse_header(bytes: &[u8], file_len: usize) -> Option<WavInfo> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut format = None;
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let fmt = bytes.get(body..body + 16)?;
            let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
            let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            if tag != 1 || bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            format = Some((rate, channels));
        } else if id == b"data" {
            let (rate, channels) = format?;
            let available = file_len.saturating_sub(body);
            // A recording cut short still has its placeholder size: use what is there.
            let data_len = if size == 0 || size > available {
                available
            } else {
                size
            };
            return Some(WavInfo {
                rate,
                channels,
                data_offset: body,
                data_len,
            });
        }
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    None
}

/// A WAV file's samples, mixed down to mono, and its sample rate.
fn read_samples(path: &Path) -> Result<(u32, Vec<i16>), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let info = parse_header(&bytes, bytes.len())
        .ok_or_else(|| t!("audio-error-not-playable").into_owned())?;
    let data = &bytes[info.data_offset..info.data_offset + info.data_len];
    let samples = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair));
    let channels = usize::from(info.channels);
    if channels == 1 {
        return Ok((info.rate, samples.collect()));
    }
    let all: Vec<i16> = samples.collect();
    let mono = all
        .chunks_exact(channels)
        .map(|frame| (frame.iter().map(|&s| i32::from(s)).sum::<i32>() / channels as i32) as i16)
        .collect();
    Ok((info.rate, mono))
}

/// A WAV file's sample rate and length in frames, from its header alone.
fn wav_length(path: &Path) -> Option<(u32, u64)> {
    let mut file = File::open(path).ok()?;
    let file_len = usize::try_from(file.metadata().ok()?.len()).ok()?;
    let mut head = vec![0; file_len.min(4096)];
    file.read_exact(&mut head).ok()?;
    let info = parse_header(&head, file_len)?;
    Some((
        info.rate,
        (info.data_len / (2 * usize::from(info.channels))) as u64,
    ))
}

/// How long a recording lasts, in milliseconds.
pub fn duration_ms(path: &Path) -> Option<u64> {
    let (rate, frames) = wav_length(path)?;
    Some(frames * 1000 / u64::from(rate.max(1)))
}

/// Brings the microphone's samples to `TARGET_RATE`, averaging the input samples that
/// fall into each output sample (a simple low-pass that keeps speech clear).
struct Downsampler {
    step: f64,
    next: f64,
    taken: u64,
    sum: f32,
    count: u32,
    last: f32,
}

impl Downsampler {
    fn new(from_rate: u32, to_rate: u32) -> Self {
        let step = f64::from(from_rate.max(1)) / f64::from(to_rate.max(1));
        Self {
            step,
            next: step,
            taken: 0,
            sum: 0.0,
            count: 0,
            last: 0.0,
        }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        for &sample in input {
            self.sum += sample;
            self.count += 1;
            self.taken += 1;
            while self.taken as f64 >= self.next {
                if self.count > 0 {
                    self.last = self.sum / self.count as f32;
                    self.sum = 0.0;
                    self.count = 0;
                }
                out.push((self.last.clamp(-1.0, 1.0) * 32767.0).round() as i16);
                self.next += self.step;
            }
        }
    }
}

/// Stream errors worth telling the user about; glitches the stream recovers from
/// (an under-run, a device change) are ignored.
fn keep_error(slot: &Mutex<Option<String>>, error: cpal::Error) {
    if matches!(
        error.kind(),
        cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied
    ) {
        return;
    }
    if let Ok(mut slot) = slot.lock() {
        slot.get_or_insert_with(|| error.to_string());
    }
}

#[derive(Default)]
struct RecordShared {
    paused: AtomicBool,
    error: Mutex<Option<String>>,
}

/// A recording in progress, written straight to its attachment file.
pub struct Recorder {
    pub attachment: String,
    pub page_id: String,
    stream: Option<cpal::Stream>,
    writer: Option<JoinHandle<io::Result<u64>>>,
    shared: Arc<RecordShared>,
    started: Instant,
    paused_for: Duration,
    paused_at: Option<Instant>,
}

/// What a finished recording left behind.
pub struct Recorded {
    pub attachment: String,
    pub page_id: String,
    pub duration_ms: u64,
    pub bytes: u64,
}

impl Recorder {
    /// Starts recording the default microphone into `path` (replacing it).
    // Tests have no microphone and record through `silent` instead.
    #[cfg_attr(test, allow(dead_code))]
    pub fn start(path: &Path, attachment: &str, page_id: &str) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| t!("audio-error-no-microphone").into_owned())?;
        let supported = device
            .default_input_config()
            .map_err(|error| error.to_string())?;
        let config = supported.config();
        let mut file = File::create(path).map_err(|error| error.to_string())?;
        write_header(&mut file, TARGET_RATE, 0).map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::channel();
        let rate = config.sample_rate;
        let writer = std::thread::Builder::new()
            .name("folio-audio".into())
            .spawn(move || write_samples(file, receiver, rate))
            .map_err(|error| error.to_string())?;
        let shared = Arc::new(RecordShared::default());
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                input_stream::<f32>(&device, config, sender, Arc::clone(&shared))
            }
            cpal::SampleFormat::I16 => {
                input_stream::<i16>(&device, config, sender, Arc::clone(&shared))
            }
            cpal::SampleFormat::U16 => {
                input_stream::<u16>(&device, config, sender, Arc::clone(&shared))
            }
            cpal::SampleFormat::I32 => {
                input_stream::<i32>(&device, config, sender, Arc::clone(&shared))
            }
            cpal::SampleFormat::I8 => {
                input_stream::<i8>(&device, config, sender, Arc::clone(&shared))
            }
            cpal::SampleFormat::U8 => {
                input_stream::<u8>(&device, config, sender, Arc::clone(&shared))
            }
            other => {
                return Err(t!("audio-error-mic-format", format = other.to_string()).into_owned())
            }
        }
        .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            attachment: attachment.to_string(),
            page_id: page_id.to_string(),
            stream: Some(stream),
            writer: Some(writer),
            shared,
            started: Instant::now(),
            paused_for: Duration::ZERO,
            paused_at: None,
        })
    }

    /// A recording that hears only silence, for tests (they have no microphone).
    #[cfg(test)]
    pub fn silent(path: &Path, attachment: &str, page_id: &str) -> Self {
        let mut file = File::create(path).unwrap();
        write_header(&mut file, TARGET_RATE, 0).unwrap();
        let (sender, receiver) = mpsc::channel();
        sender.send(vec![0.0; TARGET_RATE as usize / 10]).unwrap();
        drop(sender);
        let writer = std::thread::spawn(move || write_samples(file, receiver, TARGET_RATE));
        Self {
            attachment: attachment.to_string(),
            page_id: page_id.to_string(),
            stream: None,
            writer: Some(writer),
            shared: Arc::default(),
            started: Instant::now(),
            paused_for: Duration::ZERO,
            paused_at: None,
        }
    }

    /// Makes the recording fail as an unplugged microphone does.
    #[cfg(test)]
    pub fn fail(&self, error: &str) {
        *self.shared.error.lock().unwrap() = Some(error.to_string());
    }

    /// Time recorded so far, not counting pauses.
    pub fn elapsed(&self) -> Duration {
        let paused_now = self.paused_at.map_or(Duration::ZERO, |at| at.elapsed());
        self.started
            .elapsed()
            .saturating_sub(self.paused_for)
            .saturating_sub(paused_now)
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(Instant::now()),
            (false, Some(at)) => {
                self.paused_for += at.elapsed();
                self.paused_at = None;
            }
            _ => {}
        }
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// A problem that stopped the recording (the microphone was unplugged…).
    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }

    /// Stops recording and completes the file.
    pub fn stop(mut self) -> Result<Recorded, String> {
        // The stream's callback owns the channel's sender; dropping it ends the writer.
        drop(self.stream.take());
        let samples = self
            .writer
            .take()
            .ok_or_else(|| t!("audio-error-already-stopped").into_owned())?
            .join()
            .map_err(|_| t!("audio-error-stopped-unexpectedly").into_owned())?
            .map_err(|error| error.to_string())?;
        Ok(Recorded {
            attachment: self.attachment.clone(),
            page_id: self.page_id.clone(),
            duration_ms: samples * 1000 / u64::from(TARGET_RATE),
            bytes: 44 + samples * 2,
        })
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sender: mpsc::Sender<Vec<f32>>,
    shared: Arc<RecordShared>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let errors = Arc::clone(&shared);
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if shared.paused.load(Ordering::Relaxed) {
                return;
            }
            let mono = data
                .chunks(channels)
                .map(|frame| {
                    frame
                        .iter()
                        .map(|&sample| f32::from_sample_(sample))
                        .sum::<f32>()
                        / frame.len() as f32
                })
                .collect();
            if sender.send(mono).is_err() {
                if let Ok(mut slot) = shared.error.lock() {
                    slot.get_or_insert_with(|| t!("audio-error-not-saved").into_owned());
                }
            }
        },
        move |error| keep_error(&errors.error, error),
        Some(Duration::from_secs(3)),
    )
}

/// Runs on its own thread: converts what the microphone sends and appends it to the
/// file, then fixes the header when the stream ends. Returns the samples written.
fn write_samples(
    file: File,
    receiver: mpsc::Receiver<Vec<f32>>,
    from_rate: u32,
) -> io::Result<u64> {
    let mut out = BufWriter::new(file);
    let mut downsampler = Downsampler::new(from_rate, TARGET_RATE);
    let mut pcm = Vec::new();
    let mut written = 0u64;
    for chunk in receiver {
        downsampler.push(&chunk, &mut pcm);
        for sample in pcm.drain(..) {
            out.write_all(&sample.to_le_bytes())?;
            written += 1;
        }
    }
    let mut file = out.into_inner().map_err(|error| error.into_error())?;
    finish_header(&mut file, written * 2)?;
    Ok(written)
}

/// `PlayShared::seek` when no seek is waiting.
const NO_SEEK: u64 = u64::MAX;

struct PlayShared {
    /// The clip, loaded on a background thread.
    samples: OnceLock<Vec<i16>>,
    rate: AtomicU32,
    /// Playback position in source samples, as `f64` bits.
    position: AtomicU64,
    /// A position (as `f64` bits) for the audio thread to jump to, or `NO_SEEK`.
    seek: AtomicU64,
    paused: AtomicBool,
    finished: AtomicBool,
    error: Mutex<Option<String>>,
}

impl PlayShared {
    fn new(rate: u32) -> Self {
        Self {
            samples: OnceLock::new(),
            rate: AtomicU32::new(rate),
            position: AtomicU64::new(0f64.to_bits()),
            seek: AtomicU64::new(NO_SEEK),
            paused: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    /// Pauses or resumes; at the end, plays again from the start.
    fn toggle(&self) {
        if self.finished.swap(false, Ordering::Relaxed) {
            self.seek.store(0f64.to_bits(), Ordering::Relaxed);
            self.paused.store(false, Ordering::Relaxed);
        } else {
            self.paused.fetch_xor(true, Ordering::Relaxed);
        }
    }

    /// Jumps to source sample `target` (the audio thread picks it up).
    fn seek_to(&self, target: f64) {
        self.seek.store(target.to_bits(), Ordering::Relaxed);
        self.finished.store(false, Ordering::Relaxed);
    }

    fn is_playing(&self) -> bool {
        !self.paused.load(Ordering::Relaxed) && !self.finished.load(Ordering::Relaxed)
    }
}

/// Plays one recording through the default output device.
pub struct Player {
    pub attachment: String,
    frames: u64,
    shared: Arc<PlayShared>,
    _stream: cpal::Stream,
}

impl Player {
    /// Starts playing `path` from the beginning. Reading the file happens on a
    /// background thread, so long recordings don't freeze the window.
    pub fn start(path: PathBuf, attachment: &str) -> Result<Self, String> {
        let (rate, frames) =
            wav_length(&path).ok_or_else(|| t!("audio-error-unreadable").into_owned())?;
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| t!("audio-error-no-speakers").into_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let config = supported.config();
        let shared = Arc::new(PlayShared::new(rate));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => output_stream::<f32>(&device, config, Arc::clone(&shared)),
            cpal::SampleFormat::I16 => output_stream::<i16>(&device, config, Arc::clone(&shared)),
            cpal::SampleFormat::U16 => output_stream::<u16>(&device, config, Arc::clone(&shared)),
            cpal::SampleFormat::I32 => output_stream::<i32>(&device, config, Arc::clone(&shared)),
            other => {
                return Err(
                    t!("audio-error-speaker-format", format = other.to_string()).into_owned()
                )
            }
        }
        .map_err(|error| error.to_string())?;
        let loader = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("folio-audio-load".into())
            .spawn(move || match read_samples(&path) {
                Ok((rate, samples)) => {
                    loader.rate.store(rate, Ordering::Relaxed);
                    let _ = loader.samples.set(samples);
                }
                Err(error) => {
                    if let Ok(mut slot) = loader.error.lock() {
                        *slot = Some(error);
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            attachment: attachment.to_string(),
            frames,
            shared,
            _stream: stream,
        })
    }

    pub fn is_playing(&self) -> bool {
        self.shared.is_playing()
    }

    /// Pauses or resumes; at the end, plays again from the start.
    pub fn toggle(&self) {
        self.shared.toggle();
    }

    /// Jumps to a point given as a fraction of the clip (0.0 = start).
    pub fn seek(&self, fraction: f32) {
        self.shared
            .seek_to(f64::from(fraction.clamp(0.0, 1.0)) * self.frames as f64);
    }

    pub fn position_ms(&self) -> u64 {
        let pending = self.shared.seek.load(Ordering::Relaxed);
        let bits = if pending == NO_SEEK {
            self.shared.position.load(Ordering::Relaxed)
        } else {
            pending
        };
        let rate = f64::from(self.shared.rate.load(Ordering::Relaxed).max(1));
        ((f64::from_bits(bits) * 1000.0 / rate) as u64).min(self.duration_ms())
    }

    pub fn duration_ms(&self) -> u64 {
        self.frames * 1000 / u64::from(self.shared.rate.load(Ordering::Relaxed).max(1))
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: Arc<PlayShared>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let device_rate = config.sample_rate.max(1);
    let errors = Arc::clone(&shared);
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            fill(data, channels, device_rate, &shared)
        },
        move |error| keep_error(&errors.error, error),
        Some(Duration::from_secs(3)),
    )
}

/// The audio thread's work: the next stretch of the clip, resampled to the device's
/// rate by straight-line interpolation and copied to every channel.
fn fill<T>(data: &mut [T], channels: usize, device_rate: u32, shared: &PlayShared)
where
    T: cpal::SizedSample + FromSample<f32>,
{
    let seek = shared.seek.swap(NO_SEEK, Ordering::Relaxed);
    if seek != NO_SEEK {
        shared.position.store(seek, Ordering::Relaxed);
    }
    let silence = <T as cpal::Sample>::EQUILIBRIUM;
    let Some(samples) = shared.samples.get() else {
        data.fill(silence);
        return;
    };
    if shared.paused.load(Ordering::Relaxed) || shared.finished.load(Ordering::Relaxed) {
        data.fill(silence);
        return;
    }
    let step = f64::from(shared.rate.load(Ordering::Relaxed).max(1)) / f64::from(device_rate);
    let mut position = f64::from_bits(shared.position.load(Ordering::Relaxed));
    for frame in data.chunks_mut(channels) {
        let index = position as usize;
        let value = match (samples.get(index), samples.get(index + 1)) {
            (Some(&a), Some(&b)) => {
                let between = (position - index as f64) as f32;
                (f32::from(a) + (f32::from(b) - f32::from(a)) * between) / 32768.0
            }
            (Some(&a), None) => f32::from(a) / 32768.0,
            _ => {
                shared.finished.store(true, Ordering::Relaxed);
                0.0
            }
        };
        frame.fill(T::from_sample_(value));
        position += step;
    }
    shared.position.store(position.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
pub fn wav_bytes(rate: u32, samples: &[i16]) -> Vec<u8> {
    let mut out = io::Cursor::new(Vec::new());
    write_header(&mut out, rate, 0).unwrap();
    for sample in samples {
        out.write_all(&sample.to_le_bytes()).unwrap();
    }
    finish_header(&mut out, samples.len() as u64 * 2).unwrap();
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_files_round_trip_and_survive_a_missing_length() {
        let samples: Vec<i16> = (0..1600).map(|i| (i * 7) as i16).collect();
        let bytes = wav_bytes(TARGET_RATE, &samples);
        assert_eq!(bytes.len(), 44 + 3200);
        let info = parse_header(&bytes, bytes.len()).unwrap();
        assert_eq!(
            (info.rate, info.channels, info.data_offset, info.data_len),
            (16_000, 1, 44, 3200)
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_samples(&path).unwrap(), (16_000, samples));
        assert_eq!(duration_ms(&path), Some(100));
        // Folio closed mid-recording: the header still says 0 bytes.
        let mut cut = bytes.clone();
        cut[40..44].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse_header(&cut, cut.len()).unwrap().data_len, 3200);
        assert!(parse_header(b"definitely not a wav file..", 27).is_none());
    }

    #[test]
    fn playback_resamples_seeks_finishes_and_starts_again() {
        let shared = PlayShared::new(8_000);
        let mut out = [0f32; 8];
        // Silence until the clip has loaded.
        fill(&mut out, 2, 16_000, &shared);
        assert_eq!(out, [0.0; 8]);
        let samples: Vec<i16> = (0..10).map(|i| i * 1000).collect();
        shared.samples.set(samples).unwrap();
        // An 8 kHz clip on a 16 kHz stereo device: every sample is heard twice as long,
        // halfway values in between, on both channels.
        fill(&mut out, 2, 16_000, &shared);
        let heard: Vec<i32> = out.iter().map(|v| (v * 32768.0).round() as i32).collect();
        assert_eq!(heard, [0, 0, 500, 500, 1000, 1000, 1500, 1500]);
        assert!(shared.is_playing());
        // A jump lands on the next stretch.
        shared.seek_to(6.0);
        fill(&mut out, 2, 16_000, &shared);
        assert_eq!((out[0] * 32768.0).round(), 6000.0);
        // Past the end it falls silent and stops.
        shared.seek_to(9.0);
        let mut long = [1f32; 8];
        fill(&mut long, 1, 8_000, &shared);
        assert_eq!((long[0] * 32768.0).round(), 9000.0);
        assert_eq!(long[1..], [0.0; 7]);
        assert!(!shared.is_playing());
        // Play again starts from the beginning.
        shared.toggle();
        assert!(shared.is_playing());
        fill(&mut out, 1, 8_000, &shared);
        assert_eq!((out[1] * 32768.0).round(), 1000.0);
        // Pause gives silence and keeps the place.
        shared.toggle();
        let before = shared.position.load(Ordering::Relaxed);
        fill(&mut out, 1, 8_000, &shared);
        assert_eq!(out, [0.0; 8]);
        assert_eq!(shared.position.load(Ordering::Relaxed), before);
    }

    #[test]
    fn microphone_audio_is_brought_to_sixteen_kilohertz() {
        let mut out = Vec::new();
        Downsampler::new(48_000, 16_000).push(&[0.5; 4800], &mut out);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|&sample| sample == 16384));
        for rate in [44_100u32, 16_000, 8_000] {
            let mut out = Vec::new();
            let mut downsampler = Downsampler::new(rate, 16_000);
            // Fed in uneven chunks, as a sound card does.
            for chunk in vec![0.0f32; rate as usize].chunks(441) {
                downsampler.push(chunk, &mut out);
            }
            assert!(
                out.len().abs_diff(16_000) <= 1,
                "{rate} Hz gave {}",
                out.len()
            );
        }
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(61_000), "1:01");
        assert_eq!(format_time(3_725_000), "1:02:05");
    }
}
