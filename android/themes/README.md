# Omarchy's themes, for joma for Android

The `colors.toml` of each theme Omarchy ships (`/usr/share/omarchy/themes/<name>/`,
from github.com/basecamp/omarchy, MIT licence), copied as `<name>.toml`.
`tools/themes.py` turns each into a palette the phone can switch to at runtime
(Settings → appearance → theme), with the same roles as the desktop
(`tools/brand.py`). The build machine's current theme is added at build time when it
is not one of these. Refresh with:

    for d in /usr/share/omarchy/themes/*/; do cp "$d/colors.toml" "themes/$(basename "$d").toml"; done
