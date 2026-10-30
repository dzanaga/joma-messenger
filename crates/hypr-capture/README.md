# hypr-capture (vendored)

From gliff (github.com/omacom/gliff, rev `58c74245`, MIT, see `LICENSE`). Joma's only
change: `CaptureConfig::toplevel` captures one window instead of the output, through
`ext-foreign-toplevel-image-capture-source-manager-v1` with the handle found in
`ext-foreign-toplevel-list-v1` (`Toplevel::pick`: the identifier, Hyprland's `stableId`,
else the app id and title), plus `list_toplevels`. The rest is unchanged.

---

Captures one Hyprland output with `ext-image-copy-capture-v1` into a small
ring of GBM dmabufs, on a dedicated thread. Frames are captured on demand
(`Capturer::request_frame`) and handed out as `CapturedFrame`; dropping a
frame returns its buffer to the ring. A cursor session reports the cursor
image (ARGB, via wl_shm) and position separately, so the cursor is never baked
into the video.

Hyprland reports full-frame damage, so `damage` is informational only.
