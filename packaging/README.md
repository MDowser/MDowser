# Packaging

Everything a Linux/Windows package needs, with ONE definition per artifact so
the native installers, the RPM spec and Flatpak cannot drift:

| Path | What it is | Consumed by |
| --- | --- | --- |
| `linux/mdowser.desktop` | Desktop entry (categories, keywords, `StartupWMClass`, Markdown MIME types) | `.deb`, `.rpm`, Flatpak, `make install` |
| `linux/dev.mdowser.app.metainfo.xml` | AppStream metadata — the Software-center page (name, summary, description, links, icon, release) | `.deb`, `.rpm`, Flatpak, `make install` |
| `linux/mdowser.1` | Man page (roff) | `.deb`, `.rpm`, Flatpak, `make install` |
| `rpm/mdowser.spec` | RPM spec built from the full source tree | `make dist-rpm` (needs `rpmbuild`) |
| `flatpak/dev.mdowser.app.yaml` | Flatpak manifest — builds the `mdowser-<ver>.flatpak` bundle | `make dist` / `make flatpak` (needs `flatpak-builder`) |

The `.deb` writer and `scripts/install.sh` share `scripts/packaging.sh`
(`install_linux_tree`), which is also what stages the RPM `%install` tree, so
the file list lives in exactly one place.

Version handling: the metainfo's `<release>` is refreshed to the Cargo.toml
version at package time (`render_metainfo` for the `.deb`/`.rpm`/`make
install`; the Flatpak manifest renders its installed copy the same way in
its build commands), so the committed file stays valid standalone and every
packaged copy can never disagree with the binary. The Flatpak build also
rewrites the launchable desktop id and the stock icon name to the app id,
because Flatpak exports both under `dev.mdowser.app`.
