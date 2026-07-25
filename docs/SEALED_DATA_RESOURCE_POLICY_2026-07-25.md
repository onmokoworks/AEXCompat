# データファイル封入 (sealed data resource) ポリシー v1 (issue #362)

status: 設計確定版。実装は本書に従い、逸脱は実装 PR で「逸脱」として記録する。
前提文書: `docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md` (クラスタセッション)、
`docs/RENDER_SESSION_PROTOCOL_2026-07-19.md` (render セッション)。

## 1. 背景と動機

sealed load tree は「DLL だけをフラットに staging する」前提で作られている。
この前提が成り立たないプラグインが実環境で確認された (issue #362):

- **AddGrain.aex**: GLOBAL_SETUP / PARAMS_SETUP で
  `<プラグインのモジュール dir>/Film Stocks/` 配下の `.grain` データファイルを
  列挙・読み込みし、無いと内部で失敗する (params_setup_error=1)。
- **Scribble.aex**: BIB.dll を import しない closure を持ち、#485 の bounded
  BIB ロード (プラグインの自 dir 直下の BIB.dll だけを許す) が sealed 経路では
  BIB.dll を見つけられない。

## 2. パス形の確定結果 (実測)

cdb (ntdll!NtCreateFile / NtOpenFile ブレーク) で trusted worker 直下の
PARAMS_SETUP を計測した。AddGrain.aex は

```
\??\C:\Users\<user>\AppData\Local\Temp\aexcompat-sealed-<id>\Film Stocks\*.*
\??\C:\...\aexcompat-sealed-<id>\Film Stocks\100T (5247).grain
```

のように **プラグインのモジュール dir 直下の `Film Stocks\`** を参照する
(CWD 基準ではない。repository root に `Film Stocks` を置いても成功しないことを
別途確認済み)。偽 sealed root に `Film Stocks/` をコピーしたところ
`params_setup_error: 0` / `reported_num_params: 72` /
`status: "parameters_inspected"` となり、**sealed root 内のサブディレクトリ
配置で成功することが実証済み**である。

## 3. 設計: sealed data resource (role 3)

sealed load tree の manifest エントリに、既存の role 0 (main plugin) /
role 1 (dependency) / role 2 (cluster plugin) に加えて **role 3 (data
resource)** を設ける。

- manifest 宣言は broker が行い、SHA-256 + size 認証済みの実ファイルだけを
  封入する (DLL 封入と同一規則: サイズ + SHA-256 の再照合、reparse point 拒否、
  直子以外からの staging 拒否、staging 後の retained handle 再ハッシュ)。
- tree 内配置は `<root>/<subdir>/<basename>`。v1 では **1 段のサブディレクトリ
  のみ**許可する (Film Stocks 型)。`relative_path` は
  `<subdir>/<basename>` 形式で、各コンポーネントに既存の Windows-safe
  basename 規則 (`session_dependency_manifest::validate_windows_basename`)
  をそのまま適用する。セパレータ・ドライブ・`..`・reparse point は拒否。
- staging 先 subdir は root 直下にのみ作成し、作成後に reparse でないことを
  検証する。
- **ACL**: データファイルには既存の child DACL (worker SID に read のみ、
  replace 不可)、subdir には root と同じ directory DACL (read+traverse、
  子作成拒否) を適用する。つまり staging・hash・ACL の強度は DLL と同一。
- **manifest digest**: data エントリは role byte + relative_path バイト列で
  digest に含める。data エントリの無い tree の digest は従来と 1 バイトも
  変わらない (後方互換)。
- **audit への影響**: module audit はロードされた実行イメージだけを見るので、
  データファイルは audit 契約に影響しない。subdir 内のファイルを
  `LoadLibrary("subdir\\evil.dll")` のような相対指定でロードしようとした
  場合、そのモジュールの親 dir は sealed root 直下ではないため **plugin
  クラスに分類されず unknown として audit が fail-closed する**。よって
  subdir への PE イメージ混入は audit がバックストップとして拒否する
  (broker は内容で PE 判定しない)。

## 4. 宣言の集め方 (bridge)

- **Film Stocks 型データ**: プラグインの実 dir の直下に `Film Stocks/`
  が存在すれば、その **直下のファイルを全て** (v1 は非再帰、プラグインの
  列挙形 `Film Stocks\*.*` に一致) 、個数・合計サイズの上限
  (256 個 / 64 MiB) 付きで data resource として宣言する。上限超過は
  fail-closed (そのプラグインの discovery 失敗として構造化。黙って一部だけ
  封入しない)。
- **BIB.dll (host 常設 DLL)**: closure が BIB.dll を含まない場合に限り、
  プラグインの dir または依存検索 root にある BIB.dll を dependency として
  1 件追加する。「BIB suite を要求するプラグインだけ」は静的に判定できない
  (#485 のロード自体が遅延・要求駆動) ため、存在すれば常に足す設計とする
  (封入されるだけで、要求されなければロードされない)。closure が既に
  BIB.dll を含む場合は重複 basename として sealed tree が fail するため、
  追加しない (case-insensitive で判定)。
- 宣言の信頼境界は従来どおり: launch 前に broker が全エントリの
  パス・サイズ・SHA-256 を確定し、起動後に信頼を変えない。

## 5. スコープ外

- render セッション経路へのデータ resource 展開 (discovery/inspect 経路から
  開始。render で AddGrain 系を使う場合のフォローアップ)。
- cluster セッションの manifest (`cluster-manifest-v1`) への data resource
  宣言 (監査の宣言集合は module だけを数えるため監査契約上は不要だが、
  cluster sealed tree への staging 展開は別途検討)。
- 多段サブディレクトリ、再帰的なデータ列挙 (v1 は 1 段のみ)。
