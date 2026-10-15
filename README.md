# xdg-desktop-portal-crownos

The CrownOS backend for [xdg-desktop-portal]: screen sharing for browsers, OBS,
video calls and anything else that asks the portal for a screen cast.

| Crate | What it is |
|---|---|
| `portal` → `xdg-desktop-portal-crownos` | Serves `org.freedesktop.impl.portal.ScreenCast` (v5) on the session bus and streams the chosen screen, window or workspace over PipeWire. |
| `dialog` → `crownos-portal-dialog` | The picker that asks what to share, and the portal's other alerts, built with crownuikit and tinted with the user's accent. |

Frames never leave the GPU. The portal allocates dmabufs through GBM for the
PipeWire consumer, and hands the same buffers to crownpositor as the ring of a
`crownos-screencast-v1` session. The compositor renders into a buffer, and the
portal queues that buffer to the consumer untouched. When the consumer hands a
buffer back, it goes straight back to the compositor.

## Install

```bash
sudo ./install.sh              # release build, installs under /usr
sudo ./install.sh --uninstall
```

| File | Why |
|---|---|
| `crownos.portal` | Tells xdg-desktop-portal this backend exists and what it implements. |
| `crownos-portals.conf` | For `XDG_CURRENT_DESKTOP=CrownOS`: ScreenCast from this portal, everything else (file chooser, settings, …) from xdg-desktop-portal-gtk. |
| D-Bus service + systemd user unit | Starts the portal on demand. |

crownpositor exports `WAYLAND_DISPLAY`, `DISPLAY` and `XDG_CURRENT_DESKTOP` to
D-Bus activation and the systemd user manager when it starts on real hardware,
which is what lets an activated portal find the session.

## Behaviour

- Sources: screens, single windows and whole workspaces (reported to apps as
  monitors), chosen in a tabbed picker. Apps that only ask for monitors are
  not offered windows. Cursor: hidden or embedded.
- Every Start shows the picker, unless the app asked the portal to remember a
  previous choice (`persist_mode`). Screens are remembered by output name,
  windows by app and title, workspaces by output and name.
- The stream ends, and the app's session is closed, when the source goes away,
  the compositor revokes the capture, the consumer fails, or the user stops it
  from the bar.
- Negotiation prefers single-plane modifiers, because consumers such as
  GStreamer cannot import the extra planes of compressed layouts. A switch to
  a source of another size renegotiates the stream in place.

## Bar control

The portal also owns `org.crownos.ScreenCast` on the session bus, serving
`org.crownos.ScreenCast1` at `/org/crownos/ScreenCast` for crownbar's
recording indicator:

| Member | What it does |
|---|---|
| `Casts` property `a(tssss)` | Running casts: id, app, source kind (`output`, `window`, `workspace`), source id, name. Emits `PropertiesChanged`. |
| `Sources() → a(ssss)` | What a cast could switch to: kind, id, name, detail. |
| `Switch(t id, s kind, s source)` | Points a running cast at another source, without asking again: the bar is the user's own UI. |
| `Stop(t id)` | Ends the cast and closes the app's session. |

[xdg-desktop-portal]: https://flatpak.github.io/xdg-desktop-portal/
