# 2026-10-25: Joma aborts on the first remote video frame

**State:** diagnosed, not fixed. The cause is in our shim (`crates/joma-jami/shim/jami_shim.cpp`).

## The crash

| | |
|---|---|
| Process | `joma` (PID 1852202), `/home/digoz/.local/bin/joma`, binary built 22:07:13 |
| Signal | SIGABRT (`SI_TKILL`), Sun 2026-10-25 22:29:34 CET, host `yoga` |
| Core | `coredumpctl info 1852202` (systemd-coredump, 9.8 MB) |
| Libraries | ffmpeg 2:9.0.2-2 (`libswscale.so.10`), jami-daemon 20260917-2 (`libjami.so.17`) |

The crashing thread is libjami's `VideoReceiveThread`, decoding the peer's video in a call:

```
#0-2  libc raise / abort
#3    libswscale  range_override_needed            (+0x176d4e, av_assert0 → abort)
#4    libswscale  sws_setColorspaceDetails         (+0x177e4e)
#5    libswscale  ff_sws_init_single_context       (+0x99c4)
#6    libswscale  sws_getCachedContext (.cold)     (+0xd5ff)
#7    libjami     jami::video::VideoScaler::scale(AVFrame const*, AVFrame*)
#8    libjami     jami::video::SinkClient::sendFrameTransformed(AVFrame*)
#9    libjami     jami::video::SinkClient::update(...)
#11   libjami     jami::video::VideoGenerator::publishFrame(...)
#13   libjami     jami::MediaDecoder::decode(AVPacket&)
#16   libjami     jami::video::VideoReceiveThread::decodeFrame()
#17   libjami     jami::ThreadLoop::mainloop(...)
```

Frames #3, #5 and #6 are named from Arch's swscale debug info (build id
`eabb12cfa589df15371a6fb0f07b544b02338f43`). The other threads were idle: the gpui
dispatcher, opendht, an ICE/TLS read, x264 workers. Memory and disk were fine, and there
was no OOM.

## Cause

1. The abort is FFmpeg's `av_assert0(desc)` in `libswscale/swscale_internal.h:778`:
   `av_pix_fmt_desc_get()` returned NULL for a pixel format it got. The assert's strings
   and the `call abort` sit right after `range_override_needed`'s two
   `av_pix_fmt_desc_get` calls.
2. `VideoScaler::scale` passes the **output** frame's `width` (0x68), `height` (0x6c)
   and `format` (0x74) to `sws_getCachedContext`, as the disassembly of libjami shows.
   The daemon scales into the frame the sink's `pull()` hands it.
3. The AVFrames read from the core in frame #7:

   | | width | height | format |
   |---|---|---|---|
   | input (decoded) | 640 | 480 | 23 (`AV_PIX_FMT_NV12`) |
   | output (pulled) | 0 | 0 | **-1** (`AV_PIX_FMT_NONE`) |

4. Our pull returns a frame that is not set up:

   ```cpp
   target.preferredFormat = AV_PIX_FMT_BGRA;
   target.pull = []() { return libjami::FrameBuffer(av_frame_alloc()); };
   ```

   `preferredFormat` does not fill the pulled frame in. `av_frame_alloc()` leaves it
   0×0, `AV_PIX_FMT_NONE`, with no buffers.

Every remote video frame that reaches an attached sink takes this path, so any call with
incoming video crashes Joma. This was Joma's first recorded crash, and remote video had
never been tested before (J1 in `PLAN.md`).

## Fix (to do)

In `pull()`, return a frame ready to be written into: `format = AV_PIX_FMT_BGRA`, the
decoder's `width` and `height`, then `av_frame_get_buffer(frame, 0)`.
`Event::DecodingStarted` already carries `width` and `height`. Pass them to
`attach_sink`, keep them in `Sink`, and update them when a new `DecodingStarted`
reports a different size. Setting only the format is not enough, because without buffers
`sws_scale` writes through null data pointers. Then test a call with video between two
test profiles.

## Notes for the next crash

- gdb 17 with `set debuginfod enabled on` died on a libstdc++ assertion
  (`string_view::back`) while loading this core. Instead: fetch the debug file with
  `curl https://debuginfod.archlinux.org/buildid/<id>/debuginfo`, map offsets with
  `nm -n` (addr2line misnames the `.cold` parts), and read memory with
  `gdb -iex 'set debuginfod enabled off'`.
- `rust-lld` also took SIGBUS at 22:28:56 and 22:29:31 while writing its output. That is
  unrelated to this crash: the `joma` binary is from 22:07.
