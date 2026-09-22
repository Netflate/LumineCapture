<div align="center">
  <p>
    <h1>
      <a href="https://github.com/Netflate/LumineCapture">
        <img src="assets/hicolor/scalable/apps/lumine-capture.svg" alt="LumineCapture" width="64" />
      </a>
      <br />
      LumineCapture
    </h1>
    <h4>Lightweight, fast and powerful software</h4>
  </p>
</div>



## Preview & Benchmark against other screenshot utilities
### Preview
### Comparison (LumineCapture vs Flameshot vs Spectacle)
1. In perfect conditions
2. In high usage conditions (under heavy load)

## Features

- Easy to use.
- Lightweight.
- Written in Rust.
- Doesn't need a background daemon for best performance (except for GPU OCR, but that's optional).
- In-app screenshot editing.
- OCR.
- Pin.

## Usage

LumineCapture supports different launch options, but if you don't really care about them and just want to use it, the three most obvious commands are:

1. `lumine-capture`: takes a screenshot of the whole workspace and opens it in the editor. (If the capture goes through the portal you can of course also pick a single monitor by hand in its dialog, but it is better to use KWin or image-copy directly. The backend is chosen automatically; set `LUMINE_CAPTURE=kde`, `image-copy` or `portal` to force one.)
2. `lumine-capture -m`: takes a screenshot of the monitor under the pointer.
3. `lumine-capture -r`: no editor and no annotations, just drag a region; the screenshot is taken as soon as you release the mouse button (by default it is both saved and copied).

There is also `-t <letters>`, which changes what happens to the screenshot depending on your choice, where the letters are:

* `s` saves it to the screenshots folder, `~/Pictures/screenshots/<year-month>/` by default
* `c` copies it to the clipboard
* `p` pins it to the screen

The letters can be used in any order, even `scp`; they don't get in each other's way.

In detail:

```text
lumine-capture [MODE] [OPTIONS]
lumine-capture --ocr-daemon [serve|status|stop|calibrate]

Modes (without one, the editor opens):
  -f, --full           Capture the screen right away, no editor
  -w, --window         Capture the active window right away (KDE Plasma only)
  -r, --region         Drag a region, releasing the mouse takes the shot

Options:
  -m, --monitor        Only the monitor under the pointer
  -t, --to <LETTERS>   What to do with the shot: c copy, p pin, s save, in any order
                       and mix, e.g. -t pc. Defaults to general.accept in the config.
                       In the editor, this is what Enter and a double click do
  -o, --output <DIR>   Save the shot into this directory instead of the configured
                       one, creating it if needed; implies s in --to
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
| <kbd>D</kbd>                                                                  | Set the Line as the tool                                                       |
| <kbd>A</kbd>                                                                  | Set the Arrow as the tool                                                      |
| <kbd>R</kbd>                                                                  | Set the Rectangle as the tool                                                  |
| <kbd>C</kbd>                                                                  | Set the Circle as the tool                                                     |
| <kbd>N</kbd>                                                                  | Set the Numbered arrow as the tool                                             |
| <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>                        | Move the selection                                                             |
| <kbd>Shift</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>     | Move the selection faster                                                      |
| <kbd>Alt</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd>       | Resize the selection                                                           |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd>, <kbd>↓</kbd>, <kbd>↑</kbd>, <kbd>→</kbd> | Resize the selection faster                                      |
| <kbd>Return</kbd>                                                             | Finish with the default action (`general.accept`, copy by default)             |
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

With the OCR tool active, <kbd>Return</kbd> and <kbd>Ctrl</kbd> + <kbd>C</kbd> copy the selected text, and <kbd>Ctrl</kbd> + <kbd>A</kbd> selects all recognized text. With the Eyedropper they copy the picked color.

## Installation

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

## Acknowledgments

A lot of the reference for this project, including the layout of this README, was taken from [Spectacle](https://invent.kde.org/graphics/spectacle) and [Flameshot](https://github.com/flameshot-org/flameshot). Thanks to both projects and everyone who works on them.
