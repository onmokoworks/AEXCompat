# BIB resolver / PICA component init (issue #362, PP・ProfileToProfile) 2026-07-25

> **2026-08-18 追記 (issue #1279)**: この文書の「残課題」2 件
> (PP の U.dll SPBasicSuite、P2P の ACE CMM) は解決した。前者は U.dll の
> export `U_SP_Birth`、後者は `COR.dll` の `COR_Conception` で host 内に
> 閉じる。「実 AE の PICA ブートストラップ / CMM 環境が必要」という結論と、
> 「ae_sweetpea を `SPInit(nullptr,nullptr,0)` + `SPStartupPlugins` で
> 直接起動する」「`ensure_pica_components_initialized` は 1 プロセス 1 回」
> という実装の記述は、いずれも現在のコードでは古い。正は
> `docs/SUPPORT_LIBRARY_BIRTH_SEQUENCE_2026-08-18.md`。

対象: selector families の残件 **Particle_Playground** (GLOBAL_SETUP error=11) と
**ProfileToProfile** (GLOBAL_SETUP error=14)。docs/SELECTOR_FAMILIES_2026-07-25.md
の「BIB resolver 経由の proc 不足が疑われる、未確定」を確定させた slice。

## 特定された失敗点

### ProfileToProfile (error=14)

1. resolver trace (後述の proxy) で、BIB core の解決は全て成功するが
   **`ACEInterface2.GetMatrixRGBtoRGBOverRangeShaderCount` が NULL** で即失敗する
   ことを特定。ACE.dll は `ACEInitialize`/`ACEInitDelayed` 系の export を持ち、
   裸の `ACEInitialize(0,0)` は ACE 内部の host 提供テーブル未設定で AV になる。
2. 正規の初期化経路は **dvabravoinitializer.dll の `InitBravoComponents`**:
   `SetBIBProcAddress(resolver)` で BIB resolver を渡してから呼ぶと
   (dvacore の ExecuteTopLevelFunction 経由で) ACE が正しく登録される。
   これにより ACEInterface2 は全 205 proc 解決可能になり、プラグイン自身の
   `ACEInitializeEx` も成功する (戻り値 1 = ACE 流の true)。
3. 残ブロック: その後の ACE/CMM 使用部
   (DefaultCallbacks → CMM パス系) で error=14。実 AE 側の CMM 環境
   (AfterFXLib 提供のコールバック/パス) が必要で、fail-closed を緩めずには
   これ以上進めない。

### Particle_Playground (error=11)

1. resolver trace では BIB core (Container/Error/String/Memory) まで全成功。
   BIBMemAllocProc の往復も probe で正常確認 (allocator 仮説は否定)。
2. GLOBAL_SETUP の disasm 追跡で、BM_Birth・extended_alloc・new_handle・
   フラグ設定を経た後、**`U_SP_GetSPBasicSuite(&out)` が 11 (0xB) を返す**
   地点を特定 (Particle_Playground+0xc8bf)。
3. 原因: U.dll の SPBasicSuite ポインタ大域 (T_G_zref+0x4ea8) が未設定。
   U.dll は「タグ一致するインターフェースの BIB 登録」を契機にこれを
   インストールする (U+0x54e0 周辺) が、その登録は実 AE の PICA
   ブートストラップ (AfterFXLib/AdobePIE 側) が行う。BIBInitialize4 の
   null コールバック fallback では発生しない。SP suite 登録
   (ae_sweetpea SPInit+SPStartupPlugins) とは別の層で、fail-closed を
   緩めずには進めない。

## 対応内容 (worker 側の本実装)

`minihost/src/worker_host_suite_wiring.cpp`:

- **BIB resolver trace proxy** (診断): `AEXCOMPAT_EXTENDED_DIAG=1` のとき
  suite[0] が返す resolver を proxy 化し、(interface, procedure, signature)
  と解決結果を全記録。変数未設定時は従来どおり生の resolver (無変更)。
- **PICA component 初期化** (`ensure_pica_components_initialized`):
  provide_bib_suite 成功後 (BIB mutex の外) に 1 プロセス 1 回、
  dvabravoinitializer.dll (SetBIBProcAddress → InitBravoComponents) と
  ae_sweetpea.dll (SPInit(nullptr,nullptr,0) → SPStartupPlugins) を
  SEH ガード付きで初期化。ロードは admitted プラグインの自ディレクトリ
  (DLL_LOAD_DIR|SYSTEM32) に限定。
- **逆順 teardown**: `std::atexit` に SPShutdownPlugins → SPTerm →
  TerminateBravoComponents(false) を登録。EXE の atexit チェーンで
  コンポーネント群の DllMain detach より先に実行され、dvacore の
  shutdown fast-fail (int 3) を防ぐ。teardown 前に cout/stdout を
  flush する (CRT の flush ハンドラより先に teardown が走り、
  レポート末尾が欠けるのを防ぐ)。
- **BIB memory probe** (診断): Alloc/Free の roundtrip を env-gated で記録。

## 検証

- minihost Release build (vcvars + cmake、-DCMAKE_LINKER=link /
  -DCMAKE_AR=MSVC lib.exe 明示): OK。
- 2 件の最終状態: PP gs=11 / P2P gs=14 (どちらもプロセスは clean 終了、
  最終ブロック点は上記のとおり確定)。P2P は ACEInterface2 全解決 +
  プラグインの ACEInitializeEx 成功まで到達。
- 回帰: VRGlow / Curves / OCIOColorSpaceTransform / Scribble / AddGrain
  全て parameters_inspected (AddGrain は staging 側の Film Stocks が条件、
  従来どおり)。Scribble の shutdown クラッシュ (dvacore fast-fail) と
  8192 バイト出力途切れは逆順 teardown + flush で解消。
- selftest: 5 exe + --self-test-compute-cache 全 pass。
- pytest: BIB 契約 (Bravo/sweetpea 契約を追加) ほか 25 passed。
- broker cargo test --workspace: OK。

## 残課題

- PP: U.dll の SPBasicSuite グローバルを立てるには実 AE の PICA
  ブートストラップ (AdobePIE コールバック経由の BIB 初期化) が必要。
  BIBInitialize4 の 8 コールバックの意味を実測で特定する follow-up。
- P2P: ACE CMM 使用部 (DefaultCallbacks/CMM パス) の失敗点の確定。
  ACE_GetDirectExternalCMMPath 系の期待するパス/コールバックの調査。
- これらは「成功の捏造」をすれば見かけ上通るが、fail-closed 方針では
  進めない領域。
