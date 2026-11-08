# Encoded master and derived-file gates

Run these gates on the encoded files. Do not approve values measured from an intermediate WAV or from timeline metadata.

## Loudness and true peak

Measure `master.mp4` with:

```sh
ffmpeg -hide_banner -i render/master.mp4 -af ebur128=peak=true -f null -
```

The integrated loudness target is `-14 LUFS`. Accept values from `-15 LUFS` through `-13 LUFS`. The encoded true peak must be `-1.0 dBTP` or lower. Keep the source WAV at `-2.0 dBTP` or lower because AAC encoding can add about `1 dB`.

## Frame counts

Count decoded frames in the master and every derived MP4:

```sh
ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of default=nokey=1:noprint_wrappers=1 render/master.mp4
```

Run the same command for `9x16.mp4`, `preview.mp4` and every other derived MP4. Every derived file must have the same frame count as `master.mp4`. A packaging step must never cut the film.

## Exact frame selection

Do not use the ffmpeg `trim` filter on a clip with odd timestamps. Select exact frame numbers with `select='between(n,A,B)'` and add `-fps_mode passthrough`.

## Real master timeline

Write `timeline-master.json` when the master is assembled. Put intro scenes first and film scenes after them. Keep its scene order equal to the encoded master so that QA checks the real master.

## Probe frames

Extract new probe frames in every review round. Do not reuse probe frames from an earlier round.
