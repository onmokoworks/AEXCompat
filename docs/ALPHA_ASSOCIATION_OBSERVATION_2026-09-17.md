# AE オラクル比較の alpha association (2026-09-17)

AE の PNG 書き出しと host の raw dump は **alpha association が違う**。同じ絵でも
半透明画素で全部食い違うので、そのまま引き算すると「host が AE と違う」という
誤った結論が出る。2026-09-17 に実際にその誤読を一度出したので、観察と訂正を
時系列で残す。

対象は `KO_Foil.aex` (yama-ko.net、matchName `KO_Foil`、sha256
`b7ee86faa9824a81f00fadcf1bc52252abb609ca8391f5a725dca6dc5de9b705`)、AE 26.3x87、
bpc 8、frame 0、default parameter。入力は straight alpha の RGBA8 PNG
(1151x681、うち完全不透明 487346 px、完全透明 285512 px、0<a<255 が 10973 px)。

## 1. 観察 (時系列)

### 1.1 最初の比較と、そこから出した誤った結論 (後で否定される)

host 側は `render_frame_with_parameters` が返す raw RGBA を、AE 側は
`tools/capture-ae-reference.ps1` の PNG を decode したものを、そのまま
チャンネルごとに引き算した。

- 差のある画素 10954 / 783831
- 差は最大 254 (R), 252 (G), 253 (B)、alpha は全画素一致

このとき「半透明画素の扱いが host と AE で違う」と記録した。**これは誤り**で、
下の 1.3 で否定される。

### 1.2 差分の分布 (事実)

差のあった 10954 px は**全部**ソースの 0<a<255 の画素だった。

| ソースの alpha | 画素数 | うち差のある画素 |
| --- | --- | --- |
| a == 0 | 285512 | 0 |
| 0 < a < 255 | 10973 | 10954 |
| a == 255 | 487346 | 0 |

完全不透明な画素は 1 px も食い違っていない。straight と premultiplied は
**a が不透明な画素では定義上一致する**ので、この分布自体が association 違いの
シグネチャになっている。

### 1.3 AE のパススルー出力 (エフェクト無し)

`capture-ae-reference.ps1 -NoEffect` は同じ PNG を AE にインポートして、
エフェクトを適用せずに書き出す。AE が取り込みと書き出しで何をしているかは
これで直接見える。

観察: AE のパススルー出力は、元 PNG を **premultiply したもの**と全 783831 px で
バイト一致した。丸めは整数領域の round-half-up:

```
out_c = (src_c * a + 127) // 255      (c = R,G,B)、a はそのまま
```

つまり AE は PNG を straight alpha として取り込み、**PNG 書き出しの時点で
premultiply している**。

### 1.4 エフェクトを挟んだ再比較

host に straight のまま入力を渡して KO Foil をかけた出力を、1.3 と同じ式で
premultiply してから AE の出力と比べた。

```
premultiply(host 出力) vs AE 出力 : maxdiff [0 0 0 0] / 差のある画素 0 / 783831
```

**全画素バイト一致**。半透明の 10973 px も含めて一致する。

### 1.5 不透明入力での対照

同じ絵を白背景で合成して完全不透明にしたものを両者に通した:

```
host 出力 vs AE 出力 : maxdiff [0 0 0 0] / 差のある画素 0 / 783831
```

単色 RGBA(32,64,128,255) の 256x144 / 333x177 / 640x360 でも同じくバイト一致
(`docs/TAIL_COHORT_2026-08-18.md` と同じ入力の作り方)。

## 2. 結論と訂正

- **訂正**: 1.1 の「半透明画素の扱いが host と AE で違う」は否定される。
  エフェクトのディスパッチは straight 表現で完全に一致していた。差に見えていたのは
  host の dump (straight) と AE の PNG (premultiplied) を直接引き算していたため。
- AE の PNG 書き出しは premultiplied。host の raw dump / `--dump-frames` の出力は
  呼び出し側が渡した association のまま (host は association を変換しない)。
- したがって AE オラクル比較で半透明画素を含む入力を使うときは、**必ず
  どちらかの association に揃えてから**比較する。揃える向きは premultiplied 側。
  逆向き (unpremultiply) は alpha 0 で定義されず、低 alpha で丸め誤差が増幅する。
- ただし premultiplied 側に揃えるのはタダではない。整数領域の premultiply は
  多対一で、alpha a の画素では straight 値が約 max/a 段階ずれても同じ値に潰れる。
  つまり **低 alpha の実差は変換で消える**。1.4 の一致もこの解像度までの一致で、
  「straight 表現同士がバイト一致した」という主張ではない。AE が書き出すのは
  premultiplied のバイトだけなので、潰れる差はそもそもこのオラクルでは観測不能
  でもある。ツールはこれを `worst_case_hidden_straight_step` として出し、
  claim level に `_after_alpha_association` を付けて区別する (§4)。

## 3. 仮説 / 未確認

- AE が「straight として取り込んだ」のか「内部は premultiplied で持っていて
  書き出しで再度 premultiply した」のかは、この観察では区別できない。区別しなくても
  1.4 の一致は成立するので、比較手順としては「AE の PNG は premultiplied」で足りる。
  内部表現を主張する必要が出たら別途測る。
- AE の footage alpha interpretation は `tools/ae-reference-capture.jsx` が
  明示設定していない (AE の推測に任せている)。この観察は AE の推測が straight に
  なった 1 ファイルでのもの。premultiplied と推測される素材では前提が変わる。
- **16 bpc では上の丸め規則を確認できていない**。`-NoEffect -Bpc 16` を撮って
  `(v*a + 32767) // 65535` および 8→16 の単純スケールと突き合わせたが、
  maxdiff 4 / 差 430044 px で一致しなかった。AE 内部の 0..32768 領域を挟むため
  8bit の規則をそのまま外挿すると外れる。16bit の比較は exactness を主張せず
  tolerance を付けること。ツールはこれを**拒否しない**: `--raw-format
  argb16le-ae` に association を宣言すれば 32768 領域で premultiply して
  比較する。claim level の `_after_alpha_association` 接尾辞が「変換後の一致」
  であることだけは示すが、16bit の規則が AE と合う保証はこの観察には無い。

## 4. ツール側の対応

`tools/compare-pixel-oracles.py` に入れた (同日):

- `--raw-alpha` / `--render-alpha` (`straight` | `premultiplied`)。
  両方指定されていて食い違うときは premultiplied 側に揃えてから比較し、
  レポートの `alpha_association.compared_in` に何をしたか残す。
  premultiply は整数領域の round-half-up (1.3 の規則) で行う。正規化した
  float で掛けると丸めが変わり、バイト一致が ±1 のノイズに化ける。
  **片側だけ指定するのは拒否する** (`InputError`)。無視して続けると 1.1 の
  誤読をそのまま再現する invocation になるため。引数だけで決まる検証なので
  ファイルを読む前に走る。`--raw-u32` 経路は association を artifact metadata
  から取るので、この 2 つを渡したら同じく拒否する (受け取って無視すると
  「変換したつもり」を作る)。
- レポートに `alpha_association` ブロックを常時出す。alpha の 3 クラス
  (transparent / partial / opaque) ごとの画素数と、そのうち差のある画素数を分ける。
  **クラス分けは render 側の alpha で行う** (host 側ではない)。エフェクトが
  alpha を書き換える場合、入力の alpha 分布とは一致しない。
- `mismatches_spare_opaque_pixels`: 「差はあるが、完全不透明な画素が存在して
  そのどれも差がない」という**事実**を出す。straight と premultiplied は
  不透明画素では定義上一致するので、この状態は association 違いが作る形。
  推論の側は `diagnostic` にだけ置き、宣言なし かつ tolerance 超過あり の
  ときだけ出す (通った比較に「やり直せ」と書かないため)。不透明画素が 1 つも
  無いフレームでは「免れた不透明画素」が存在しないので flag は立てない。
- `differences_resolved_by_association`: 変換前に差があって変換後に一致した
  チャンネル数。表現の違いがどれだけあったかを示す。
- `worst_case_hidden_straight_step`: **変換した側**の最も alpha の低い画素に
  おいて、premultiply で潰れてしまう straight 領域の最大段差。
  `f(v) = (v*a + m//2) // m` の連続する v の最長ランから直接数えている
  (`collapse_width`)。`(m-1)//a` という近似は a=1 で 2 倍に外すので使わない
  (m=255, a=1 の真値は 127、近似は 254)。`null` は「その段差を測る整数領域が
  無い」で、変換していないか、float 領域で変換したか、のどちらか。
- `association_is_lossless`: この変換が何かを隠しうるか。`null` は変換していない。
  `worst_case_hidden_straight_step` が `null` になるケースでもこの欄は必ず答える。
  変換側の alpha に **non-finite が 1 画素でもあれば false**: NaN は全 straight 値を
  NaN に、±inf は 0 以外を inf に潰す (符号は積の符号なので負の色成分では反転する)
  ので、その画素は丸ごと消える。[0,1] に clamp して floor を取ると
  「最も潰す入力に最も潰さない答え」を返してしまう。
  それ以外の float 領域の変換は alpha>0 なら乗算が厳密なので true。
- claim level: 変換して比較した場合は `export_exact_after_alpha_association` の
  ように接尾辞が付き、渡されたバイトそのものの一致とは読めないようにする。

使い方:

```
uv run python tools/compare-pixel-oracles.py \
  --raw <host の raw dump> --render <AE の PNG> \
  --width W --height H --raw-format rgba8 \
  --raw-alpha straight --render-alpha premultiplied
```

宣言なしで走らせた場合の `alpha_association` (1.1 の再現。このセッションで
実際に走らせた出力から、`diagnostic` の本文だけ省略):

```json
{
  "association_domain": null,
  "association_is_lossless": null,
  "compared_in": "as_provided",
  "diagnostic": "pixels differ but no fully opaque pixel does: ...",
  "differences_resolved_by_association": 0,
  "mismatched_pixels": {
    "opaque": 0,
    "partial": 10954,
    "transparent": 0
  },
  "mismatches_spare_opaque_pixels": true,
  "pixels": {
    "opaque": 487346,
    "partial": 10973,
    "transparent": 285512
  },
  "raw": "unspecified",
  "render": "unspecified",
  "worst_case_hidden_straight_step": null
}
```

(この 3 クラスの画素数は、このフレームでは 1.2 のソース側の表と一致する。
エフェクトが alpha を変えていない — 1.1 の「alpha は全画素一致」— ためで、
一般には一致しない。)

宣言して走らせると `match: true` /
`claim_level: export_exact_after_alpha_association` になり、
`differences_resolved_by_association: 32727`、
`association_is_lossless: false`、
`worst_case_hidden_straight_step: 255` (このフレームは alpha 0 の画素を持つ
ので、そこでは straight 領域の全部が潰れる) が付く。この 2 つの数字は
このセッションの実行から取ったもので、リポジトリ内の artifact からは再現できない
(入力がローカルの私物画像のため)。
