# Installing Cawget

Cawget is distributed as ready-made packages on the
[Releases page](https://github.com/mahziyarng98/cawget/releases). Pick the one that fits your system:

| Your system | Package | Section |
|---|---|---|
| Any 64-bit (x86_64) Linux | AppImage, no installation needed | [AppImage](#appimage-any-x86_64-linux-no-installation) |
| Debian, Ubuntu, Linux Mint and other Debian-based systems | `.deb` | [Debian and Ubuntu](#debian-ubuntu-and-derivatives-deb) |
| Fedora, openSUSE and other RPM-based systems | `.rpm` | [Fedora and openSUSE](#fedora-opensuse-and-other-rpm-systems-rpm) |
| Arch Linux | AUR: **coming soon** (AUR registration is temporarily closed); for now `makepkg` or the AppImage | [Arch Linux](#arch-linux) |

In the commands below, replace `<version>` with the version you downloaded, for example `0.1.1`.

## What has been tested

- **Built and packaged on Ubuntu 22.04 (x86_64)** by the project's release workflow, which also runs the
  automated test suite there.
- **AppImage manually tested on Arch Linux (Hyprland)** on 2026-11-04.
- **Arch package built with `makepkg`** from the downloaded PKGBUILD on Arch Linux on 2026-11-04; installing the
  resulting package has not been tested yet.
- **Untested:** installing and running the .deb and .rpm packages on any distribution (including Ubuntu 22.04
  itself, other Ubuntu or Debian versions, Fedora, openSUSE and others), and installing the Arch package with
  `makepkg -si`. The AppImage has been manually tested only on Arch Linux (Hyprland), on 2026-11-04, as noted
  above. Reports are welcome in the [issue tracker](https://github.com/mahziyarng98/cawget/issues).
- Only 64-bit x86 (x86_64 / amd64) packages are provided. No packages for ARM or other architectures.

## Check your download (recommended)

Every release has a file called `SHA256SUMS` with a checksum for each package. Download it into the same folder
as your package, open a terminal in that folder and run:

```sh
sha256sum -c SHA256SUMS --ignore-missing
```

The line for your file must end in `OK`. `--ignore-missing` skips the packages you did not download. If you see
`FAILED`, delete the file and download it again.

## AppImage (any x86_64 Linux, no installation)

An AppImage is a single file that runs without being installed; it installs nothing system-wide. It carries the
GTK and WebKitGTK libraries it needs.

1. Download `Cawget_<version>_amd64.AppImage` (and `SHA256SUMS`, see above).
2. Make it executable and start it:

   ```sh
   chmod +x Cawget_<version>_amd64.AppImage
   ./Cawget_<version>_amd64.AppImage
   ```

   You can also make it executable in your file manager (Properties, Permissions, "Allow executing file as
   program") and then double-click it.

- If it does not start and mentions FUSE, either install your distribution's `libfuse2` package or start it with
  `./Cawget_<version>_amd64.AppImage --appimage-extract-and-run`.
- To remove it, delete the file. Your download queue stays in `~/.local/share/cawget/` (see below).
- The AppImage does not add an entry to the application menu by itself.

## Debian, Ubuntu and derivatives (.deb)

1. Download `Cawget_<version>_amd64.deb` (and `SHA256SUMS`, see above).
2. Open a terminal in the folder where the file is, then install it:

   ```sh
   sudo apt install ./Cawget_<version>_amd64.deb
   ```

   The leading `./` matters: it tells `apt` to install this file from the current folder. Without it, `apt`
   would look for a package of that name in its online repositories and fail. `apt` also installs the libraries
   the package needs (WebKitGTK 4.1, GTK 3, the tray library) from your distribution.

3. Start Cawget from your application menu (it is listed as **Cawget**), or type `cawget` in a terminal.

To remove it:

```sh
sudo apt remove cawget
```

This removes the program, not your download queue (see [Your data](#your-data)).

The package needs `libwebkit2gtk-4.1-0`, which is not available on older releases such as Ubuntu 20.04 or
Debian 11; on those, use the AppImage.

## Fedora, openSUSE and other RPM systems (.rpm)

1. Download `Cawget-<version>-1.x86_64.rpm` (and `SHA256SUMS`, see above).
2. Install it from the folder where the file is (the leading `./` means "this file in the current folder"):

   ```sh
   sudo dnf install ./Cawget-<version>-1.x86_64.rpm      # Fedora
   sudo zypper install ./Cawget-<version>-1.x86_64.rpm   # openSUSE
   ```

3. Start Cawget from your application menu, or type `cawget` in a terminal.

To remove it: `sudo dnf remove cawget` (Fedora) or `sudo zypper remove cawget` (openSUSE).

## Flatpak bundle

A single-file Flatpak for x86_64. **Cawget is not on Flathub**; this bundle from the Releases page is the only
Flatpak build. You need Flatpak installed and the Flathub remote added (the bundle uses the GNOME runtime from
Flathub). It was built and tested on Arch Linux (Hyprland).

```sh
sha256sum -c cawget-0.1.1-x86_64.flatpak.sha256     # run in the folder with both downloaded files
flatpak install --user cawget-0.1.1-x86_64.flatpak
```

Start **Cawget** from your application menu. To remove it:

```sh
flatpak uninstall --user io.github.mahziyarng98.cawget
```

Note: a Flatpak app started from a terminal with `flatpak run` stops when that terminal is closed. Start it from the
application menu instead. The sandbox gives Cawget network access and your Downloads folder; its data lives under
`~/.var/app/io.github.mahziyarng98.cawget/`.

## Arch Linux

> **AUR package: coming soon (AUR registration is temporarily closed).** Until it is published, `yay -S cawget-bin`
> will not find it.

In the meantime you have two options:

- **Build the package yourself** from the recipe (PKGBUILD) in this repository. `makepkg` downloads the `.deb`
  from the release, checks its SHA-256, turns it into an Arch package, and `-si` installs it together with the
  libraries it needs:

  ```sh
  mkdir cawget-bin && cd cawget-bin
  curl -LO https://raw.githubusercontent.com/mahziyarng98/cawget/main/packaging/aur/PKGBUILD
  makepkg -si
  ```

  This needs the `base-devel` group (`sudo pacman -S --needed base-devel`). It is good practice to read a
  PKGBUILD before running `makepkg`. To remove the package: `sudo pacman -R cawget-bin`.

- **Use the [AppImage](#appimage-any-x86_64-linux-no-installation)**, which needs no installation.

Once the AUR package is published, it can be installed with an AUR helper such as `yay -S cawget-bin`.

## Troubleshooting

- **No tray icon:** the tray icon needs a desktop that shows StatusNotifier icons. Most desktops do; GNOME needs
  an extension for it. Without one, Cawget works normally without a tray icon, and closing the window quits it.

## Your data

All packages store your download queue and settings in one file:

```
~/.local/share/cawget/cawget.db
```

Removing a package does not delete it. Delete that folder yourself if you want to start fresh. It contains only
references to credentials: logins are kept in your system keyring (GNOME Keyring / KWallet). See "Credentials and
the system keyring" in the README, including how existing plaintext databases are migrated (with a backup).

## Where the packages install files

| Item | Location |
|---|---|
| Program (command `cawget`) | `/usr/bin/cawget` |
| Application menu entry | `/usr/share/applications/Cawget.desktop` |
| Icons | `/usr/share/icons/hicolor/*/apps/cawget.png` |
| AppStream metadata | `/usr/share/metainfo/io.github.mahziyarng98.cawget.metainfo.xml` |
| Licences | `/usr/share/doc/cawget/` |

## Command line tool

The packages contain the desktop app only. The command line tool `cawget-cli` is built from source:

```sh
cargo build --release -p cawget-cli   # result: target/release/cawget-cli
```

Copy `target/release/cawget-cli` to a folder in your `PATH` (and `packaging/cawget-cli.1` to your man path) if
you want it. It uses the same download queue as the app.

## Building from source

Requirements and the full build steps are in the README, section "Build". In short:

```sh
cd gui
npm ci
npx tauri build          # results in target/release/bundle/: AppImage, .deb and .rpm
```

## Planned, not available

- An APT repository (automatic updates through `apt`)
- The AUR package `cawget-bin` (see above)
- Flatpak and Snap packages
- A browser extension that hands downloads to Cawget
