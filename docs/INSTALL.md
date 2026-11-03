# Installing Cawget

Packages are attached to each release on GitHub. Check a download against the release's `SHA256SUMS`:

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

## AppImage (any recent x86_64 distribution)

```sh
chmod +x Cawget_*_amd64.AppImage
./Cawget_*_amd64.AppImage
```

The AppImage carries its own GTK and WebKitGTK libraries. If it does not start because FUSE is missing, run it as
`./Cawget_*_amd64.AppImage --appimage-extract-and-run`, or install your distribution's `libfuse2` package.
The tray icon needs a desktop that shows StatusNotifier icons.

## Debian, Ubuntu and derivatives (.deb)

```sh
sudo apt install ./Cawget_*_amd64.deb
```

## Fedora, openSUSE and other RPM distributions (.rpm)

```sh
sudo dnf install ./Cawget-*.x86_64.rpm      # Fedora
sudo zypper install ./Cawget-*.x86_64.rpm   # openSUSE
```

## Arch Linux (AUR)

The `cawget-bin` package installs the release's .deb contents:

```sh
git clone https://aur.archlinux.org/cawget-bin.git
cd cawget-bin
makepkg -si
```

or with an AUR helper, e.g. `paru -S cawget-bin`. The PKGBUILD lives in `packaging/aur/` of this repository.

## Building from source

Requirements and the full build steps are in the README ("Build"). In short:

```sh
cd gui
npm ci
npx tauri build          # target/release/bundle/: AppImage, .deb and .rpm
cargo build --release -p cawget-cli   # the command line tool: target/release/cawget-cli
```

The command line tool is not part of the packages; copy `target/release/cawget-cli` to a folder in your `PATH`
(and `packaging/cawget-cli.1` to your man path) if you want it.

## Where things are installed

| Item | Location |
|---|---|
| Program | `/usr/bin/cawget` |
| Desktop entry | `/usr/share/applications/Cawget.desktop` |
| Icons | `/usr/share/icons/hicolor/*/apps/cawget.png` |
| AppStream metadata | `/usr/share/metainfo/io.github.mahziyarng98.cawget.metainfo.xml` |
| Licences | `/usr/share/doc/cawget/` |
| Your download queue | `~/.local/share/cawget/cawget.db` |
