//! Script-owned audio recording and local microphone streams.
use crate::audio_input::AudioInput;
use engine_core::gameplay::Owner;
use std::{collections::BTreeMap, fs::File, io::BufWriter, path::Path};

const MAX_SAMPLES: usize = 16 * 1024 * 1024; // 64 MiB per in-memory recording.
const READ_SAMPLES: usize = 4096;
type Writer = hound::WavWriter<BufWriter<File>>;

struct Capture {
    owner: Owner,
    input: Option<AudioInput>,
    microphone: bool,
    recording: bool,
    active: bool,
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
    cursor: usize,
    writer: Option<Writer>,
    error: Option<String>,
    dropped: usize,
}
impl Capture {
    fn append(&mut self, samples: &[f32]) {
        if !self.active {
            return;
        }
        if self.samples.len() + samples.len() > MAX_SAMPLES {
            self.error =
                Some("recording exceeded the 64 MiB memory limit; stop and unload it".into());
            self.active = false;
            self.input = None;
            return;
        }
        if let Some(writer) = &mut self.writer {
            for sample in samples {
                if let Err(error) = writer.write_sample(*sample) {
                    self.error = Some(format!("cannot write recording: {error}"));
                    self.active = false;
                    self.input = None;
                    break;
                }
            }
        }
        self.samples.extend_from_slice(samples);
    }
    fn pump(&mut self) -> Result<usize, String> {
        let Some(input) = &self.input else {
            return Ok(0);
        };
        let (samples, dropped) = input.read(usize::MAX)?;
        self.append(&samples);
        self.dropped += dropped;
        Ok(dropped)
    }
    fn stop(&mut self) -> Result<(), String> {
        let input = self.input.take();
        if let Some(mut input) = input {
            input.close();
            match input.read(usize::MAX) {
                Ok((samples, dropped)) => {
                    self.dropped += dropped;
                    if self.recording {
                        self.append(&samples);
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        self.active = false;
        if let Some(writer) = self.writer.take() {
            writer.finalize().map_err(|e| e.to_string())?;
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(())
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Default)]
pub(crate) struct AudioCapture {
    captures: BTreeMap<String, Capture>,
}
impl AudioCapture {
    pub fn start(
        &mut self,
        owner: Owner,
        recording: bool,
        microphone: bool,
        path: Option<&Path>,
    ) -> Result<String, String> {
        if self.captures.values().filter(|c| c.owner == owner).count() >= 4 {
            return Err(
                "a script may retain at most four capture handles; unload an old handle first"
                    .into(),
            );
        }
        let input = if microphone {
            Some(AudioInput::open()?)
        } else {
            None
        };
        let sample_rate = input.as_ref().map_or(48_000, |i| i.sample_rate);
        let channels = input.as_ref().map_or(2, |i| i.channels);
        let writer = path
            .map(|path| {
                // Never overwrite an existing file. Parent directories must already exist.
                let file = File::options()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| e.to_string())?;
                hound::WavWriter::new(
                    BufWriter::new(file),
                    hound::WavSpec {
                        channels,
                        sample_rate,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )
                .map_err(|e| e.to_string())
            })
            .transpose()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.captures.insert(
            id.clone(),
            Capture {
                owner,
                input,
                microphone,
                recording,
                active: true,
                sample_rate,
                channels,
                samples: Vec::new(),
                cursor: 0,
                writer,
                error: None,
                dropped: 0,
            },
        );
        Ok(id)
    }
    fn get(&mut self, owner: Owner, id: &str) -> Result<&mut Capture, String> {
        self.captures
            .get_mut(id)
            .filter(|c| c.owner == owner)
            .ok_or_else(|| "capture handle is unavailable or belongs to another script".into())
    }
    pub fn stop(&mut self, owner: Owner, id: Option<&str>, recording: bool) -> Result<(), String> {
        if let Some(id) = id {
            let capture = self.get(owner, id)?;
            if capture.recording != recording {
                return Err("wrong capture handle type".into());
            }
            return capture.stop();
        }
        let mut error = None;
        for capture in self
            .captures
            .values_mut()
            .filter(|c| c.owner == owner && c.recording == recording)
        {
            if let Err(e) = capture.stop() {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
    pub fn read(&mut self, owner: Owner, id: &str) -> Result<serde_json::Value, String> {
        let capture = self.get(owner, id)?;
        if capture.recording {
            capture.pump()?;
        } else {
            let (samples, dropped) = capture
                .input
                .as_ref()
                .map_or(Ok((Vec::new(), 0)), |i| i.read(READ_SAMPLES))?;
            return Ok(
                serde_json::json!({"samples":samples,"sampleRate":capture.sample_rate,
                "channels":capture.channels,"active":capture.active,"droppedSamples":dropped}),
            );
        }
        let dropped = std::mem::take(&mut capture.dropped);
        if let Some(error) = &capture.error {
            return Err(error.clone());
        }
        let count = READ_SAMPLES - READ_SAMPLES % usize::from(capture.channels);
        let end = (capture.cursor + count).min(capture.samples.len());
        let packet = serde_json::json!({"samples":&capture.samples[capture.cursor..end],
            "sampleRate":capture.sample_rate,"channels":capture.channels,
            "active":capture.active,"droppedSamples":dropped});
        capture.cursor = end;
        Ok(packet)
    }
    pub fn unload(&mut self, owner: Owner, id: &str) -> Result<(), String> {
        self.get(owner, id)?;
        self.captures.remove(id);
        Ok(())
    }
    pub fn submit(&mut self, samples: &[f32]) {
        for capture in self
            .captures
            .values_mut()
            .filter(|c| c.recording && c.active)
        {
            if capture.microphone {
                if let Err(error) = capture.pump() {
                    capture.error = Some(error);
                    capture.active = false;
                    capture.input = None;
                }
            } else {
                capture.append(samples);
            }
        }
    }
    pub fn cleanup(&mut self, mut predicate: impl FnMut(Owner) -> bool) {
        self.captures.retain(|_, c| !predicate(c.owner));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> Owner {
        Owner {
            entity_bound: true,
            scene: engine_core::SceneId::new(),
            entity: engine_core::EntityId::new(),
            script: engine_core::ScriptId::new(),
        }
    }
    #[test]
    fn hot_mic_reads_live_pcm_and_stop_releases_input() {
        let owner = owner();
        let mut service = AudioCapture::default();
        let id = service.start(owner, true, false, None).unwrap();
        let capture = service.get(owner, &id).unwrap();
        capture.recording = false;
        capture.microphone = true;
        capture.input = Some(AudioInput::fixture(vec![0.5; READ_SAMPLES + 2], 2, 4));
        let packet = service.read(owner, &id).unwrap();
        assert_eq!(packet["samples"].as_array().unwrap().len(), READ_SAMPLES);
        assert_eq!(packet["droppedSamples"], 4);
        assert_eq!(packet["active"], true);
        assert_eq!(
            service.read(owner, &id).unwrap()["samples"],
            serde_json::json!([0.5, 0.5])
        );
        service.stop(owner, None, false).unwrap();
        assert!(service.get(owner, &id).unwrap().input.is_none());
        let packet = service.read(owner, &id).unwrap();
        assert_eq!(packet["active"], false);
        assert_eq!(packet["samples"], serde_json::json!([]));
    }

    #[test]
    fn recording_retains_pcm_after_stop_and_finalizes_wav() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("record.wav");
        let owner = owner();
        let mut service = AudioCapture::default();
        let id = service.start(owner, true, false, Some(&path)).unwrap();
        service.submit(&[0.25, -0.25, 0.5, -0.5]);
        assert!(service.read(super::tests::owner(), &id).is_err());
        assert!(service.stop(owner, Some(&id), false).is_err());
        service.stop(owner, Some(&id), true).unwrap();
        service.stop(owner, Some(&id), true).unwrap();
        service.submit(&[1.0, 1.0]);
        let packet = service.read(owner, &id).unwrap();
        assert_eq!(
            packet["samples"],
            serde_json::json!([0.25, -0.25, 0.5, -0.5])
        );
        assert_eq!(packet["sampleRate"], 48_000);
        assert_eq!(packet["channels"], 2);
        assert_eq!(packet["active"], false);
        assert_eq!(
            service.read(owner, &id).unwrap()["samples"],
            serde_json::json!([])
        );
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(
            reader
                .samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![0.25, -0.25, 0.5, -0.5]
        );
        assert!(service.start(owner, true, false, Some(&path)).is_err());
        service.unload(owner, &id).unwrap();
        assert!(service.read(owner, &id).is_err());
    }
    #[test]
    fn owner_cleanup_finalizes_files_and_read_packets_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cleanup.wav");
        let owner = owner();
        let mut service = AudioCapture::default();
        let id = service.start(owner, true, false, Some(&path)).unwrap();
        let other = service
            .start(super::tests::owner(), true, false, None)
            .unwrap();
        service.submit(&vec![0.25; READ_SAMPLES * 2]);
        assert_eq!(
            service.read(owner, &id).unwrap()["samples"]
                .as_array()
                .unwrap()
                .len(),
            READ_SAMPLES
        );
        service.cleanup(|o| o == owner);
        assert!(service.read(owner, &id).is_err());
        assert!(service.captures.contains_key(&other));
        assert_eq!(
            hound::WavReader::open(path).unwrap().duration(),
            u32::try_from(READ_SAMPLES).unwrap()
        );
    }
    #[test]
    fn memory_limit_and_handle_limit_are_reported() {
        let owner = owner();
        let mut service = AudioCapture::default();
        let id = service.start(owner, true, false, None).unwrap();
        // Fill directly to avoid writing a 64 MiB fixture to disk.
        service
            .get(owner, &id)
            .unwrap()
            .samples
            .resize(MAX_SAMPLES, 0.0);
        service.submit(&[1.0, 1.0]);
        assert!(
            service
                .stop(owner, Some(&id), true)
                .unwrap_err()
                .contains("64 MiB")
        );
        for _ in 0..3 {
            service.start(owner, true, false, None).unwrap();
        }
        assert!(service.start(owner, true, false, None).is_err());
        service.unload(owner, &id).unwrap();
        assert!(service.start(owner, true, false, None).is_ok());
    }
}
