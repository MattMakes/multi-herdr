# Encoded master and derived-file gates

Run these gates on the encoded files. Do not approve values measured from an intermediate WAV or from timeline metadata.

## Loudness and true peak

Measure `master.mp4` with:

```sh
ffmpeg -hide_banner -i render/master.mp4 -af ebur128=peak=true -f null -
```

The integrated loudness target is `-14 LUFS`. Accept values from `-15 LUFS` through `-13 LUFS`. The encoded true peak must be `-1.0 dBTP` or lower. AAC encoding can raise the true peak by up to `+4 dB`: native ffmpeg `aac` did so in the retro, and macOS `aac_at` did not. The render scripts use `aac_at` when `ffmpeg -encoders` lists it, else native `aac`. Leave that margin below `-1.0 dBTP` in the source WAV, and always gate on the encoded MP4.

## Frame counts

Count decoded frames in the master and every derived MP4:

```sh
ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of default=nokey=1:noprint_wrappers=1 render/master.mp4
```

Run the same command for `9x16.mp4`, `preview.mp4` and every other derived MP4. Every derived file must have the same frame count as `master.mp4`. A packaging step must never cut the film.

## Exact frame selection

Do not use the ffmpeg `trim` filter on a clip with odd timestamps. Select exact frame numbers with `select='between(n,A,B)'`, restamp them, and write constant 60 fps:

```sh
ffmpeg -i in.mp4 -vf "select='between(n,A,B)',setpts=N/(60*TB)" -fps_mode cfr -r 60 out.mp4
```

Do not use `-fps_mode passthrough` with `-r`: that pair gives an error. Count the packets of `out.mp4` with `ffprobe -count_packets`. It must equal `B - A + 1`.

## Audio length

Do not mux with `-shortest`. It cut the last 3 frames. Pad the audio to the video length instead, for example `apad,atrim=end_sample=<frames / fps * 48000>`.

## Real master timeline

Write `timeline-master.json` when the master is assembled. Put intro scenes first and film scenes after them. Keep its scene order equal to the encoded master so that QA checks the real master.

## Probe frames

Extract new probe frames in every review round. Do not reuse probe frames from an earlier round.
