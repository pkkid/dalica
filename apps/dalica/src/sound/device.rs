//! Windows' and macOS' sound output (M15.27): `cpal` without its default features, so only its
//! WASAPI host (the `windows` crate) and its CoreAudio host (`objc2-core-audio`), which keep those
//! APIs' `unsafe` calls out of this workspace.
//!
//! A stream plays on the default output device at the device's own format (shared mode asks for
//! it): our samples are resampled to its rate (linear interpolation) and mapped to its channels
//! (mono doubled, stereo into the first two, more mixed down) in the device's callback, which
//! takes them from the queue as it needs them; a callback that finds the queue empty plays
//! silence and counts nothing as heard. What the callbacks consumed is what has been heard. The
//! device going away (unplugged, or the stream invalidated) fails the stream, and the output
//! opens the default device again ([`super::RETRIES`]); a change of default the host follows by
//! itself.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, Sample, SampleFormat, SizedSample};
use dalica_engine::sound::{Format, NO_OUTPUT};

use super::{Backend, Core, downmix};

/// The default output device of the default host.
pub struct Device;

/// How often the output's thread looks whether the stream is still the one open.
const LOOK: Duration = Duration::from_millis(10);

impl Backend for Device {
    fn probe(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| NO_OUTPUT.to_string())?;
        device.default_output_config().map(|_| ()).map_err(|_| NO_OUTPUT.to_string())
    }

    fn play(&mut self, core: &Arc<Core>, stream: u64, format: Format) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| NO_OUTPUT.to_string())?;
        let supported = device.default_output_config().map_err(|e| format!("the output device: {e}"))?;
        let config = supported.config();
        let failed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let fill = Fill::new(Arc::clone(core), stream, format, config.sample_rate, config.channels);
        let out = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, fill, &failed),
            SampleFormat::I16 => build::<i16>(&device, config, fill, &failed),
            SampleFormat::U16 => build::<u16>(&device, config, fill, &failed),
            SampleFormat::I32 => build::<i32>(&device, config, fill, &failed),
            other => return Err(format!("the output device takes {other:?} samples, which Dalica does not write")),
        }?;
        out.play().map_err(|e| format!("the output device: {e}"))?;
        loop {
            std::thread::sleep(LOOK);
            if let Some(why) = failed.lock().unwrap_or_else(PoisonError::into_inner).take() {
                return Err(why);
            }
            if !core.is_current(stream, format) {
                return Ok(());
            }
        }
    }
}

/// Builds the device's stream of `T` samples, filled by `fill`; a device lost or a stream
/// invalidated is put in `failed`.
fn build<T: SizedSample + FromSample<f32> + Send + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut fill: Fill,
    failed: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String> {
    let failed = Arc::clone(failed);
    let data = move |out: &mut [T], _: &cpal::OutputCallbackInfo| fill.fill(out);
    let error = move |e: cpal::Error| match e.kind() {
        ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::HostUnavailable => {
            *failed.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("the output device: {e}"));
        }
        _ => log::debug!("the output device: {e}"),
    };
    device.build_output_stream::<T, _, _>(config, data, error, None).map_err(|e| format!("the output device: {e}"))
}

/// The device's callback side: the queue's samples resampled and mapped to the device.
struct Fill {
    core: Arc<Core>,
    stream: u64,
    /// The stream's rate and channels.
    rate: u32,
    channels: u16,
    /// The device's.
    device_rate: u32,
    device_channels: u16,
    /// Source frames (stereo after the mix-down) taken and not yet passed.
    frames: VecDeque<[f32; 2]>,
    /// Where between the first two of `frames` the next device frame falls (0 ..= 1).
    at: f64,
    /// Source frames passed since the stream opened: what has been heard.
    passed: u64,
}

impl Fill {
    fn new(core: Arc<Core>, stream: u64, format: Format, device_rate: u32, device_channels: u16) -> Fill {
        Fill {
            core,
            stream,
            rate: format.rate,
            channels: format.channels,
            device_rate: device_rate.max(1),
            device_channels: device_channels.max(1),
            frames: VecDeque::new(),
            at: 0.0,
            passed: 0,
        }
    }

    /// Takes source frames until at least `need` wait.
    fn refill(&mut self, need: usize) {
        if self.frames.len() >= need {
            return;
        }
        let Some(samples) = self.core.take(self.stream, need.saturating_sub(self.frames.len()).max(256)) else { return };
        let stereo = downmix(&samples, self.channels, 2);
        self.frames.extend(stereo.as_chunks::<2>().0.iter().copied());
    }

    fn fill<T: Sample + FromSample<f32>>(&mut self, out: &mut [T]) {
        let step = f64::from(self.rate) / f64::from(self.device_rate);
        let channels = usize::from(self.device_channels);
        let frames = out.len() / channels;
        self.refill(((frames as f64 * step).ceil() as usize).saturating_add(2));
        for frame in out.chunks_mut(channels) {
            // The last frame waiting plays as it is (its successor, if one comes, is not here yet).
            let Some(a) = self.frames.front().copied() else {
                frame.iter_mut().for_each(|v| *v = <T as Sample>::from_sample(0.0f32));
                continue;
            };
            let b = self.frames.get(1).copied().unwrap_or(a);
            let t = self.at as f32;
            let lr = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            for (c, v) in frame.iter_mut().enumerate() {
                let s = match (channels, c) {
                    (1, _) => (lr[0] + lr[1]) / 2.0,
                    (_, 0) => lr[0],
                    (_, 1) => lr[1],
                    _ => 0.0,
                };
                *v = <T as Sample>::from_sample(s);
            }
            self.at += step;
            while self.at >= 1.0 && !self.frames.is_empty() {
                self.at -= 1.0;
                self.frames.pop_front();
                self.passed += 1;
            }
        }
        self.core.heard(self.stream, self.passed);
    }
}
