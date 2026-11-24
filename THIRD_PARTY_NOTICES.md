# Third-party notices

Tidewater itself is under the [MPL-2.0](LICENSE). Binary builds statically include the components below, each under
its own licence.

## SDL 3

The shell links SDL 3 (built from source by the `sdl3-src` crate) under the zlib licence.

```
Copyright (C) 1997-2026 Sam Lantinga <slouken@libsdl.org>

This software is provided 'as-is', without any express or implied
warranty.  In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

### yuv2rgb (bundled in SDL 3)

SDL 3 bundles `yuv2rgb` (`src/video/yuv2rgb`) under the BSD-3-Clause licence.

```
Copyright (c) 2016, Adrien Descamps
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of yuv2rgb nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## StormLib-derived code

The Huffman and IMA ADPCM decoders in `crates/formats/src/mpq_codec/` are ports of
[StormLib](https://github.com/ladislav-zezula/StormLib) by Ladislav Zezula, under the MIT licence. The full text is in
[licenses/StormLib-MIT.txt](licenses/StormLib-MIT.txt).

## First-run screen font

The first-run screen (`crates/client/src/firstrun/font.rs`) draws with an embedded 8 x 8 bitmap font, because no game
font exists before the game files are imported. The glyph table is `font8x8_basic.h` by Daniel Hepper
([dhepper/font8x8](https://github.com/dhepper/font8x8)), which is in the **public domain**. It is based on the
public-domain IBM VGA 8 x 8 font as published by Marcel Sondaar. Only U+0020 to U+007E are used, unchanged.

## Clear font

The `font` option's clear font (`crates/client/src/clearfont.rs`, glyph table `clearfont/glyphs.rs`) is Pixel Operator
Bold by Jayvee Enaguas, released under **CC0 1.0 Universal** (public domain dedication;
[Pixel Operator on Font Library](https://fontlibrary.org/en/font/pixel-operator)). `tools/gen_clear_font.py` renders
its printable ASCII characters (U+0021 to U+007E) at the font's 16-pixel design size, without smoothing, into the
table; the pixels are unchanged.

## Touch icons and their renderer

The touch controls' icons (`crates/client/assets/touch-icons/*.svg`) are Tidewater's own, under its MPL-2.0; no
third-party icon set is used. They are drawn by [resvg](https://github.com/linebender/resvg) (Apache-2.0 OR MIT),
whose rasteriser [tiny-skia](https://github.com/linebender/tiny-skia) and `tiny-skia-path` are under the BSD-3-Clause
licence ([licenses/tiny-skia-BSD-3-Clause.txt](licenses/tiny-skia-BSD-3-Clause.txt), copyright Google Inc. and
Yevhenii Reizner), and which uses [arrayref](https://github.com/droundy/arrayref) under the BSD-2-Clause licence
([licenses/arrayref-BSD-2-Clause.txt](licenses/arrayref-BSD-2-Clause.txt), copyright David Roundy).

## Cargo dependencies

Generated from `Cargo.lock`. Regenerate it with `python3 tools/gen_notices.py` after any dependency change.

Licence notes:

- **`r-efi`** is offered under several licences; Tidewater uses it under **MIT**.
- **`self_cell`** is offered under Apache-2.0 or GPL-2.0; Tidewater uses it under **Apache-2.0** (the map editor only).
- **`epaint_default_fonts`** (the map editor only) embeds egui's default fonts: Ubuntu Light under the Ubuntu Font
  Licence 1.0, and Hack and Noto Emoji under the SIL Open Font Licence 1.1; the crate's code is MIT or Apache-2.0.
- **`option-ext`** is **MPL-2.0**, which is compatible with this project's licence. It is used unmodified.

<!-- BEGIN GENERATED -->
| Crate | Version | Licence |
|---|---|---|
| [accesskit](https://github.com/AccessKit/accesskit) | 0.25.2 | MIT OR Apache-2.0 |
| [adler2](https://github.com/oyvindln/adler2) | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| [ahash](https://github.com/tkaitchuck/ahash) | 0.8.12 | MIT OR Apache-2.0 |
| [android-activity](https://github.com/rust-mobile/android-activity) | 0.6.1 | MIT OR Apache-2.0 |
| [android-properties](https://github.com/miklelappo/android-properties) | 0.2.2 | MIT |
| [anstream](https://github.com/rust-cli/anstyle.git) | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle](https://github.com/rust-cli/anstyle.git) | 1.0.14 | MIT OR Apache-2.0 |
| [anstyle-parse](https://github.com/rust-cli/anstyle.git) | 1.0.0 | MIT OR Apache-2.0 |
| [anstyle-query](https://github.com/rust-cli/anstyle.git) | 1.1.5 | MIT OR Apache-2.0 |
| [anstyle-wincon](https://github.com/rust-cli/anstyle.git) | 3.0.11 | MIT OR Apache-2.0 |
| [anyhow](https://github.com/dtolnay/anyhow) | 1.0.104 | MIT OR Apache-2.0 |
| [arboard](https://github.com/1Password/arboard) | 3.7.0 | MIT OR Apache-2.0 |
| [arrayref](https://github.com/droundy/arrayref) | 0.3.9 | BSD-2-Clause |
| [arrayvec](https://github.com/bluss/arrayvec) | 0.7.8 | MIT OR Apache-2.0 |
| [as-raw-xcb-connection](https://github.com/psychon/as-raw-xcb-connection) | 1.0.1 | MIT OR Apache-2.0 |
| [atomic-waker](https://github.com/smol-rs/atomic-waker) | 1.1.2 | Apache-2.0 OR MIT |
| [autocfg](https://github.com/cuviper/autocfg) | 1.5.1 | Apache-2.0 OR MIT |
| [base64](https://github.com/marshallpierce/rust-base64) | 0.23.1 | MIT OR Apache-2.0 |
| [bit-set](https://github.com/contain-rs/bit-set) | 0.10.0 | Apache-2.0 OR MIT |
| [bit-vec](https://github.com/contain-rs/bit-vec) | 0.9.1 | Apache-2.0 OR MIT |
| [bitflags](https://github.com/bitflags/bitflags) | 1.3.2 | MIT/Apache-2.0 |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.2 | MIT OR Apache-2.0 |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.12.1 | MIT OR Apache-2.0 |
| [block2](https://github.com/madsmtm/objc2) | 0.5.1 | MIT |
| [block2](https://github.com/madsmtm/objc2) | 0.6.2 | MIT |
| [bumpalo](https://github.com/fitzgen/bumpalo) | 3.20.4 | MIT OR Apache-2.0 |
| [bytemuck](https://github.com/Lokathor/bytemuck) | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| [bytemuck_derive](https://github.com/Lokathor/bytemuck) | 1.12.1 | Zlib OR Apache-2.0 OR MIT |
| [byteorder-lite](https://github.com/image-rs/byteorder-lite) | 0.1.0 | Unlicense OR MIT |
| [bytes](https://github.com/tokio-rs/bytes) | 1.12.1 | MIT |
| [calloop](https://github.com/Smithay/calloop) | 0.13.0 | MIT |
| [calloop-wayland-source](https://github.com/smithay/calloop-wayland-source) | 0.3.0 | MIT |
| [cc](https://github.com/rust-lang/cc-rs) | 1.8.0 | MIT OR Apache-2.0 |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.5 | MIT OR Apache-2.0 |
| [cfg_aliases](https://github.com/katharostech/cfg_aliases) | 0.2.2 | MIT |
| [cgl](https://github.com/servo/cgl-rs) | 0.3.2 | MIT / Apache-2.0 |
| [clap](https://github.com/clap-rs/clap) | 4.6.7 | MIT OR Apache-2.0 |
| [clap_builder](https://github.com/clap-rs/clap) | 4.6.7 | MIT OR Apache-2.0 |
| [clap_derive](https://github.com/clap-rs/clap) | 4.6.7 | MIT OR Apache-2.0 |
| [clap_lex](https://github.com/clap-rs/clap) | 1.1.1 | MIT OR Apache-2.0 |
| [clipboard-win](https://github.com/DoumanAsh/clipboard-win) | 5.4.3 | BSL-1.0 |
| [cmake](https://github.com/rust-lang/cmake-rs) | 0.1.58 | MIT OR Apache-2.0 |
| [cobs](https://github.com/jamesmunns/cobs.rs) | 0.3.0 | MIT OR Apache-2.0 |
| [codespan-reporting](https://github.com/brendanzab/codespan) | 0.13.1 | Apache-2.0 |
| [color](https://github.com/linebender/color) | 0.3.3 | Apache-2.0 OR MIT |
| [colorchoice](https://github.com/rust-cli/anstyle.git) | 1.0.5 | MIT OR Apache-2.0 |
| [combine](https://github.com/Marwes/combine) | 4.6.9 | MIT |
| [concurrent-queue](https://github.com/smol-rs/concurrent-queue) | 2.5.0 | Apache-2.0 OR MIT |
| [const-oid](https://github.com/RustCrypto/formats) | 0.10.2 | Apache-2.0 OR MIT |
| [core-foundation](https://github.com/servo/core-foundation-rs) | 0.9.4 | MIT OR Apache-2.0 |
| [core-foundation-sys](https://github.com/servo/core-foundation-rs) | 0.8.7 | MIT OR Apache-2.0 |
| [core-graphics](https://github.com/servo/core-foundation-rs) | 0.23.2 | MIT OR Apache-2.0 |
| [core-graphics-types](https://github.com/servo/core-foundation-rs) | 0.1.3 | MIT OR Apache-2.0 |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.3.1 | MIT OR Apache-2.0 |
| [crc32fast](https://github.com/srijs/rust-crc32fast) | 1.5.2 | MIT OR Apache-2.0 |
| [crossbeam-utils](https://github.com/crossbeam-rs/crossbeam) | 0.8.23 | MIT OR Apache-2.0 |
| [crunchy](https://github.com/eira-fransham/crunchy) | 0.2.4 | MIT |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.2.2 | MIT OR Apache-2.0 |
| [cursor-icon](https://github.com/rust-windowing/cursor-icon) | 1.2.0 | MIT OR Apache-2.0 OR Zlib |
| [data-url](https://github.com/servo/rust-url) | 0.3.2 | MIT OR Apache-2.0 |
| [digest](https://github.com/RustCrypto/traits) | 0.11.3 | MIT OR Apache-2.0 |
| [directories](https://github.com/soc/directories-rs) | 6.0.0 | MIT OR Apache-2.0 |
| [dirs-sys](https://github.com/dirs-dev/dirs-sys-rs) | 0.5.0 | MIT OR Apache-2.0 |
| [dispatch](http://github.com/SSheldon/rust-dispatch) | 0.2.0 | MIT |
| [dispatch2](https://github.com/madsmtm/objc2) | 0.3.1 | Zlib OR Apache-2.0 OR MIT |
| [dlib](https://github.com/elinorbgr/dlib) | 0.5.3 | MIT |
| [document-features](https://github.com/slint-ui/document-features) | 0.2.12 | MIT OR Apache-2.0 |
| [downcast-rs](https://github.com/marcianx/downcast-rs) | 1.2.1 | MIT/Apache-2.0 |
| [dpi](https://github.com/rust-windowing/winit) | 0.1.2 | Apache-2.0 AND MIT |
| [ecolor](https://github.com/emilk/egui) | 0.37.0 | MIT OR Apache-2.0 |
| [eframe](https://github.com/emilk/egui/tree/main/crates/eframe) | 0.37.0 | MIT OR Apache-2.0 |
| [egui](https://github.com/emilk/egui) | 0.37.0 | MIT OR Apache-2.0 |
| [egui-wgpu](https://github.com/emilk/egui/tree/main/crates/egui-wgpu) | 0.37.0 | MIT OR Apache-2.0 |
| [egui-winit](https://github.com/emilk/egui/tree/main/crates/egui-winit) | 0.37.0 | MIT OR Apache-2.0 |
| [egui_glow](https://github.com/emilk/egui/tree/main/crates/egui_glow) | 0.37.0 | MIT OR Apache-2.0 |
| [either](https://github.com/rayon-rs/either) | 1.19.0 | MIT OR Apache-2.0 |
| [emath](https://github.com/emilk/egui/tree/main/crates/emath) | 0.37.0 | MIT OR Apache-2.0 |
| [embedded-io](https://github.com/embassy-rs/embedded-io) | 0.4.0 | MIT OR Apache-2.0 |
| [embedded-io](https://github.com/rust-embedded/embedded-hal) | 0.6.1 | MIT OR Apache-2.0 |
| [epaint](https://github.com/emilk/egui/tree/main/crates/epaint) | 0.37.0 | MIT OR Apache-2.0 |
| [epaint_default_fonts](https://github.com/emilk/egui/tree/main/crates/epaint_default_fonts) | 0.37.0 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 |
| [equivalent](https://github.com/indexmap-rs/equivalent) | 1.0.3 | Apache-2.0 OR MIT |
| [errno](https://github.com/lambda-fairy/rust-errno) | 0.3.14 | MIT OR Apache-2.0 |
| [error-code](https://github.com/DoumanAsh/error-code) | 3.4.0 | BSL-1.0 |
| [euclid](https://github.com/servo/euclid) | 0.22.14 | MIT OR Apache-2.0 |
| [fastrand](https://github.com/smol-rs/fastrand) | 2.5.0 | Apache-2.0 OR MIT |
| [fax](https://github.com/pdf-rs/fax) | 0.2.7 | MIT |
| [fdeflate](https://github.com/image-rs/fdeflate) | 0.3.7 | MIT OR Apache-2.0 |
| [fearless_simd](https://github.com/linebender/fearless_simd) | 0.4.1 | Apache-2.0 OR MIT |
| [find-msvc-tools](https://github.com/rust-lang/cc-rs) | 0.1.14 | MIT OR Apache-2.0 |
| [fixedbitset](https://github.com/petgraph/fixedbitset) | 0.5.9 | MIT OR Apache-2.0 |
| [flate2](https://github.com/rust-lang/flate2-rs) | 1.1.10 | MIT OR Apache-2.0 |
| [float-cmp](https://github.com/mikedilger/float-cmp) | 0.9.0 | MIT |
| [foldhash](https://github.com/orlp/foldhash) | 0.1.5 | Zlib |
| [foldhash](https://github.com/orlp/foldhash) | 0.2.0 | Zlib |
| [font-types](https://github.com/googlefonts/fontations) | 0.12.7 | MIT OR Apache-2.0 |
| [foreign-types](https://github.com/sfackler/foreign-types) | 0.5.0 | MIT/Apache-2.0 |
| [foreign-types-macros](https://github.com/sfackler/foreign-types) | 0.2.4 | MIT/Apache-2.0 |
| [foreign-types-shared](https://github.com/sfackler/foreign-types) | 0.3.1 | MIT/Apache-2.0 |
| [futures-core](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-task](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [futures-util](https://github.com/rust-lang/futures-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [gethostname](https://codeberg.org/swsnr/gethostname.rs.git) | 1.1.0 | Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.2.17 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.3.4 | MIT OR Apache-2.0 |
| [getrandom](https://github.com/rust-random/getrandom) | 0.4.3 | MIT OR Apache-2.0 |
| [gl_generator](https://github.com/brendanzab/gl-rs/) | 0.14.0 | Apache-2.0 |
| [glam](https://github.com/bitshifter/glam-rs) | 0.33.12 | MIT OR Apache-2.0 |
| [glifo](https://github.com/linebender/vello) | 0.3.0 | Apache-2.0 OR MIT |
| [glow](https://github.com/grovesNL/glow) | 0.17.0 | MIT OR Apache-2.0 OR Zlib |
| [glutin](https://github.com/rust-windowing/glutin) | 0.32.3 | Apache-2.0 |
| [glutin-winit](https://github.com/rust-windowing/glutin) | 0.5.0 | MIT |
| [glutin_egl_sys](https://github.com/rust-windowing/glutin) | 0.7.1 | Apache-2.0 |
| [glutin_glx_sys](https://github.com/rust-windowing/glutin) | 0.6.1 | Apache-2.0 |
| [glutin_wgl_sys](https://github.com/rust-windowing/glutin) | 0.6.1 | Apache-2.0 |
| [guillotiere](https://github.com/nical/guillotiere) | 0.7.0 | MIT/Apache-2.0 |
| [half](https://github.com/VoidStarKat/half-rs) | 2.7.1 | MIT OR Apache-2.0 |
| [harfrust](https://github.com/harfbuzz/harfrust) | 0.12.0 | MIT |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.15.5 | MIT OR Apache-2.0 |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.1 | MIT OR Apache-2.0 |
| [heck](https://github.com/withoutboats/heck) | 0.5.0 | MIT OR Apache-2.0 |
| [hermit-abi](https://github.com/hermit-os/hermit-rs) | 0.5.3 | MIT OR Apache-2.0 |
| [http](https://github.com/hyperium/http) | 1.5.0 | MIT OR Apache-2.0 |
| [httparse](https://github.com/seanmonstar/httparse) | 1.10.1 | MIT OR Apache-2.0 |
| [hybrid-array](https://github.com/RustCrypto/hybrid-array) | 0.4.15 | MIT OR Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25.10 | MIT OR Apache-2.0 |
| [imagesize](https://github.com/Roughsketch/imagesize) | 0.15.0 | MIT |
| [indexmap](https://github.com/indexmap-rs/indexmap) | 2.14.2 | Apache-2.0 OR MIT |
| [is_terminal_polyfill](https://github.com/polyfill-rs/is_terminal_polyfill) | 1.70.2 | MIT OR Apache-2.0 |
| [itertools](https://github.com/rust-itertools/itertools) | 0.15.0 | MIT OR Apache-2.0 |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | MIT OR Apache-2.0 |
| [jni](https://github.com/jni-rs/jni-rs) | 0.22.4 | MIT OR Apache-2.0 |
| [jni-macros](https://github.com/jni-rs/jni-rs) | 0.22.4 | MIT OR Apache-2.0 |
| [jni-sys](https://github.com/jni-rs/jni-sys) | 0.3.1 | MIT OR Apache-2.0 |
| [jni-sys](https://github.com/jni-rs/jni-sys) | 0.4.1 | MIT OR Apache-2.0 |
| [jni-sys-macros](https://github.com/jni-rs/jni-sys) | 0.4.1 | MIT OR Apache-2.0 |
| [jobserver](https://github.com/rust-lang/jobserver-rs) | 0.1.35 | MIT OR Apache-2.0 |
| [js-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) | 0.3.107 | MIT OR Apache-2.0 |
| [khronos_api](https://github.com/brendanzab/gl-rs/) | 3.1.0 | Apache-2.0 |
| [kurbo](https://github.com/linebender/kurbo) | 0.13.1 | Apache-2.0 OR MIT |
| [libc](https://github.com/rust-lang/libc) | 0.2.190 | MIT OR Apache-2.0 |
| [libloading](https://github.com/nagisa/rust_libloading/) | 0.8.9 | ISC |
| [libm](https://github.com/rust-lang/compiler-builtins) | 0.2.16 | MIT |
| [libredox](https://gitlab.redox-os.org/redox-os/libredox.git) | 0.1.25 | MIT |
| [linebender_resource_handle](https://github.com/linebender/raw_resource_handle) | 0.1.1 | Apache-2.0 OR MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [litrs](https://github.com/LukasKalbertodt/litrs) | 1.0.1 | MIT OR Apache-2.0 |
| [lock_api](https://github.com/Amanieu/parking_lot) | 0.4.14 | MIT OR Apache-2.0 |
| [log](https://github.com/rust-lang/log) | 0.4.34 | MIT OR Apache-2.0 |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | Unlicense OR MIT |
| [memmap2](https://github.com/RazrFalcon/memmap2-rs) | 0.9.12 | MIT OR Apache-2.0 |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| [moxcms](https://github.com/awxkee/moxcms.git) | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| [naga](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [naga-types](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [ndk](https://github.com/rust-mobile/ndk) | 0.9.0 | MIT OR Apache-2.0 |
| [ndk-context](https://github.com/rust-windowing/android-ndk-rs) | 0.1.1 | MIT OR Apache-2.0 |
| [ndk-sys](https://github.com/rust-mobile/ndk) | 0.6.0+11769913 | MIT OR Apache-2.0 |
| [nohash-hasher](https://github.com/paritytech/nohash-hasher) | 0.2.0 | Apache-2.0 OR MIT |
| [nom](https://github.com/rust-bakery/nom) | 8.0.0 | MIT |
| [num-traits](https://github.com/rust-num/num-traits) | 0.2.19 | MIT OR Apache-2.0 |
| [num_enum](https://github.com/illicitonion/num_enum) | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| [num_enum_derive](https://github.com/illicitonion/num_enum) | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| [objc-sys](https://github.com/madsmtm/objc2) | 0.3.5 | MIT |
| [objc2](https://github.com/madsmtm/objc2) | 0.5.3 | MIT |
| [objc2](https://github.com/madsmtm/objc2) | 0.6.5 | MIT |
| [objc2-app-kit](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-app-kit](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-cloud-kit](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-contacts](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-core-data](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-core-foundation](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-graphics](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-core-image](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-core-location](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-encode](https://github.com/madsmtm/objc2) | 4.1.0 | MIT |
| [objc2-foundation](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-foundation](https://github.com/madsmtm/objc2) | 0.3.2 | MIT |
| [objc2-io-surface](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-link-presentation](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-metal](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-quartz-core](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-symbols](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-ui-kit](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-ui-kit](https://github.com/madsmtm/objc2) | 0.3.2 | Zlib OR Apache-2.0 OR MIT |
| [objc2-uniform-type-identifiers](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [objc2-user-notifications](https://github.com/madsmtm/objc2) | 0.2.2 | MIT |
| [once_cell](https://github.com/matklad/once_cell) | 1.21.4 | MIT OR Apache-2.0 |
| [once_cell_polyfill](https://github.com/polyfill-rs/once_cell_polyfill) | 1.70.2 | MIT OR Apache-2.0 |
| [option-ext](https://github.com/soc/option-ext.git) | 0.2.0 | MPL-2.0 |
| [orbclient](https://gitlab.redox-os.org/redox-os/orbclient) | 0.3.55 | MIT |
| [os_pipe](https://github.com/oconnor663/os_pipe.rs) | 1.2.3 | MIT |
| [parking_lot](https://github.com/Amanieu/parking_lot) | 0.12.5 | MIT OR Apache-2.0 |
| [parking_lot_core](https://github.com/Amanieu/parking_lot) | 0.9.12 | MIT OR Apache-2.0 |
| [peniko](https://github.com/linebender/peniko) | 0.6.1 | Apache-2.0 OR MIT |
| [percent-encoding](https://github.com/servo/rust-url/) | 2.3.2 | MIT OR Apache-2.0 |
| [petgraph](https://github.com/petgraph/petgraph) | 0.8.3 | MIT OR Apache-2.0 |
| [pico-args](https://github.com/RazrFalcon/pico-args) | 0.5.0 | MIT |
| [pin-project](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [pin-project-internal](https://github.com/taiki-e/pin-project) | 1.1.13 | Apache-2.0 OR MIT |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.17 | Apache-2.0 OR MIT |
| [pkg-config](https://github.com/rust-lang/pkg-config-rs) | 0.3.34 | MIT OR Apache-2.0 |
| [plain](https://github.com/randomites/plain) | 0.2.3 | MIT/Apache-2.0 |
| [png](https://github.com/image-rs/image-png) | 0.18.1 | MIT OR Apache-2.0 |
| [polling](https://github.com/smol-rs/polling) | 3.11.0 | Apache-2.0 OR MIT |
| [polycool](https://github.com/linebender/kurbo) | 0.4.0 | MIT OR Apache-2.0 |
| [portable-atomic](https://github.com/taiki-e/portable-atomic) | 1.15.0 | Apache-2.0 OR MIT |
| [portable-atomic-util](https://github.com/taiki-e/portable-atomic-util) | 0.2.8 | Apache-2.0 OR MIT |
| [postcard](https://github.com/jamesmunns/postcard) | 1.1.3 | MIT OR Apache-2.0 |
| [proc-macro-crate](https://github.com/bkchr/proc-macro-crate) | 3.5.0 | MIT OR Apache-2.0 |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | MIT OR Apache-2.0 |
| [profiling](https://github.com/aclysma/profiling) | 1.0.18 | MIT OR Apache-2.0 |
| [pxfm](https://github.com/awxkee/pxfm) | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| [quick-error](http://github.com/tailhook/quick-error) | 2.0.1 | MIT/Apache-2.0 |
| [quick-xml](https://github.com/tafia/quick-xml) | 0.41.0 | MIT |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | MIT OR Apache-2.0 |
| [r-efi](https://github.com/r-efi/r-efi) | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [r-efi](https://github.com/r-efi/r-efi) | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| [raw-window-handle](https://github.com/rust-windowing/raw-window-handle) | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| [read-fonts](https://github.com/googlefonts/fontations) | 0.41.0 | MIT OR Apache-2.0 |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.4.1 | MIT |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/syscall) | 0.5.18 | MIT |
| [redox_syscall](https://gitlab.redox-os.org/redox-os/kernel) | 0.9.5 | MIT |
| [redox_users](https://gitlab.redox-os.org/redox-os/users) | 0.5.3 | MIT |
| [renderdoc-sys](https://github.com/ebkalderon/renderdoc-rs) | 1.1.0 | MIT OR Apache-2.0 |
| [resvg](https://github.com/linebender/resvg) | 0.49.0 | Apache-2.0 OR MIT |
| [rgb](https://github.com/kornelski/rust-rgb) | 0.8.53 | MIT |
| [ring](https://github.com/briansmith/ring) | 0.17.14 | Apache-2.0 AND ISC |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21.2 | MIT OR Apache-2.0 |
| [rpkg-config](https://github.com/maia-s/rpkg-config-rs) | 0.1.2 | Zlib OR MIT OR Apache-2.0 |
| [rustc-hash](https://github.com/rust-lang-nursery/rustc-hash) | 1.1.0 | Apache-2.0/MIT |
| [rustc-hash](https://github.com/rust-lang/rustc-hash) | 2.1.3 | Apache-2.0 OR MIT |
| [rustc_version](https://github.com/djc/rustc-version-rs) | 0.4.1 | MIT OR Apache-2.0 |
| [rustix](https://github.com/bytecodealliance/rustix) | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustix](https://github.com/bytecodealliance/rustix) | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [rustls](https://github.com/rustls/rustls) | 0.23.46 | Apache-2.0 OR ISC OR MIT |
| [rustls-pki-types](https://github.com/rustls/pki-types) | 1.15.1 | MIT OR Apache-2.0 |
| [rustls-webpki](https://github.com/rustls/webpki) | 0.103.16 | ISC |
| [rustversion](https://github.com/dtolnay/rustversion) | 1.0.23 | MIT OR Apache-2.0 |
| [same-file](https://github.com/BurntSushi/same-file) | 1.0.6 | Unlicense/MIT |
| [scoped-tls](https://github.com/alexcrichton/scoped-tls) | 1.0.1 | MIT/Apache-2.0 |
| [scopeguard](https://github.com/bluss/scopeguard) | 1.2.0 | MIT OR Apache-2.0 |
| [sdl3-src](https://codeberg.org/maia/sdl3-sys-rs) | 3.4.18 | Zlib |
| [sdl3-sys](https://codeberg.org/maia/sdl3-sys-rs) | 0.7.2+SDL-3.4.18 | Zlib |
| [self_cell](https://github.com/Voultapher/self_cell) | 1.3.0 | Apache-2.0 OR GPL-2.0-only |
| [semver](https://github.com/dtolnay/semver) | 1.0.28 | MIT OR Apache-2.0 |
| [send_wrapper](https://github.com/thk1/send_wrapper) | 0.6.0 | MIT/Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | MIT OR Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | 1.0.154 | MIT OR Apache-2.0 |
| [serde_spanned](https://github.com/toml-rs/toml) | 1.1.2 | MIT OR Apache-2.0 |
| [sha2](https://github.com/RustCrypto/hashes) | 0.11.1 | MIT OR Apache-2.0 |
| [shlex](https://github.com/comex/rust-shlex) | 2.0.1 | MIT OR Apache-2.0 |
| [simd-adler32](https://github.com/mcountryman/simd-adler32) | 0.3.10 | MIT |
| [simd_cesu8](https://github.com/seancroach/simd_cesu8) | 1.2.0 | Apache-2.0 OR MIT |
| [simdutf8](https://github.com/rusticstuff/simdutf8) | 0.1.5 | MIT OR Apache-2.0 |
| [simplecss](https://github.com/linebender/simplecss) | 0.2.2 | Apache-2.0 OR MIT |
| [siphasher](https://github.com/jedisct1/rust-siphash) | 1.0.4 | MIT OR Apache-2.0 |
| [skrifa](https://github.com/googlefonts/fontations) | 0.44.0 | MIT OR Apache-2.0 |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | MIT |
| [slotmap](https://github.com/orlp/slotmap) | 1.1.1 | Zlib |
| [smallvec](https://github.com/servo/rust-smallvec) | 1.16.4 | MIT OR Apache-2.0 |
| [smithay-client-toolkit](https://github.com/smithay/client-toolkit) | 0.19.2 | MIT |
| [smol_str](https://github.com/rust-analyzer/smol_str) | 0.2.2 | MIT OR Apache-2.0 |
| [socket2](https://github.com/rust-lang/socket2) | 0.6.5 | MIT OR Apache-2.0 |
| [static_assertions](https://github.com/nvzqz/static-assertions-rs) | 1.1.0 | MIT OR Apache-2.0 |
| [strict-num](https://github.com/RazrFalcon/strict-num) | 0.1.1 | MIT |
| [strsim](https://github.com/rapidfuzz/strsim-rs) | 0.11.1 | MIT |
| [subtle](https://github.com/dalek-cryptography/subtle) | 2.6.1 | BSD-3-Clause |
| [svgtypes](https://github.com/linebender/svgtypes) | 0.16.1 | Apache-2.0 OR MIT |
| [syn](https://github.com/dtolnay/syn) | 2.0.119 | MIT OR Apache-2.0 |
| [syn](https://github.com/dtolnay/syn) | 3.0.8 | MIT OR Apache-2.0 |
| [tempfile](https://github.com/Stebalien/tempfile) | 3.27.0 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror](https://github.com/dtolnay/thiserror) | 2.0.21 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 1.0.69 | MIT OR Apache-2.0 |
| [thiserror-impl](https://github.com/dtolnay/thiserror) | 2.0.21 | MIT OR Apache-2.0 |
| [tiff](https://github.com/image-rs/image-tiff) | 0.11.3 | MIT |
| [tiny-skia](https://github.com/linebender/tiny-skia) | 0.12.0 | BSD-3-Clause |
| [tiny-skia-path](https://github.com/linebender/tiny-skia/tree/master/path) | 0.12.0 | BSD-3-Clause |
| [tokio](https://github.com/tokio-rs/tokio) | 1.54.1 | MIT |
| [toml](https://github.com/toml-rs/toml) | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_datetime](https://github.com/toml-rs/toml) | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_edit](https://github.com/toml-rs/toml) | 0.25.18+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_parser](https://github.com/toml-rs/toml) | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| [toml_writer](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| [tracing](https://github.com/tokio-rs/tracing) | 0.1.44 | MIT |
| [tracing-core](https://github.com/tokio-rs/tracing) | 0.1.36 | MIT |
| [tree_magic_mini](https://github.com/mbrubeck/tree_magic/) | 3.2.2 | MIT |
| [type-map](https://github.com/kardeiz/type-map) | 0.5.1 | MIT/Apache-2.0 |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | MIT OR Apache-2.0 |
| [unicode-general-category](https://github.com/yeslogic/unicode-general-category) | 1.1.0 | Apache-2.0 |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-properties](https://github.com/unicode-rs/unicode-properties) | 0.1.5 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| [unicode-segmentation](https://github.com/unicode-rs/unicode-segmentation) | 1.13.3 | MIT OR Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2.3 | MIT OR Apache-2.0 |
| [untrusted](https://github.com/briansmith/untrusted) | 0.9.0 | ISC |
| [ureq](https://github.com/algesten/ureq) | 3.5.0 | MIT OR Apache-2.0 |
| [ureq-proto](https://github.com/algesten/ureq-proto) | 0.6.5 | MIT OR Apache-2.0 |
| [usvg](https://github.com/linebender/resvg) | 0.49.0 | Apache-2.0 OR MIT |
| [utf8-zero](https://github.com/algesten/utf8-zero) | 0.8.1 | MIT OR Apache-2.0 |
| [utf8parse](https://github.com/alacritty/vte) | 0.2.2 | Apache-2.0 OR MIT |
| [uuid](https://github.com/uuid-rs/uuid) | 1.29.3 | Apache-2.0 OR MIT |
| [vcpkg](https://github.com/mcgoo/vcpkg-rs) | 0.2.15 | MIT/Apache-2.0 |
| [vello_common](https://github.com/linebender/vello) | 0.2.0 | Apache-2.0 OR MIT |
| [vello_cpu](https://github.com/linebender/vello) | 0.2.0 | Apache-2.0 OR MIT |
| [version_check](https://github.com/SergioBenitez/version_check) | 0.9.5 | MIT/Apache-2.0 |
| [walkdir](https://github.com/BurntSushi/walkdir) | 2.5.0 | Unlicense/MIT |
| [wasi](https://github.com/bytecodealliance/wasi) | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasip2](https://github.com/bytecodealliance/wasi-rs) | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-futures](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) | 0.4.80 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-macro-support](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) | 0.2.130 | MIT OR Apache-2.0 |
| [wasm-bindgen-shared](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) | 0.2.130 | MIT OR Apache-2.0 |
| [wayland-backend](https://github.com/smithay/wayland-rs) | 0.3.17 | MIT |
| [wayland-client](https://github.com/smithay/wayland-rs) | 0.31.15 | MIT |
| [wayland-csd-frame](https://github.com/rust-windowing/wayland-csd-frame) | 0.3.0 | MIT |
| [wayland-cursor](https://github.com/smithay/wayland-rs) | 0.31.14 | MIT |
| [wayland-protocols](https://github.com/smithay/wayland-rs) | 0.32.13 | MIT |
| [wayland-protocols-plasma](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-protocols-wlr](https://github.com/smithay/wayland-rs) | 0.3.12 | MIT |
| [wayland-scanner](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [wayland-sys](https://github.com/smithay/wayland-rs) | 0.31.11 | MIT |
| [web-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) | 0.3.107 | MIT OR Apache-2.0 |
| [web-time](https://github.com/daxpedda/web-time) | 1.1.0 | MIT OR Apache-2.0 |
| [webpki-roots](https://github.com/rustls/webpki-roots) | 1.0.10 | CDLA-Permissive-2.0 |
| [weezl](https://github.com/image-rs/weezl) | 0.1.12 | MIT OR Apache-2.0 |
| [wgpu](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [wgpu-core](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [wgpu-core-deps-windows-linux-android](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [wgpu-hal](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [wgpu-naga-bridge](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [wgpu-types](https://github.com/gfx-rs/wgpu) | 30.0.2 | MIT OR Apache-2.0 |
| [winapi-util](https://github.com/BurntSushi/winapi-util) | 0.1.11 | Unlicense OR MIT |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.52.0 | MIT OR Apache-2.0 |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.61.2 | MIT OR Apache-2.0 |
| [windows-targets](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_aarch64_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_aarch64_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_gnu](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_i686_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_gnu](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_gnullvm](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [windows_x86_64_msvc](https://github.com/microsoft/windows-rs) | 0.52.6 | MIT OR Apache-2.0 |
| [winit](https://github.com/rust-windowing/winit) | 0.30.13 | Apache-2.0 |
| [winnow](https://github.com/winnow-rs/winnow) | 1.0.4 | MIT |
| [winresource](https://github.com/BenjaminRi/winresource) | 0.1.34 | MIT |
| [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen) | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [wl-clipboard-rs](https://github.com/YaLTeR/wl-clipboard-rs) | 0.9.5 | MIT OR Apache-2.0 |
| [x11-dl](https://github.com/AltF02/x11-rs.git) | 2.21.0 | MIT |
| [x11rb](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [x11rb-protocol](https://github.com/psychon/x11rb) | 0.13.2 | MIT OR Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | 0.3.11 | MIT |
| [xkbcommon-dl](https://github.com/rust-windowing/xkbcommon-dl) | 0.4.2 | MIT |
| [xkeysym](https://github.com/notgull/xkeysym) | 0.2.1 | MIT OR Apache-2.0 OR Zlib |
| [xml-rs](https://github.com/kornelski/xml-rs) | 0.8.29 | MIT |
| [zerocopy](https://github.com/google/zerocopy) | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zerocopy-derive](https://github.com/google/zerocopy) | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| [zeroize](https://github.com/RustCrypto/utils) | 1.9.1 | Apache-2.0 OR MIT |
| [zlib-rs](https://github.com/trifectatechfoundation/zlib-rs) | 0.6.8 | Zlib |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | MIT |
| [zune-core](https://github.com/etemesi254/zune-image) | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| [zune-jpeg](https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg) | 0.5.15 | MIT OR Apache-2.0 OR Zlib |
<!-- END GENERATED -->

## Android build files

- **Gradle wrapper** (`platform/android/gradlew`, `gradlew.bat`, `gradle/wrapper/gradle-wrapper.jar`), from SDL's
  Android project template, is part of Gradle and is under the
  [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0). It is a build tool and is not included in the app.
- **SDL's Java sources** (`org.libsdl.app`) are copied from the `sdl3-src` crate by `platform/android/build.sh` at build
  time and compiled into the APK under SDL's zlib licence above.

## Update check (desktop builds)

The desktop program links `ureq` (MIT or Apache-2.0), `rustls` (Apache-2.0, ISC or MIT), `rustls-webpki` (ISC), `ring` (an ISC-style licence with OpenSSL-derived parts) and `webpki-roots` (CDLA-Permissive-2.0, Mozilla root certificates) to ask GitHub for the latest release. Their licence texts are in their crates.
