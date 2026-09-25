# Third-Party Licenses

Licenses for third-party assets and libraries used in this project.

---

## Phosphor Icons

The default icon set is from [Phosphor Icons](https://phosphoricons.com), an open source icon family.

**License:** MIT

```
MIT License

Copyright (c) 2023 Phosphor Icons

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Lucide Icons

An alternate icon set (selectable via Settings → Interface → Icon Set) is from [Lucide](https://lucide.dev), an open source icon library.

**License:** ISC

```
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2023 as part
of Feather (MIT). All other copyright (c) for Lucide are held by Lucide
Contributors 2025.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```

---

## particle-milkdrop / OjoDrop

The MilkDrop visualizer mode renders presets with the `particle-milkdrop` engine and analyzes audio with `particle-audio`, both from [OjoDrop](https://github.com/sho-run/ojodrop) (built from the project's fork, [f-o-o-g-s/ojodrop](https://github.com/f-o-o-g-s/ojodrop): upstream rev `093e4098f18d89af571ccd59f62aaf8305546084` plus patches that let it run inside iced's GPU device, feed a preset the playing cover, and read preset code case-insensitively as MilkDrop does). nokkvi builds them without the standalone player and without the C++ `.milk` converter, so none of the converter's components (hlsl2glslfork, MojoShader, glsl-optimizer) are compiled in. OjoDrop credits Ryan Geiss (MilkDrop), Jordan Berg (Butterchurn, milkdrop-shader-converter) and Nullsoft / Winamp.

**License:** MIT

```
MIT License

Copyright (c) 2026 Dog House Music Studios

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## BeatDrop

OjoDrop's extended waveform modes 8-17 adapt geometry from [BeatDrop](https://github.com/OfficialIncubo/BeatDrop-Music-Visualizer) (commit `945ae10ecf928d24717b64f4e1a69b2c100c4829`). That code is compiled into every nokkvi build, so its notice ships with nokkvi. No endorsement is implied.

**License:** BSD-3-Clause

```
BSD 3-Clause License

Copyright (c) 2018 Maxim Volskiy and individual contributors
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
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

---

## MilkDrop presets

The presets bundled under `assets/milkdrop/` (embedded in the binary) are the pre-converted JSON presets from [butterchurn-presets](https://github.com/jberg/butterchurn-presets) at commit `c10e2616762f262de898e5d4c1f162a3e01340c1`, minus the presets that repo lists as broken and those that need external images. The file names keep each preset author's credit.

The pack repository is MIT-licensed (below). The individual presets are the work of the MilkDrop community, each author holding copyright in their own preset; they have been freely shared for two decades without a stated license. nokkvi follows the stance of projectM's `presets-cream-of-the-crop` collection: the presets are treated as freely redistributable, and any author who objects to their preset being included will have it removed on request (open an issue).

**License (pack repository):** MIT

```
MIT License

Copyright (c) 2013-2018 Jordan Berg

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Rust Dependencies

All Rust crate dependencies are listed in `Cargo.toml` and `data/Cargo.toml`. Every transitive dependency uses a permissive or file-based weak copyleft open source license (including MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, CC0-1.0, BSL-1.0, Zlib, MPL-2.0, Unicode-3.0, CDLA-Permissive-2.0, and The Unlicense). There are no strong copyleft-only (e.g., GPL-only) dependencies.

You can audit dependency licenses yourself with:

```bash
cargo install cargo-license
cargo license
```
