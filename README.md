# mitos-software-center

A graphical package manager for MITOS. Install, remove, and update
software without a terminal — the whole reason
[`mitos-pkg`](https://github.com/shark-empire/mitos-pkg) grew a daemon
(`mitos-pkgd`) in the first place.

This app does none of the actual work itself: it connects to a running
`mitos-pkgd` over its Unix socket via `mitos_pkg::daemon::client::DaemonClient`
and renders whatever comes back. See `mitos-pkg`'s own
`docs/INTEGRATION.md` (`## mitos-pkgd (daemon)`) for that protocol.

## Layout

```
src/
  main.rs           sets up channels, spawns the worker, opens the window
  daemon_worker.rs  the only file that talks to mitos-pkgd — everything
                     runs on a background thread so installs/downloads
                     never freeze the UI
  ui.rs             the actual window: search + install, installed +
                     remove, and a live activity log
```

## Running it

Needs a `mitos-pkgd` already running and reachable at
`/run/mitos-pkgd/pkgd.sock` (or wherever `$MITOS_PKGD_SOCKET` points, if
set):

```sh
cargo run --release
```

## Why `eframe`/`egui`, and why the feature flags in `Cargo.toml` matter

`mitos-gui` is a Wayland compositor, not X11 — `Cargo.toml` builds
`eframe` with `default-features = false` and only `"wayland"` (no
`"x11"`) so this doesn't pull in libx11/libxcb for a display server
MITOS doesn't run. `"glow"` (OpenGL) rather than `"wgpu"` for the same
reason `bottom`/`ouch`/etc. were picked as terminal apps elsewhere in
`mitos-packages`: the more conservative, widely-supported option is the
safer default while MITOS's own GPU driver story is still unproven.

**Genuinely open question this repo can't answer on its own:** does a
standard `winit`-based Wayland client (which is what `eframe` is, under
the hood) actually run correctly against `mitos-gui`'s current
implementation of the Wayland protocols it needs (`xdg-shell`, `wl_seat`,
`wl_output`, at minimum)? Wayland's whole value proposition is that any
conformant client works on any conformant compositor — but
"conformant" is doing real work in that sentence, and `mitos-gui`'s own
docs describe a multi-stage build roadmap, so its actual protocol
coverage as of any given commit is a `mitos-gui`-side question this
project can't answer from here. Worth confirming against a real
`mitos-gui` build before assuming this runs, not just builds.

## Known gaps (v1)

- **Activity log, not a real progress bar.** `ProgressEvent`s render as
  lines of text ("Downloading mitos-shell 1.0.0...") rather than a
  percentage — matches the granularity `mitos-pkgd` actually reports
  (see that daemon's own docs on why it's phase-level, not byte-level).
- **One operation at a time, app-wide.** Every button disables while
  anything is in flight. `mitos-pkgd` itself would happily serve
  concurrent read requests (`list`/`search`) alongside a running
  install, but this UI doesn't bother distinguishing that yet — see
  `ui.rs`'s `busy` field.
- **No uninstall confirmation, no "why is this held" surfacing, no
  dependency preview before install.** All real, all skipped for a
  first version that proves the daemon connection end to end before
  investing in polish.
- **Doesn't read `mitos-pkg`'s config file** for a customized
  `daemon_socket` — only the compiled-in default or `$MITOS_PKGD_SOCKET`.
  Loading `Config` properly is a small, safe follow-up.
