# Desktop share

Two ways, chosen per call:

| Peer | Way |
|---|---|
| runs Joma (said `Hello`), and we are on Hyprland | **Joma share**: gliff's pipeline over a Jami tunnel |
| anything else, or the Joma share failed | **Jami share**: portal → PipeWire → the call's video |

## Joma share

1. When a call becomes current, each Joma sends `Hello{share, view}` as an in-call
   message of type `application/x-joma-desk+json` (other clients ignore it).
2. Share: the sharer starts the endpoint on `127.0.0.1:<port>`, exposes it as a Jami
   service (`addExposedService`, policy *specific*: only the call peer), and sends
   `Offer{service, device, uri, token, output, size}`.
3. The viewer asks the sharer's daemon for its services (`queryPeerServices`): the answer
   names the advertising device in the form tunnels need. It opens a tunnel
   (`openServiceTunnel`), connects to the local port, sends the token, then speaks
   gliff's protocol (`gliff-proto`, framed by `gliff-transport`).
4. `Accept` once pictures arrive. `Decline`, or no `Accept` within 15 s, makes the
   sharer fall back to the Jami share. `Stop` ends it; the service is removed.

The tunnel is Jami's own peer-to-peer channel (ICE, TLS, TURN when needed), authorised
by contact identity. The token guards the loopback port against other local processes.

**Source.** The user picks an output or one window (`share::sources()`: Hyprland's
monitors and clients). A window is captured on its own through
`ext-foreign-toplevel-image-capture-source`, found in `ext-foreign-toplevel-list` by
its identifier, which Hyprland gives as `stableId` in `hyprctl clients`. gliff's
`hypr-capture` only captures outputs, so Joma vendors it (`crates/hypr-capture`, MIT)
with `CaptureConfig::toplevel` added. `Share::set_source` switches source while the
viewer watches: the capture restarts, the encoder follows the new size (a new
`StreamConfig`), the connection stays. Odd window sizes are cropped to even. An area (`Source::Area`, drawn with `slurp`) is
the output's capture cut on the CPU before encoding: its ring is CPU-readable (linear
or wl_shm), and the pointer is moved into the area and hidden outside it.

**Ink** (`joma-desk/ink.rs`): the viewer's laser and strokes, as fractions of the
shared picture, travel as JSON lines over a second connection through the same tunnel
(the token, then `I` instead of `\n`); the sharer answers `clear`. The share's accept
loop runs the video session and the drawing channel as local tasks side by side. On the
sharer they show in an overlay of our own: a `zwlr_layer_shell` surface on the shared
output (layer overlay, an empty input region so clicks go through), drawn in software
into a wl_shm buffer, rewriting only the rectangle that changed and waiting for the
compositor to release the buffer. A window or an area is placed from Hyprland's
geometry (looked up again every second). GPUI has no layer shell, hence the overlay.

**Preview** (`joma-desk/preview.rs`): picker thumbnails and the sharer's live preview
(1 fps) capture into wl_shm (the compositor copies the frame; no encoder, no render
node of ours) and shrink on the CPU.

**Sharer** (`joma-desk/share.rs`): `hypr-capture` fills GBM dmabufs from
`ext-image-copy-capture` on demand (a still screen costs nothing; a 700 ms rescue
recaptures a first frame the compositor withholds). `gliff-vk` splits each frame into two
4:2:0 planes (AVC444) and VA-API encodes both; without a hardware encoder, `gliff-sw`
(OpenH264) sends one 4:2:0 stream. Pacing and bitrate are gliff's `rate.rs` unchanged
(GCC-like controller, quality ladder). The ladder's resolution steps are not taken yet.

**Viewer** (`joma-desk/view.rs`): two VA-API decoders and the `gliff-vk` recombine
shader, or `gliff-sw`; each picture is read back once as BGRA for GPUI, and the remote
pointer is drawn over it (gliff never bakes the cursor into the video). The stage
(`joma/stage.rs`) zooms (fit, fill, 1:1, free up to 8 screen pixels per picture pixel),
pans, keeps the remote pointer 15 % away from the edges when following, and resets to
fit when the picture's size changes. Full screen is the window's own
(`xdg_toplevel.set_fullscreen`) with only the stage in it.

Only viewing for now: no remote control, no clipboard (gliff has both; see the plan).

## GPU tiers (2026-10-25)

`joma_desk::gpu` picks the hardware: gliff's Vulkan + VA-API pipeline on the compositor's
GPU, else on another GPU that passes, else gliff-sw on the CPU. `joma probe` lists what
each render node offers and the tier picked; `joma probe --bench` times each tier.

On this MacBook the Radeon 550 (Polaris, GFX8) drives the screen, but RADV offers
`VK_EXT_image_drm_format_modifier` only from GFX9, so gliff's Vulkan path refuses it;
the Intel HD 530 passes. The VA-API-only tier (`va.rs`: CPU colour conversion, hardware
H.264) was written for such GPUs, but its first encode on the Radeon hung the VCE ring;
the GPU reset failed and Hyprland went down. That tier is now opt-in (`JOMA_VIDEO=va`),
and the benchmark leaves the display GPU alone unless asked.
