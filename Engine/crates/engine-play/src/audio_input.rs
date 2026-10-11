//! Native microphone capture never takes scene or scripting locks.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct InputBuffer {
    samples: VecDeque<f32>,
    dropped: usize,
    error: Option<String>,
}

pub(crate) struct AudioInput {
    buffer: Arc<Mutex<InputBuffer>>,
    pub sample_rate: u32,
    pub channels: u16,
    #[cfg(windows)]
    stop: std::sync::mpsc::Sender<()>,
    #[cfg(windows)]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl AudioInput {
    pub fn open() -> Result<Self, String> {
        #[cfg(not(windows))]
        return Err("microphone capture is currently supported on Windows only".into());
        #[cfg(windows)]
        {
            use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
            let buffer = Arc::new(Mutex::new(InputBuffer::default()));
            let shared = Arc::clone(&buffer);
            let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
            let (stop, stopped) = std::sync::mpsc::channel();
            let worker = std::thread::Builder::new()
                .name("rustic-microphone".into())
                .spawn(move || {
                    let open = || -> Result<(cpal::Stream, u32, u16), String> {
                        let device = cpal::default_host()
                            .default_input_device()
                            .ok_or("no default microphone is available")?;
                        let supported = device.default_input_config().map_err(|e| e.to_string())?;
                        let config: cpal::StreamConfig = supported.clone().into();
                        let stream = match supported.sample_format() {
                            cpal::SampleFormat::F32 => build::<f32>(&device, &config, &shared),
                            cpal::SampleFormat::I16 => build::<i16>(&device, &config, &shared),
                            cpal::SampleFormat::U16 => build::<u16>(&device, &config, &shared),
                            format => {
                                Err(format!("unsupported microphone sample format: {format:?}"))
                            }
                        }?;
                        stream.play().map_err(|e| e.to_string())?;
                        Ok((stream, config.sample_rate.0, config.channels))
                    };
                    match open() {
                        Ok((stream, rate, channels)) => {
                            let _ = ready_tx.send(Ok((rate, channels)));
                            let _ = stopped.recv();
                            drop(stream);
                        }
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            let (sample_rate, channels) = ready_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|e| format!("microphone startup failed: {e}"))??;
            Ok(Self {
                buffer,
                sample_rate,
                channels,
                stop,
                worker: Some(worker),
            })
        }
    }

    #[cfg_attr(
        not(windows),
        allow(
            clippy::unused_self,
            reason = "only the Windows backend owns a native capture worker"
        )
    )]
    pub fn close(&mut self) {
        #[cfg(windows)]
        {
            let _ = self.stop.send(());
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    pub fn read(&self, limit: usize) -> Result<(Vec<f32>, usize), String> {
        let mut buffer = self
            .buffer
            .lock()
            .map_err(|_| "microphone buffer lock poisoned")?;
        if let Some(error) = &buffer.error {
            return Err(error.clone());
        }
        let count = limit.min(buffer.samples.len());
        let count = count - count % usize::from(self.channels);
        let samples = buffer.samples.drain(..count).collect();
        let dropped = std::mem::take(&mut buffer.dropped);
        Ok((samples, dropped))
    }
}

#[cfg(windows)]
impl Drop for AudioInput {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(windows)]
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: &Arc<Mutex<InputBuffer>>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::traits::DeviceTrait;
    let data = Arc::clone(buffer);
    let errors = Arc::clone(buffer);
    let capacity = config.sample_rate.0 as usize * usize::from(config.channels) * 2;
    device
        .build_input_stream(
            config,
            move |samples: &[T], _| {
                // Bound the queue to two seconds; a slow script gets recent input.
                if let Ok(mut buffer) = data.lock() {
                    for sample in samples {
                        if buffer.samples.len() == capacity {
                            buffer.samples.pop_front();
                            buffer.dropped += 1;
                        }
                        let value = sample.to_sample::<f32>();
                        buffer.samples.push_back(if value.is_finite() {
                            value.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        });
                    }
                }
            },
            move |error| {
                if let Ok(mut buffer) = errors.lock() {
                    buffer.error = Some(error.to_string());
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
impl AudioInput {
    pub fn fixture(samples: Vec<f32>, channels: u16, dropped: usize) -> Self {
        #[cfg(windows)]
        let (stop, _) = std::sync::mpsc::channel();
        Self {
            buffer: Arc::new(Mutex::new(InputBuffer {
                samples: samples.into(),
                dropped,
                error: None,
            })),
            sample_rate: 48_000,
            channels,
            #[cfg(windows)]
            stop,
            #[cfg(windows)]
            worker: None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_preserve_channel_frames_and_report_dropped_input_once() {
        let input = AudioInput::fixture(vec![0.25; 12], 3, 6);
        let (samples, dropped) = input.read(8).unwrap();
        assert_eq!(samples.len(), 6);
        assert_eq!(dropped, 6);
        assert_eq!(input.read(usize::MAX).unwrap(), (vec![0.25; 6], 0));
        assert!(input.read(4096).unwrap().0.is_empty());
        input.buffer.lock().unwrap().error = Some("device disconnected".into());
        assert_eq!(input.read(4096).unwrap_err(), "device disconnected");
    }
}
