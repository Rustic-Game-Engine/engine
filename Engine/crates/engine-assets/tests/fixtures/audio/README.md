# Audio decoder fixtures

These files contain an original synthesized 440 Hz sine wave, 48 kHz, stereo,
0.12 seconds (5760 frames). They contain no third-party music or recordings and
are covered by the repository license. Tests read the checked-in files directly;
FFmpeg is only needed to regenerate them.

Generate the source with:

```sh
ffmpeg -f lavfi -i 'sine=frequency=440:sample_rate=48000:duration=0.12' -ac 2 -c:a pcm_s16le tone.wav
```

Then encode each fixture using `ffmpeg -i tone.wav -c:a CODEC -map_metadata -1 FILE`:

| File | Codec |
| --- | --- |
| tone.mp3 | libmp3lame |
| tone.aac | aac (ADTS) |
| tone.m4a | aac (AAC-LC in MP4) |
| tone.flac | flac |
| vorbis.ogg | libvorbis |
| opus.ogg | libopus |
