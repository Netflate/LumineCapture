<div align="center">
  <p>
    <h1>
      <a href="https://github.com/Netflate/LumineCapture">
        <img src="assets/hicolor/scalable/apps/lumine-capture.svg" alt="LumineCapture" width="64" />
      </a>
      <br />
      LumineCapture
    </h1>
    <h4>Fast, lightweight Wayland screenshot tool</h4>
  </p>
</div>

## Preview

![preview](https://github.com/user-attachments/assets/05a7bdd1-9d6d-416a-a52c-e184d4314433)

## Features

- Easy to use.
- Lightweight.
- Written in Rust.
- Doesn't need a background daemon for best performance (except for GPU OCR, but that's optional).
- In-app screenshot editing.
- OCR.
- Pin.
  
## Speed Comparison (LumineCapture vs Flameshot vs Spectacle)
> every test ran on a dual-monitor setup, so each screenshot covered the whole workspace (both screens), which made every run a bit slower. Compare the tools with each other rather than looking at the absolute numbers.


### 1) In optimal conditions:
The measurement for each of the three started exactly one frame before the command execution, so the actual time is approximately 0.10 ms lower for each one. 
It's also important to note that the Spectacle measurement might be unfair. I decided to end the measurement exactly when the application is fully initialized and ready to use, and that's when the toolbar starts appearing, not immediately after the screen dims. This is because Spectacle hasn't loaded the toolbar by that point. The measurement ends at the frame when the toolbar animation starts

![first](https://github.com/user-attachments/assets/7648e73b-9d0f-41f6-84d0-12f127f3c84a)

### 2) In high CPU usage condition: 

![second](https://github.com/user-attachments/assets/8ea42674-09bd-437d-8bee-08865c5a211f)

### 3) Saving a full-workspace screenshot without a GUI (30 times):

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `LumineCapture` | 150.5 ± 51.9 | 111.0 | 280.9 | 1.00 |
| `Spectacle` | 576.1 ± 11.7 | 556.5 | 605.4 | 3.83 ± 1.32 |
| `Flameshot (daemon running)` | 737.9 ± 33.4 | 692.5 | 841.0 | 4.90 ± 1.71 |

Script: [`scripts/bench.sh`](scripts/bench.sh) 

## Usage

LumineCapture supports different launch options, but if you don't really care about them and just want to use it, the four most obvious commands are:

1. `lumine-capture`: takes a screenshot of the whole workspace and opens it in the editor. (If the capture goes through the portal you can of course also pick a single monitor by hand in its dialog, but it is better to use KWin, image-copy or screencopy directly. The backend is chosen automatically; set `LUMINE_CAPTURE=kde`, `image-copy`, `screencopy` or `portal` to force one.)
2. `lumine-capture -m`: takes a screenshot of the monitor under the pointer.
3. `lumine-capture -r`: no editor and no annotations, just drag a region; the screenshot is taken as soon as you release the mouse button (by default it is both saved and copied).
4. `lumine-capture -r ocr`: the same drag, but instead of screenshotting, it scans the text. By default, it is copied to the clipboard. 

**Important:** by default every screenshot is also saved as a file and copied to the clipboard, even if in the editor you only chose to copy or pin it. There are two ways to change that:

1. `-t <letters>` decides what happens to the screenshot for this launch, where the letters are:

    * `s` saves it to the screenshots folder, `~/Pictures/screenshots/<year-month>/` by default
    * `c` copies it to the clipboard
    * `p` pins it to the screen
    * `o` writes the PNG to stdout for piping to utilities and other tools
    * `r` copies the recognized text to the primary selection (middle click). **It works only with `-r ocr`**: screenshot ignores `r`, since vast majority of apps paste only text with the middle click

    The letters can be used in any order, even `scp`; they don't get in each other's way.

2. `to` in the config sets what happens to every screenshot on top of what you choose in the editor:

    ```toml
    [general]
    to = "c"
    ```

    An empty value (`to = ""`) stops it from doing anything with the screenshot on its own.

Whatever `-t` or the config picked is highlighted on the toolbar.

In detail:

```text
lumine-capture [MODE] [OPTIONS]
lumine-capture --ocr-daemon [serve|status|stop|calibrate]

Modes (without one, the editor opens):
  -f, --full           Capture the screen right away, no editor
  -w, --window         Capture the active window right away (KDE Plasma only)
  -r, --region [ocr]   Drag a region, releasing the mouse immediately takes the shot.
                       With ocr, reads the text in it instead. Unlike OCR in the editor,
                       it doesn't show any scanning progress: it closes, and when the scan
                       finishes, shows a notification with the outcome.
                       Recommended to use with -t, see below

Options:
  -m, --monitor        Only the monitor under the pointer
  -d, --delay <SECONDS>
                       Take the shot this long after the launch, e.g. -d 3 or -d 0.5
  -t, --to <LETTERS>   What to do with the shot: c copy, p pin, s save, o write the PNG
                       to stdout, in any order and mix, e.g. -t pc. Defaults to general.to
                       in the config. In the editor, Enter and a double click do this, and
                       any other finish adds it. A bare -t means nothing by default.
                       Only for -r ocr: r copies the text to the primary selection (middle
                       click). Screenshots ignore r, apps paste only text with the middle click
  -o, --output <DIR>   Save the shot into this directory instead of the configured
                       one, creating it if needed; implies s in --to
  -n, --nonotify       Disable notifications for this launch
  -s, --speed          Print how long each startup step took to stderr
      --config <PATH>  Use this config file instead of the default location
      --print-default-config
                       Print the default config.toml and exit
  -h, --help           Print this help and exit
  -V, --version        Print the version and exit
```

### Config file

You can also edit some of the settings (like overriding the default colors) in the configuration file.\
Linux path: `~/.config/LumineCapture/config.toml`.

## Keyboard shortcuts

### Local

These shortcuts are available in the editor. All of them can be changed in the `[keys]` section of the config file.

|  Keys                                                                         |  Description                                                                   |
|---                                                                            |---                                                                             |
| <kbd>S</kbd>                                                                  | Set the Selection as the tool (choose or adjust the capture area)              |
| <kbd>V</kbd>                                                                  | Set the Pick as the tool (select, move and resize annotations)                 |
| <kbd>O</kbd>                                                                  | Set the OCR as the tool (recognize text, select it and copy)                   |
| <kbd>G</kbd>                                                                  | Set the Eyedropper as the tool (pick a color from the screen)                  |
| <kbd>T</kbd>                                                                  | Set the Text as the tool                                                       |
| <kbd>P</kbd>                                                                  | Set the Pen as the tool                                                        |
| <kbd>H</kbd>                                                                  | Set the Highlighter as the tool                                                |
| <kbd>D</kbd>                                                                  | Set the Line as the tool                                                       |
| <kbd>A</kbd>                                                                  | Set the Arrow as the tool                                                      |
| <kbd>R</kbd>                                                                  | Set the Rectangle as the tool                                                  |
| <kbd>C</kbd>                                                                  | Set the Circle as the tool                                                     |
| <kbd>N</kbd>                                                                  | Set the Numbered arrow as the tool                                             |
| <kbd>X</kbd>                                                                  | Set the Redact as the tool (blur || pixelate)                                  |
| <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>                        | Move the selection                                                             |
| <kbd>Shift</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>     | Move the selection faster                                                      |
| <kbd>Alt</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>       | Resize the selection                                                           |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd> | Resize the selection faster                                      |
| <kbd>Return</kbd>                                                             | Finish with `general.to` (copy and save by default)                            |
| <kbd>Ctrl</kbd> + <kbd>C</kbd>                                                | Copy to the clipboard and finish                                               |
| <kbd>Ctrl</kbd> + <kbd>S</kbd>                                                | Save as a file and finish                                                      |
| <kbd>Ctrl</kbd> + <kbd>P</kbd>                                                | Pin to the screen and finish                                                   |
| <kbd>Esc</kbd>                                                                | Quit without taking the screenshot                                             |
| <kbd>Ctrl</kbd> + <kbd>Z</kbd>                                                | Undo the last modification                                                     |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd>, <kbd>Ctrl</kbd> + <kbd>Y</kbd> | Redo the next modification                                                 |
| <kbd>Ctrl</kbd> + <kbd>A</kbd>                                                | Select the whole monitor under the cursor                                      |
| <kbd>Delete</kbd>, <kbd>Backspace</kbd>                                       | Delete the selected annotation                                                 |
| <kbd>Space</kbd>                                                              | Toggle the visibility of the toolbar and the settings panels                   |
| <kbd>M</kbd>                                                                  | Toggle the magnifier                                                           |
| <kbd>[</kbd>, <kbd>]</kbd>                                                    | Decrease / increase the size (width or font) of the current tool               |
| Double click on the selection                                                 | Finish with the default action, same as <kbd>Return</kbd>                      |
| Right Click                                                                   | Clear the selection                                                            |
| Mouse Wheel                                                                   | Change the tool's size (width or font)                                         |
| Hold <kbd>Shift</kbd> while drawing                                           | Straight line, snaps to horizontal or vertical (Pen, Highlighter, Line, Arrow) |

With the OCR tool active, <kbd>Return</kbd> and <kbd>Ctrl</kbd> + <kbd>C</kbd> copy the selected text, and <kbd>Ctrl</kbd> + <kbd>A</kbd> selects all recognized text. With the Eyedropper they copy the picked color.

### Global

- **KDE Plasma:** import [lumine-capture-shortcuts-kde.kksrc](docs/lumine-capture-shortcuts-kde.kksrc) in *System Settings -> Keyboard -> Shortcuts -> Import -> Custom Scheme*, or bind commands youself. Spectacle holds <kbd>Meta</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd> and <kbd>Shift</kbd> + <kbd>Print</kbd> by default, so clear them there first if you want.
- **COSMIC and others:** bind them in your bind settings.


## Installation

Packages are on the [releases page](https://github.com/Netflate/LumineCapture/releases/latest).

> [!NOTE]
> Tested only on **KDE Plasma**, **COSMIC** and **niri**. Other Wayland compositors may work but were never tried. **GNOME is not supported yet.**

Packages are built for x86_64 & ARM64 (aarch64), except for Arch package, its x86_64 only.

**Fedora 43+**

From the [Copr repository](https://copr.fedorainfracloud.org/coprs/netflate/lumine-capture/)
```sh
sudo dnf copr enable netflate/lumine-capture
sudo dnf install lumine-capture
```
or a single package from the releases page:
```sh
sudo dnf install https://github.com/Netflate/LumineCapture/releases/download/v0.3.0/lumine-capture-0.3.0-1.$(uname -m).rpm
```

**Ubuntu 24.04+ / Debian 13+**
```sh
wget https://github.com/Netflate/LumineCapture/releases/download/v0.3.0/lumine-capture_0.3.0-1_$(dpkg --print-architecture).deb
sudo apt install ./lumine-capture_0.3.0-1_$(dpkg --print-architecture).deb
```
**openSUSE Tumbleweed**

Community package from the [openSUSE Build Service](https://build.opensuse.org/project/show/home:itachi_re) (x86_64 only, unofficial repack of the release RPM):
```sh
sudo zypper addrepo https://download.opensuse.org/repositories/home:itachi_re/openSUSE_Tumbleweed/home:itachi_re.repo
sudo zypper refresh
sudo zypper install lumine-capture
```
**Arch Linux**
```sh
curl -LO https://github.com/Netflate/LumineCapture/releases/download/v0.3.0/lumine-capture-0.3.0-1-x86_64.pkg.tar.zst
sudo pacman -U lumine-capture-0.3.0-1-x86_64.pkg.tar.zst
```
or build it yourself from the [PKGBUILD](packaging/arch/PKGBUILD) (the only way for ARM on arch):
```sh
git clone https://github.com/Netflate/LumineCapture && cd LumineCapture/packaging/arch && makepkg -si
```

**aerynOS**
```sh
sudo moss it lumine-capture
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

The bundled [Inter](https://github.com/rsms/inter) font is under the SIL Open Font License 1.1 ([LICENSE-OFL](assets/fonts/LICENSE-OFL.txt)).

## Acknowledgments

A lot of the reference for this project, including the layout of this README, was taken from [Spectacle](https://invent.kde.org/graphics/spectacle) and [Flameshot](https://github.com/flameshot-org/flameshot). Thanks to both projects and everyone who works on them.
