# Synthetic GIF fixtures

`loop.mp4` and `loop.gif` contain 0.6 seconds of solid red followed by 0.6 seconds of solid blue, at 96×64 and 10 fps. They contain no personal media or audio. The changing colors let rendering tests distinguish actual animation from a static preview.

Generated with FFmpeg 8.1.3:

```sh
ffmpeg -v error -nostdin -threads 1 -filter_complex_threads 1 \
  -f lavfi -i 'color=red:size=96x64:rate=10:duration=0.6' \
  -f lavfi -i 'color=blue:size=96x64:rate=10:duration=0.6' \
  -filter_complex '[0:v][1:v]concat=n=2:v=1:a=0' -an \
  -c:v libx264 -threads 1 -pix_fmt yuv420p -movflags +faststart loop.mp4
ffmpeg -v error -nostdin -threads 1 -filter_threads 1 -i loop.mp4 -an -loop 0 loop.gif
```
