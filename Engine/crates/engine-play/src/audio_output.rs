//! Native device handles and streams stay on the audio thread, outside scene locks.
use std::sync::{Arc, Mutex};
pub(crate) type StreamAudio = Arc<Mutex<Vec<f32>>>;
thread_local! {
    static STREAM_AUDIO: std::cell::RefCell<Option<StreamAudio>> = const { std::cell::RefCell::new(None) };
}

// New scene contexts inherit the connection's sink without changing other players.
pub(crate) fn with_stream_audio<T>(sink: &StreamAudio, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<StreamAudio>);
    impl Drop for Restore {
        fn drop(&mut self) {
            STREAM_AUDIO.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(STREAM_AUDIO.with(|slot| slot.replace(Some(Arc::clone(sink)))));
    operation()
}
#[derive(Default)]
pub(crate) struct AudioOutput {
    stream: Option<StreamAudio>,
    #[cfg(windows)]
    sender: Option<std::sync::mpsc::SyncSender<Command>>,
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
enum Command {
    Samples(Vec<f32>),
    Devices {
        input: bool,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
    Select {
        input: bool,
        id: Option<String>,
        reply: std::sync::mpsc::Sender<Result<serde_json::Value, String>>,
    },
}

impl AudioOutput {
    pub fn new() -> Self {
        if let Some(stream) = STREAM_AUDIO.with(|slot| slot.borrow().clone()) {
            return Self {
                stream: Some(stream),
                #[cfg(windows)]
                sender: None,
            };
        }
        // Tests must not initialize native device drivers for each scene fixture.
        #[cfg(all(windows, not(test)))]
        {
            let (sender, receiver) = std::sync::mpsc::sync_channel(8);
            if std::thread::Builder::new()
                .name("rustic-audio".into())
                .spawn(move || native::run(&receiver))
                .is_ok()
            {
                return Self {
                    sender: Some(sender),
                    stream: None,
                };
            }
        }
        Self::default()
    }

    #[allow(clippy::unused_self)]
    #[cfg_attr(not(windows), allow(clippy::unnecessary_wraps))]
    pub fn devices(&self, input: bool) -> Result<serde_json::Value, String> {
        #[cfg(windows)]
        if self.sender.is_some() {
            return self.request(|reply| Command::Devices { input, reply });
        }
        let _ = input;
        Ok(serde_json::json!([]))
    }

    #[allow(clippy::unused_self)]
    pub fn select(&self, input: bool, id: Option<&str>) -> Result<(), String> {
        #[cfg(windows)]
        if self.sender.is_some() {
            self.request(|reply| Command::Select {
                input,
                id: id.map(str::to_owned),
                reply,
            })?;
            return Ok(());
        }
        let _ = (input, id);
        Err("native audio device selection is unavailable on this host".into())
    }

    #[cfg(windows)]
    fn request(
        &self,
        command: impl FnOnce(std::sync::mpsc::Sender<Result<serde_json::Value, String>>) -> Command,
    ) -> Result<serde_json::Value, String> {
        let (reply, response) = std::sync::mpsc::channel();
        self.sender
            .as_ref()
            .ok_or("audio worker unavailable")?
            .try_send(command(reply))
            .map_err(|e| format!("audio worker unavailable or busy: {e}"))?;
        response
            .recv()
            .map_err(|e| format!("audio worker stopped: {e}"))?
    }

    #[allow(clippy::unused_self)]
    pub fn submit(&self, samples: Vec<f32>) {
        if let Some(stream) = &self.stream {
            if let Ok(mut queue) = stream.lock() {
                if queue.len() + samples.len() > 48_000 {
                    queue.clear();
                }
                queue.extend(samples.into_iter().take(48_000));
            }
            return;
        }
        #[cfg(windows)]
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(Command::Samples(samples));
        }
        #[cfg(target_os = "emscripten")]
        BROWSER_AUDIO.with(|queue| {
            let mut queue = queue.borrow_mut();
            // Keep no more than half a second of interleaved stereo audio.
            if queue.len() + samples.len() > 48_000 {
                queue.clear();
            }
            queue.extend(samples.into_iter().take(48_000));
        });
        #[cfg(all(not(windows), not(target_os = "emscripten")))]
        drop(samples);
    }
}

#[cfg(all(windows, not(test)))]
mod native {
    use super::Command;
    use rodio::cpal::{
        Device, Host,
        traits::{DeviceTrait, HostTrait},
    };
    use std::collections::BTreeMap;

    struct NativeAudio {
        host: Host,
        output: Option<(rodio::OutputStream, rodio::Sink)>,
        input: Option<Device>,
        outputs: BTreeMap<String, Device>,
        inputs: BTreeMap<String, Device>,
        serial: u64,
    }

    fn open(device: Device) -> Result<(rodio::OutputStream, rodio::Sink), String> {
        let stream = rodio::OutputStreamBuilder::from_device(device)
            .map_err(|e| e.to_string())?
            .open_stream()
            .map_err(|e| e.to_string())?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        Ok((stream, sink))
    }

    impl NativeAudio {
        fn new() -> Self {
            let host = rodio::cpal::default_host();
            let output = host.default_output_device().and_then(|d| open(d).ok());
            Self {
                host,
                output,
                input: None,
                outputs: BTreeMap::new(),
                inputs: BTreeMap::new(),
                serial: 0,
            }
        }

        fn devices(&mut self, input: bool) -> Result<serde_json::Value, String> {
            let devices = if input {
                self.host.input_devices()
            } else {
                self.host.output_devices()
            }
            .map_err(|e| e.to_string())?;
            let mut cache = BTreeMap::new();
            let mut list = Vec::new();
            for device in devices {
                let name = device.name().map_err(|e| e.to_string())?;
                self.serial += 1;
                let direction = if input { "input" } else { "output" };
                let id = format!("{direction}:{}", self.serial);
                list.push(serde_json::json!({"id":id,"name":name}));
                cache.insert(id, device);
            }
            if input {
                self.inputs = cache;
            } else {
                self.outputs = cache;
            }
            Ok(serde_json::json!(list))
        }

        fn select(&mut self, input: bool, id: Option<&str>) -> Result<serde_json::Value, String> {
            let device = if let Some(id) = id {
                let cache = if input { &self.inputs } else { &self.outputs };
                cache
                    .get(id)
                    .cloned()
                    .ok_or("unknown audio device ID; list devices again")?
            } else {
                (if input {
                    self.host.default_input_device()
                } else {
                    self.host.default_output_device()
                })
                .ok_or("no default audio device available")?
            };
            if input {
                device.default_input_config().map_err(|e| e.to_string())?;
                self.input = Some(device);
            } else {
                // Open first: failure preserves the current stream and voices.
                let replacement = open(device)?;
                self.output = Some(replacement);
            }
            Ok(serde_json::Value::Null)
        }

        fn submit(&self, samples: Vec<f32>) {
            if let Some((_, sink)) = &self.output
                && !samples.is_empty()
                && sink.len() < 4
            {
                sink.append(rodio::buffer::SamplesBuffer::new(2, 48_000, samples));
            }
        }
    }

    pub(super) fn run(receiver: &std::sync::mpsc::Receiver<Command>) {
        let mut audio = NativeAudio::new();
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Samples(samples) => audio.submit(samples),
                Command::Devices { input, reply } => {
                    let _ = reply.send(audio.devices(input));
                }
                Command::Select { input, id, reply } => {
                    let _ = reply.send(audio.select(input, id.as_deref()));
                }
            }
        }
    }
}

#[cfg(target_os = "emscripten")]
thread_local! { static BROWSER_AUDIO: std::cell::RefCell<Vec<f32>> = const { std::cell::RefCell::new(Vec::new()) }; }

#[cfg(target_os = "emscripten")]
pub(crate) fn take_browser_audio() -> Vec<f32> {
    BROWSER_AUDIO.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    #[test]
    fn streamed_audio_is_bounded_and_connection_scoped() {
        let first = StreamAudio::default();
        let second = StreamAudio::default();
        let output = with_stream_audio(&first, AudioOutput::new);
        let other = with_stream_audio(&second, AudioOutput::new);
        assert!(AudioOutput::new().stream.is_none());
        output.submit(vec![0.25; 100]);
        other.submit(vec![0.5; 200]);
        assert_eq!(first.lock().unwrap().len(), 100);
        assert_eq!(*second.lock().unwrap(), vec![0.5; 200]);
        output.submit(vec![1.0; 96_000]);
        assert_eq!(first.lock().unwrap().len(), 48_000);
    }
}
