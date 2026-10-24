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

**Sharer** (`joma-desk/share.rs`): `hypr-capture` fills GBM dmabufs from
`ext-image-copy-capture` on demand (a still screen costs nothing; a 700 ms rescue
recaptures a first frame the compositor withholds). `gliff-vk` splits each frame into two
4:2:0 planes (AVC444) and VA-API encodes both; without a hardware encoder, `gliff-sw`
(OpenH264) sends one 4:2:0 stream. Pacing and bitrate are gliff's `rate.rs` unchanged
(GCC-like controller, quality ladder). The ladder's resolution steps are not taken yet.

**Viewer** (`joma-desk/view.rs`): two VA-API decoders and the `gliff-vk` recombine
shader, or `gliff-sw`; each picture is read back once as BGRA for GPUI, and the remote
pointer is drawn over it (gliff never bakes the cursor into the video).

Only viewing for now: no remote control, no clipboard (gliff has both; see the plan).
