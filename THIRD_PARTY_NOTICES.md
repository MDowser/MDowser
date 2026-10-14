# Third-party notices

MDowser itself is distributed under the Mozilla Public License 2.0
(`MPL-2.0`, see [`LICENSE`](LICENSE)).

The release binaries statically link third-party Rust crates and embed
third-party fonts. This file lists every license family in the dependency
graph, the components whose licenses require a notice, every direct
dependency with its pinned license expression, and the full texts where the
license requires them to travel with the binary. The complete, pinned
dependency list is [`Cargo.lock`](Cargo.lock); the allowed-license policy is
[`deny.toml`](deny.toml).

## License summary (release dependency graph)

The dependency graph is dominated by permissive licenses that require only
attribution (all satisfied by this file plus each crate's own `LICENSE`):

| License | Representative components |
| --- | --- |
| MIT | `bytes`, `hyper`, `mio`, `harfrust`, `objc2` |
| Apache-2.0 | `ab_glyph`, `glutin`, `winit`, `accesskit_winit` |
| `MIT OR Apache-2.0` and variants | `accesskit`, `ahash`, `reqwest`, `tokio`, `eframe`/`egui` |
| BSD-2-Clause / BSD-3-Clause | `rav1d` (BSD-2), `avif-decode`, `tiny-skia` (BSD-3), `subtle`, `brotli` |
| ISC | `libloading`, `rustls-webpki`, `untrusted`, `aws-lc-rs`/`aws-lc-sys` |
| Zlib | `foldhash`, `slotmap`, `zlib-rs` |
| `Unlicense OR MIT` / `CC0-1.0` / `MIT-0` | `memchr`, `byteorder`, `dunce`, `imgref`, `to_method` |
| `Apache-2.0 WITH LLVM-exception` | `rustix`, `linux-raw-sys`, `wasi` |
| Unicode-3.0 | `icu_*`, `litemap`, `tinystr`, `writeable`, `yoke`, `zerovec`, ... |
| BSL-1.0 | `clipboard-win`, `error-code` (Windows-only) |
| MPL-2.0 | `avif-parse` (file-level copyleft; unmodified, see below) |
| OFL-1.1 + Ubuntu-font-1.0 + Bitstream Vera | `epaint_default_fonts` (Ubuntu Light, Hack, Noto Emoji; compiled into the binary but never registered by MDowser) + the bundled Noto faces under `assets/fonts/` (see below) |

## Direct dependencies (shipped binary)

Every direct dependency, with the license expression of the pinned version
in `Cargo.lock` (each crate's own sources carry the full text; this table
is the attribution summary). `deny.toml` enforces the allowed-license
policy in CI-style `cargo deny check`.

| Crate | License |
| --- | --- |
| `arboard` (macOS only) | MIT OR Apache-2.0 |
| `avif-decode` | BSD-3-Clause |
| `avif-parse` | MPL-2.0 (see below) |
| `aws-lc-rs` | ISC AND (Apache-2.0 OR ISC) (see below) |
| `brotli` | BSD-3-Clause AND MIT |
| `eframe` (egui, epaint, wgpu, winit, ...) | MIT OR Apache-2.0 |
| `emojis` | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `flate2` | MIT OR Apache-2.0 |
| `futures-util` | MIT OR Apache-2.0 |
| `harfrust` | MIT |
| `image` | MIT OR Apache-2.0 |
| `jxl-oxide` | MIT OR Apache-2.0 |
| `libc` | MIT OR Apache-2.0 |
| `muda` (macOS only) | Apache-2.0 OR MIT |
| `objc2` (macOS only) | MIT |
| `objc2-app-kit` (macOS only) | Zlib OR Apache-2.0 OR MIT |
| `poll-promise` | MIT OR Apache-2.0 |
| `pulldown-cmark` | MIT |
| `reqwest` | MIT OR Apache-2.0 |
| `resvg` | Apache-2.0 OR MIT |
| `roxmltree` | MIT OR Apache-2.0 |
| `serde` | MIT OR Apache-2.0 |
| `serde_norway` | MIT OR Apache-2.0 |
| `skrifa` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `unicode-bidi` | MIT OR Apache-2.0 |
| `unicode-segmentation` | MIT OR Apache-2.0 |
| `url` | MIT OR Apache-2.0 |

Build-only dependencies (`resvg` and `brotli` also appear in
`[build-dependencies]`) never link into the binary; their notices are
covered by the same rows.

`Unicode-3.0` (Unicode License v3):

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2025 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS
INCLUDED IN THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT
OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF
USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THE DATA FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

## AWS-LC (rustls crypto provider): `aws-lc-rs` / `aws-lc-sys`

`reqwest`'s `rustls` backend links AWS-LC for TLS cryptography: the safe
`aws-lc-rs` wrapper plus the statically linked `aws-lc-sys` C/assembly
library. AWS-LC is distributed under the Apache-2.0 license, with portions
under ISC, MIT, BSD-3-Clause and MIT-0 (its `LICENSE` file carries the
per-component terms). The complete copyright notices and license texts ship
inside the `aws-lc-sys` crate sources from crates.io; the project is at
<https://github.com/aws/aws-lc>.

## MPL-2.0: `avif-parse`

`avif-decode` (pure-Rust AVIF support) depends on `avif-parse`, a fork of
the `mp4parse` crate licensed under the Mozilla Public License 2.0
(file-level copyleft). MDowser links it **unmodified** and does not
relicense it; the source is available from crates.io
(<https://crates.io/crates/avif-parse>) and its upstream repository. The
MPL-2.0 text is available at <https://mozilla.org/MPL/2.0/>. No changes
are made to MPL-covered files, so only this notice is required.

## BSL-1.0: `clipboard-win`, `error-code`

Both crates are Windows-only transitive dependencies (clipboard support
through `egui-winit`). Boost Software License - Version 1.0:

```text
Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization
obtaining a copy of the software and accompanying documentation covered by
this license (the "Software") to use, reproduce, display, distribute,
execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to
do so, all subject to the following:

The copyright notices in the Software and this entire statement, including
the above license grant, this restriction and the following disclaimer,
must be included in all copies of the Software, in whole or in part, and
all derivative works of the Software, unless such copies or derivative
works are solely in the form of machine-executable object code generated by
a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT
SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE
FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

## Bundled fonts (`epaint_default_fonts` + `assets/fonts/`)

`egui`/`epaint` compile Ubuntu Light, Hack, Noto Emoji and emoji-icon-font
into the binary. The crate's declared expression is
`(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`; Hack adds the
Bitstream Vera License (`fonts/Hack-Regular.txt`), reproduced below.
MDowser deliberately does NOT register any of them: its UI and SVG text
stacks bind only the Noto faces below, so those faces are present in the
executable but never painted.

MDowser embeds the SIL Open Font License 1.1 (OFL-1.1)
faces under `assets/fonts/` listed below, shipped unmodified and registered both
for UI text (`ui::theme::apply`) and for statically rasterized SVG text
(`media::decode`):

| Asset | Face (version) | Copyright notice | SHA-256 |
| --- | --- | --- | --- |
| `assets/fonts/NotoEmoji-Variable.ttf` (1.9 MB) | Noto Emoji, variable `wght` (3.002) | Copyright 2013 Google LLC | `de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551` |
| `assets/fonts/NotoSansCJK-Regular.ttc` (18.6 MB) | Noto Sans CJK + Noto Sans Mono CJK, TTC faces 0-4 proportional / 5-9 mono in JP, KR, SC, TC and HK order (2.004) | © 2014-2021 Adobe (<http://www.adobe.com/>). | `b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a` |
| `assets/fonts/NotoSans-Variable.ttf` (2.0 MB) | Noto Sans, variable `wdth`/`wght` (2.015) | Copyright 2022 The Noto Project Authors (<https://github.com/notofonts/latin-greek-cyrillic>) | `bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d` |
| `assets/fonts/NotoColorEmoji-Regular.ttf` (24.2 MB) | Noto Color Emoji, COLRv1 + SVG-in-OpenType (2.055) | Copyright 2022 Google Inc. | `4d82a18d8d95f60ba883ce242bbadbf84a576e987745dd7ba38d71c67cff2d73` |

Provenance and purpose:

- **Noto Emoji** from the Google Fonts repository
  (<https://github.com/google/fonts/tree/main/ofl/notoemoji>): complete
  monochrome emoji coverage; epaint's bundled subset is older and renders
  recent code points (`🤩`, `🫶`, ...) as tofu boxes. It sits right behind
  each family's primary face (first in both fallback chains).
- **Noto Sans CJK** from the Noto CJK project
  (<https://github.com/notofonts/noto-cjk>, `Sans/OTC/NotoSansCJK-Regular.ttc`):
  Chinese, Japanese and Korean (kana, Hangul, Bopomofo, CJK Unified
  Ideographs + Ext A, plus its own Latin and symbols). The `.ttc`
  collection is embedded once; the UI registers face 0 (proportional JP)
  as a CJK fallback and face 5 (Noto Sans Mono CJK JP) as the MONOSPACE
  primary, so code stays truly monospaced, while the SVG font database
  loads every face, so an SVG `font-family="Noto Sans CJK SC"` resolves
  too.
- **Noto Sans** from the Google Fonts repository
  (<https://github.com/google/fonts/tree/main/ofl/notosans>): the app's
  proportional primary (chrome, menus, documents and the SVG default),
  with Latin Extended-A/Additional and Vietnamese coverage
  (Polish, Czech, Turkish, Romanian, ...).
- **Noto Color Emoji** from the Google Fonts repository
  (<https://github.com/google/fonts/tree/main/ofl/notocoloremoji>): the
  color emoji source. epaint's text stack rasterizes monochrome outlines
  only, so this font is NOT registered in the fallback chains: `media::emoji`
  shapes each emoji cluster with harfrust, isolates the glyph's document
  from the font's SVG table (the font shares one large document between
  thousands of glyphs; only the `glyph<id>` element and its `#id` references
  are extracted, under the em-square viewBox the OpenType SVG spec defines)
  and rasterizes it through the same hardened `resvg` pipeline as SVG
  images. Results are cached as bounded textures (`config::EMOJI_CACHE_*`)
  used by document emoji and stored bookmark-tile glyphs; anything
  that fails falls back to the monochrome Noto Emoji face.

`SCRIPT_FONTS` (`assets/fonts/NotoSans*-Variable.ttf` / `-Regular.ttf`), one face
per writing system, all from the Google Fonts repository
(<https://github.com/google/fonts>, `ofl/<family>/`): the major non-CJK
scripts and Noto Sans Symbols 2. Harfrust applies their complex shaping
(Indic reordering, Arabic joining, ...); the viewer's bidi layer handles
display order for RTL scripts.

| Asset | Script | Version | Copyright notice | SHA-256 |
| --- | --- | --- | --- | --- |
| `assets/fonts/NotoSansArabic-Variable.ttf` (0.81 MB) | Arabic | 2.012 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic) | `63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e` |
| `assets/fonts/NotoSansArmenian-Variable.ttf` (0.21 MB) | Armenian | 2.008 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/armenian) | `0870908d8318435a5daf1cd280ae15063f990cd9ad60f3e94c734ce9e1ffef71` |
| `assets/fonts/NotoSansBengali-Variable.ttf` (0.44 MB) | Bengali | 3.011 | Copyright 2025 The Noto Project Authors (https://github.com/notofonts/bengali) | `dcd42978094e584a849c84a51450eeac40c8826057d566ea6d4b9627a403a05a` |
| `assets/fonts/NotoSansCanadianAboriginal-Variable.ttf` (0.28 MB) | Canadian Aboriginal | 2.004 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/canadian-aboriginal) | `81b0f7e7ca48b28e343bb6a523a066743acedfbb29f374cbe4501e6d5f08f7af` |
| `assets/fonts/NotoSansCherokee-Variable.ttf` (0.21 MB) | Cherokee | 2.001 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/cherokee) | `865d9e644f5011f2020a10325b7242f85c9c8ef0edece00e5ed743e5dfb275fd` |
| `assets/fonts/NotoSansDevanagari-Variable.ttf` (0.61 MB) | Devanagari | 2.007 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/devanagari) | `14ec4af41f27482216d1c2229f417ff9b1425e1babb014e57d1d40d03229853e` |
| `assets/fonts/NotoSansEthiopic-Variable.ttf` (1.09 MB) | Ethiopic | 2.102 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/ethiopic) | `0dbccc00b22d180ebd6a4bd8a733918a29e709fa6798023adc3e6cd40da65077` |
| `assets/fonts/NotoSansGeorgian-Variable.ttf` (0.22 MB) | Georgian | 2.005 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/georgian) | `dc591156f36842d38996c4a7a17fee9bb58e45da3e2cac7a31b7d33de700adb9` |
| `assets/fonts/NotoSansGujarati-Variable.ttf` (0.64 MB) | Gujarati | 2.106 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/gujarati) | `9901d8552f1dd5d2c50dbd4caa6f6e174e74e8264f06594ab259ae6e7b1ac428` |
| `assets/fonts/NotoSansGurmukhi-Variable.ttf` (0.26 MB) | Gurmukhi | 2.004 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/gurmukhi) | `1e6f728fa620e566f842d81e220265813faa12771214765d289c98e035adc5f2` |
| `assets/fonts/NotoSansHebrew-Variable.ttf` (0.11 MB) | Hebrew | 3.001 | Copyright 2024 The Noto Project Authors (https://github.com/notofonts/hebrew) | `7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88` |
| `assets/fonts/NotoSansKannada-Variable.ttf` (0.61 MB) | Kannada | 2.006 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/kannada) | `cca4f3b3a8cb12fb261f1b43baf5d2f7f59d90fe123d41f0065ed3a183997ec9` |
| `assets/fonts/NotoSansKhmer-Variable.ttf` (0.34 MB) | Khmer | 2.004 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/khmer) | `f37a8431a0c5d5ed2f81a767417546aca576a81fb7eff9c924d46aecf828f2ca` |
| `assets/fonts/NotoSansLao-Variable.ttf` (0.17 MB) | Lao | 2.003 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/lao) | `9608b94603a82d09a8038946f9775242f99e3b3459b7f1e4d5b335b578cd7ab3` |
| `assets/fonts/NotoSansMalayalam-Variable.ttf` (0.48 MB) | Malayalam | 2.104 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/malayalam) | `312e0e7c3cc15fa09eb42a8f749eeb246b593ed420e3c81aafe8d910c3a6fb56` |
| `assets/fonts/NotoSansMyanmar-Variable.ttf` (0.75 MB) | Myanmar | 2.107 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/myanmar) | `7abbbfbe2514105d7ce94937aee3feb2ba89b73a256c8b77b5866bd9b83e32ec` |
| `assets/fonts/NotoSansNKo-Regular.ttf` (0.08 MB) | N'Ko | 2.004; ttfautohint (v1.8.4.7-5d5b) | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/nko) | `97954444367ad6708c36e85a89aec2830720e34968398d9caf07e7c3307c7993` |
| `assets/fonts/NotoSansOriya-Variable.ttf` (0.58 MB) | Odia | 2.007 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/oriya) | `910342275d619081b896b4ed25fd9ad6ff320c25591b810de1fcc4beb153ba82` |
| `assets/fonts/NotoSansSinhala-Variable.ttf` (1.13 MB) | Sinhala | 2.006 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/sinhala) | `9bd93e407a278075be403324063bc94a7e306c44de4df81214e932330c22eecf` |
| `assets/fonts/NotoSansSymbols2-Regular.ttf` (1.18 MB) | Symbols 2 | 2.008; ttfautohint (v1.8.4.7-5d5b) | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/symbols) | `7d5fb73b7ca67a6798101741f5d280a3d016a56a197afcd4199dbb57b4b82a21` |
| `assets/fonts/NotoSansTamil-Variable.ttf` (0.32 MB) | Tamil | 2.004 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/tamil) | `aa3a9b321f4b0bb2c40203ffbde9af89713227866e0e13f76e5b9eeea727cf88` |
| `assets/fonts/NotoSansTelugu-Variable.ttf` (0.85 MB) | Telugu | 2.005 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/telugu) | `e618af7bf999df192ed4f388eba2e563f2b5015034e9cbb317b5bd793bd7334d` |
| `assets/fonts/NotoSansThaana-Variable.ttf` (0.11 MB) | Thaana | 3.001 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/thaana) | `a296795c892a9ec2b0f4d3af315568506ef9fd55920386170992d3935d8866ca` |
| `assets/fonts/NotoSansThai-Variable.ttf` (0.21 MB) | Thai | 2.002 | Copyright 2022 The Noto Project Authors (https://github.com/notofonts/thai) | `5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8` |

epaint rasterizes glyphs with fontations (`skrifa`) and shapes with
`harfrust` (complex LTR scripts get their reordering and shaping, and RTL
runs get their glyphs reversed by the shaper; document emoji bypass epaint
entirely and become inline color textures, see above). epaint itself applies no
Unicode bidi algorithm, so the viewer adds the missing display ordering in
`viewer::bidi`: paragraph base direction, visual run order, RTL word
order and paired-punctuation mirroring are resolved before layout, and RTL
paragraphs are laid out from the right. Content split across inline
elements (a link inside an Arabic sentence, for example) is reordered per
fragment, not across the whole paragraph. All fonts are shipped
unmodified, so only this notice and the font licenses below are required.
The MIT parts follow each crate's own license; the OFL, Ubuntu and
Bitstream Vera texts are reproduced in full below, as required for font
redistribution (the Ubuntu/Hack texts cover the faces epaint compiles in
even though MDowser never registers them).

### SIL Open Font License 1.1

```text
This Font Software is licensed under the SIL Open Font License,
Version 1.1.

This license is copied below, and is also available with a FAQ at:
http://scripts.sil.org/OFL

-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font
creation efforts of academic and linguistic communities, and to
provide a free and open framework in which fonts may be shared and
improved in partnership with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply to
any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software
components as distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to,
deleting, or substituting -- in part or in whole -- any of the
components of the Original Version, by changing formats or by porting
the Font Software to a new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed,
modify, redistribute, and sell modified and unmodified copies of the
Font Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components, in
Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the
corresponding Copyright Holder. This restriction only applies to the
primary font name as presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created using
the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

### Ubuntu Font Licence 1.0

```text
-------------------------------
UBUNTU FONT LICENCE Version 1.0
-------------------------------

PREAMBLE
This licence allows the licensed fonts to be used, studied, modified and
redistributed freely. The fonts, including any derivative works, can be
bundled, embedded, and redistributed provided the terms of this licence
are met. The fonts and derivatives, however, cannot be released under
any other licence. The requirement for fonts to remain under this
licence does not require any document created using the fonts or their
derivatives to be published under this licence, as long as the primary
purpose of the document is not to be a vehicle for the distribution of
the fonts.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this licence and clearly marked as such. This may
include source files, build scripts and documentation.

"Original Version" refers to the collection of Font Software components
as received under this licence.

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to
a new environment.

"Copyright Holder(s)" refers to all individuals and companies who have a
copyright ownership of the Font Software.

"Substantially Changed" refers to Modified Versions which can be easily
identified as dissimilar to the Font Software by users of the Font
Software comparing the Original Version with the Modified Version.

To "Propagate" a work means to do anything with it that, without
permission, would make you directly or secondarily liable for
infringement under applicable copyright law, except executing it on a
computer or modifying a private copy. Propagation includes copying,
distribution (with or without modification and with or without charging
a redistribution fee), making available to the public, and in some
countries other activities as well.

PERMISSION & CONDITIONS
This licence does not grant any rights under trademark law and all such
rights are reserved.

Permission is hereby granted, free of charge, to any person obtaining a
copy of the Font Software, to propagate the Font Software, subject to
the below conditions:

1) Each copy of the Font Software must contain the above copyright
notice and this licence. These can be included either as stand-alone
text files, human-readable headers or in the appropriate machine-
readable metadata fields within text or binary files as long as those
fields can be easily viewed by the user.

2) The font name complies with the following:
(a) The Original Version must retain its name, unmodified.
(b) Modified Versions which are Substantially Changed must be renamed to
avoid use of the name of the Original Version or similar names entirely.
(c) Modified Versions which are not Substantially Changed must be
renamed to both (i) retain the name of the Original Version and (ii) add
additional naming elements to distinguish the Modified Version from the
Original Version. The name of such Modified Versions must be the name of
the Original Version, with "derivative X" where X represents the name of
the new work, appended to that name.

3) The name(s) of the Copyright Holder(s) and any contributor to the
Font Software shall not be used to promote, endorse or advertise any
Modified Version, except (i) as required by this licence, (ii) to
acknowledge the contribution(s) of the Copyright Holder(s) or (iii) with
their explicit written permission.

4) The Font Software, modified or unmodified, in part or in whole, must
be distributed entirely under this licence, and must not be distributed
under any other licence. The requirement for fonts to remain under this
licence does not affect any document created using the Font Software,
except any version of the Font Software extracted from a document
created using the Font Software may only be distributed under this
licence.

TERMINATION
This licence becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER
DEALINGS IN THE FONT SOFTWARE.
```

### Bitstream Vera License (Hack)

`epaint` bundles Hack Regular (`fonts/Hack-Regular.ttf`, Copyright 2018
Source Foundry Authors); it is compiled into the binary but never
registered by MDowser. Hack is distributed under the MIT License (the
crate's own license file, alongside Ubuntu Light and Noto Emoji) and
derives from Bitstream Vera Sans Mono, Copyright 2003 Bitstream Inc.,
distributed under the Bitstream Vera License with Reserved Font Names
"Bitstream" and "Vera":

```text
BITSTREAM VERA LICENSE

Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream Vera is a trademark of Bitstream, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy of the fonts accompanying this license ("Fonts") and associated documentation files (the "Font Software"), to reproduce and distribute the Font Software, including without limitation the rights to use, copy, merge, publish, distribute, and/or sell copies of the Font Software, and to permit persons to whom the Font Software is furnished to do so, subject to the following conditions:

The above copyright and trademark notices and this permission notice shall be included in all copies of one or more of the Font Software typefaces.

The Font Software may be modified, altered, or added to, and in particular the designs of glyphs or characters in the Fonts may be modified and additional glyphs or characters may be added to the Fonts, only if the fonts are renamed to names not containing either the words "Bitstream" or the word "Vera".

This License becomes null and void to the extent applicable to Fonts or Font Software that has been modified and is distributed under the "Bitstream Vera" names.

The Font Software may be sold as part of a larger software package but no copy of one or more of the Font Software typefaces may be sold by itself.

THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL BITSTREAM OR THE GNOME FOUNDATION BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.

Except as contained in this notice, the names of Gnome, the Gnome Foundation, and Bitstream Inc., shall not be used in advertising or otherwise to promote the sale, use or other dealings in this Font Software without prior written authorization from the Gnome Foundation or Bitstream Inc., respectively. For further information, contact: fonts at gnome dot org.
```

## License texts for the permissive families

The full MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC and Zlib texts
are the standard ones; each dependency ships its own copy alongside the
crate sources in `~/.cargo/registry/src`, and the upstream projects
publish them in their repositories. MDowser's own license text is in
`LICENSE`. `cargo deny check`
enforces that only the licenses listed in `deny.toml` enter the graph.
