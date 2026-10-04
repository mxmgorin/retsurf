# Shaders

Game Mode can draw the game through a shader: the Shader row in Quick Access and
Settings > Gaming, or `[game_mode.view] shader` in `config.toml`. Menus and the
on-screen keyboard stay unfiltered. GPU renderers only; on the software renderer the
row is hidden.

| Id | Effect |
| --- | --- |
| `scanlines` | Dark rows between the game's lit ones |
| `lcd-grid` | A dark grid between the game's pixels |
| `zfast-crt` | A fast CRT: scanlines and an aperture mask |
| `crt-pi` | A CRT tuned for Raspberry Pi GPUs |
| `crt-lottes` | An arcade-monitor CRT with a curved screen; the heaviest of these |
| `xbr-lv3` | xBR: smooths pixel-art edges and diagonals |
| `omniscale` | OmniScale: a smoother pixel-art upscaler; needs GLES 3 or GL 3 |
| `sharp-bilinear` | Crisp pixels at Fit and Stretch, without uneven widths |
| `sameboy-lcd` | A colour LCD's RGB subpixels, best at 6x and up |
| `sameboy-mono-lcd` | A monochrome LCD's pixel grid and drop shadow |

`zfast-crt`, `crt-pi`, `crt-lottes`, `xbr-lv3` and `sharp-bilinear` come unmodified from
libretro's [glsl-shaders](https://github.com/libretro/glsl-shaders); `omniscale` and the
`sameboy-` pair are ported from [SameBoy](https://github.com/LIJI32/SameBoy). Each file
carries its authors and license.

## Your own

Shaders are single-pass RetroArch GLSL, so most single-pass files from glsl-shaders work
as they are. Put `<id>.glsl` in the `shaders` folder of the data directory (next to `config.toml`) and
pick it by `<id>`. A file named after a built-in replaces it; the built-ins' sources are
in [`resources/shaders`](../resources/shaders). To reload an edited file, switch the row
to Off and back. A shader that fails to compile is logged and the game is drawn
unfiltered. Multi-pass presets (`.glslp`) are not supported, and `#pragma parameter`
values stay at their defaults.

The shader sees the game at its own resolution, as in an emulator: the game's canvas
with Scaling on, exact at Integer; with Scaling Off, the whole page at 240 rows per
screen height.

| Uniform | Value |
| --- | --- |
| `Texture` | The game, nearest-sampled |
| `TextureSize`, `InputSize` | The game's resolution |
| `OutputSize` | The game's size on screen, in pixels |
| `FrameCount`, `FrameDirection` | Frames drawn so far, and 1 |
| `MVPMatrix` | Takes `VertexCoord` to clip space |

Attributes are `VertexCoord` and `TexCoord`. Each file is compiled under `#define VERTEX`
and `#define FRAGMENT`, with its own `#version`, else GLSL 1.00 on GLES and 1.20 on
desktop GL, else the context's version.
