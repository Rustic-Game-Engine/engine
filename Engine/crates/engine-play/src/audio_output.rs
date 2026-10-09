//! Device output is isolated from the scene and script locks.
#[derive(Default)]
pub(crate) struct AudioOutput {
    #[cfg(windows)]
    sender: Option<std::sync::mpsc::SyncSender<Vec<f32>>>,
}
impl AudioOutput {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<f32>>(4);
            let _ = std::thread::Builder::new()
                .name("rustic-audio".into())
                .spawn(move || {
                    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
                        return;
                    };
                    let sink = rodio::Sink::connect_new(stream.mixer());
                    while let Ok(samples) = receiver.recv() {
                        if !samples.is_empty() && sink.len() < 4 {
                            sink.append(rodio::buffer::SamplesBuffer::new(2, 48_000, samples));
                        }
                    }
                });
            Self {
                sender: Some(sender),
            }
        }
        #[cfg(not(windows))]
        {
            Self::default()
        }
    }
    #[cfg_attr(
        not(windows),
        allow(
            clippy::unused_self,
            reason = "the Windows audio backend retains instance state; other hosts consume samples without output"
        )
    )]
    pub fn submit(&self, samples: Vec<f32>) {
        #[cfg(windows)]
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(samples);
        }
        #[cfg(not(windows))]
        drop(samples);
    }
}
