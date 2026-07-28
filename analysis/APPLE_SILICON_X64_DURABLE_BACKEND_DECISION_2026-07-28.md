# Apple Silicon x64 durable backend decision (2026-07-28)

## Decision

2026-07-28 時点では、第三の製品backendを実装しない。

- arm64のUnicorn workerを、Rosettaに依存しないdurable correctness backendとする。
- x86_64 native carrierは、一般用途Rosettaを利用できる間のopt-in acceleratorとして扱う。
  Appleの公表上、その一般提供はmacOS 27までだが、carrier自体を将来のmacOS 27で
  検証済みという意味ではない。
- Rosettaを利用できない場合、native carrierの速度は失われるが、arm64 Unicornによる
  Win64 AEX実行経路は残る。ただし、その経路も対応ISA、host contract、memory budget、
  timeoutを含む現在のfail-closed resource contractの範囲内である。
- 第三backendは、下記の再評価triggerが成立した後にbounded spikeを行い、一方式だけを選ぶ。

これは「Rosetta終了まで何もしない」という判断ではない。Rosetta終了後にも残る
arm64実行経路を正本とし、新しい実行engineを互換性の根拠なく増やさないという判断である。

## Appleの公開制約

Appleは、一般用途のIntel app向けRosettaをmacOS 27まで提供し、それ以降は一部の
古い未保守gaming title向け機能だけを残す予定だと公表している。

- [Using Intel-based apps on a Mac with Apple silicon](https://support.apple.com/en-us/102527)
  (published 2026-02-16)
- [About the Rosetta translation environment](https://developer.apple.com/documentation/Apple-Silicon/about-the-rosetta-translation-environment)

Appleの公開仕様上、arm64 codeとx86_64 codeを同一processへ混在させることはできない。
現在のarm64 harnessから独立したx86_64 workerを起動する構成はこの境界に適合する。
RosettaはAVX/AVX2を変換するがAVX512は変換しない。

したがって、native carrierをmacOS 28以降の一般用途backendとして扱うことはできない。
一方、arm64 process内でWin64 PEをmappingしてCPU命令をemulateするUnicorn workerは、
このRosetta提供期限には依存しない。

## Current repository contract

現在の正本は次の通りである。

1. `guest/crates/aex-guest-worker/src/lib.rs`
   - default buildはUnicorn実装をexportする。
   - `native-carrier` featureのmacOS buildだけが別のnative実装をexportする。
2. `broker/crates/harness/src/macos.rs`
   - native carrierは`AEXCOMPAT_NATIVE_CARRIER=1`のときだけ先行候補になる。
   - 通常はarm64 Unicorn Release、arm64 Unicorn debugの順で候補になる。
   - one-shot setupおよびresident admissionでは、nativeのlaunch error、非zero終了、
     signal、deadline超過後に次のUnicorn候補へ進む。
   - admission済みresidentの後続frame失敗は、そのframeをUnicornへ自動replayせず、
     sessionを閉じて失敗を報告する。次のuser renderでは新しいsessionを開始できる。
3. `guest/crates/aex-guest-worker/src/native_x64.rs`
   - native carrierは速度用の限定経路であり、DllMain、SEH、import、callbackの全互換を
     持つbackendではない。
   - `_CxxThrowException`、未実装`_vcomp_*`、MSVCのUDT-by-value return
     （hidden sret）はguestへ入る前に拒否し、Unicornを案内する。
   - 未分類のscalar-return importは現在もzero-return stubを使用するため、未分類importを
     全面的に安全だと保証する契約ではない。
4. `guest/crates/aex-guest-worker/src/x64.rs`
   - PE mapping、Win64 ABI、Suite/callback、resident session、structured diagnosticsを
     arm64 process内で提供する。
   - 未対応命令、import、callback、selectorは推測で成功へ丸めずfail closedする。

`AEXCOMPAT_GUEST_WORKER`は一つのworkerを明示するexpert overrideであり、自動fallbackや
native deadlineを提供する安全な製品routingではない。

## Evidence at this decision point

### Functional durability

PR #586のSHA固定5-entry runでは、通常のAE CPU entryをUnicornで実行し、次を確認した。

- Fractal NoiseとVideo Limiterはrender成功。
- Box BlurとDirectional BlurはAdobe private `FLT Blur Suite v1`境界まで進み、
  structured selector errorで停止。
- TransformはAdobe private dvacore C++ object-return importをstructured import errorとして
  停止し、未初期化hidden-sretによるmemory crashを起こさない。
- crashは0件。

同じheadのfrozen24では23 render / 1 structured selectorとなり、既存23 renderのraw
outputは全件byte-identical、session cleanupも維持された。

この証拠はUnicornが任意のAEXをすでに完全対応することを意味しない。ただし、Rosettaが
なくなっただけでAEXCompatの実行backendが消える、という主張を否定するには十分である。

### Performance

既存の
`analysis/APPLE_SILICON_X64_NATIVE_CARRIER_RESULT_2026-07-25.md`
では、一つのOLM Full-HD fixtureについて次が記録されている。

- native carrier P95: default 0.853秒、Amount=50 0.656秒
- historical Unicorn P95: default 13.707秒、Amount=50 60.287秒
- small fixtureとFull-HDでoutputはexact

速度差は大きい。ただし一つのprivate effectと二つのparameter設定だけでは、第三の汎用
backendを採用する根拠にならない。現時点で不足しているdurabilityは「実行可能性」ではなく、
Rosetta終了後のinteractive performanceである。

resident admissionは現在、実際の出力解像度でdefault renderを行い、30秒の
`RESIDENT_RENDER_DEADLINE`内にreadyになることを要求する。admission probeに成功したworkerは
閉じられ、実session用のfresh workerが再起動されるため、最初のuser frameまでに少なくとも
probe renderと本renderの双方を支払う。既存の13.707秒/60.287秒は単一renderのP95であり、
Rosettaなしのsession startup costや、30秒を超えるeffectの利用可能性を保証しない。

## Re-evaluation triggers

次のどれかが成立したら、この判断を再評価する。trigger成立は製品backendの即採用ではなく、
一方式に限定したbounded spikeの開始条件である。

1. macOS 28 betaまたはfinalのclean environmentで、x86_64 native carrierを起動できない。
2. supported OSまたは製品配布方針がRosettaの導入、利用、MDM許可を前提にできない。
3. 無関係なSHA固定AEXが2本以上、Suite/import/host contractではなくUnicornのCPU実行意味論を
   原因として失敗し、一つの候補engineが両方を解消できる。
4. representative workflowについて数値化されたlatency SLOをUnicornが満たさず、
   Rosetta native laneも利用できない。residentでは、現在の30秒admission deadline、
   probeとfresh sessionの二重render、steady frameを分けて測る。
5. Developer ID、Hardened Runtime、notarizationを含む配布artifactで、既存backendを
   許容可能なentitlementとprocess isolationのまま出荷できない。
6. 実corpusでAVX512などRosetta非対応ISAが必要になる。この場合は下記候補を機械的に
   選ばず、まずUnicorn側ISA拡張またはAVX512を実際に実行できる一方式をbounded spikeで
   立証する。
7. arm64 Unicorn worker自体がsupported toolchainまたはOSでbuild/runできなくなる。

Appleの告知だけは第三backend実装のtriggerにしない。告知された将来の機能継続は既存の
arm64 Unicornが担保しているためである。

## Adoption gates for one future backend

将来の候補を製品backendへ採用するには、少なくとも次をすべて満たす。

### Compatibility

- WindowsのSHA固定manifestと現在のMac frozen corpusを再利用する。
- nativeとUnicornの成功集合を減らさない。
- deterministic renderはUnicornとbyte-identicalにする。
- Classic、SmartFX、PluginData、ARGB8/16/32Fを含む。
- one-shot、resident、parameter変更、連続frame、cleanupを含む。
- crash 0、guards intact、timeout/import/callback/return sentinelはfail closedにする。
- 少なくとも異なるsupplierまたは無関係なAEXを2本以上含む。

### Performance

- cold startとresident steady stateを分離して報告する。
- 10-run nearest-rank P95を用いる。
- OLM Full-HD defaultとAmount=50は各1.0秒以下、かつ現在のnative referenceの2倍以内を
  adoption goalとする。
- bounded spikeの継続条件は、effect名、SHA、RVAに特化せずUnicorn比3倍以上とする。

### Distribution and maintenance

- Rosettaのないarm64 environmentで動作する。
- pinnedかつ再現可能なsourceからbuildできる。
- GPL-2.0-only workerとの同梱がlicense上成立する。
- private Apple APIを使わない。
- signed、Hardened Runtime、notarized bundleで動作する。
- JITを使う場合は`MAP_JIT`、write-protect、必要entitlementを配布artifactで検証する。
- arm64 UnicornもTCG code generationを行うため、既存backendであっても同じJIT配布gateを
  実際のartifactで通す。
- runtime downloadを必須にせず、依存物とpackage sizeをboundedにする。
- 現在のprocess isolationとdeadlineより弱くしない。

## Ranked future options

### 1. Generic hot-trace arm64 JIT with Unicorn fallback

最も既存構造へ段階導入しやすい候補である。iced-x86 decode、guest state、memory、host
callback境界を再利用し、対応できるhot extentだけをarm64へcompileする。未対応traceは
開始前または明示的なstate handoffでUnicornへ戻す。

採用前に、異なる実AEX 3本以上でdynamic instructionの集中または共通opcode familyを確認する。
effect名、SHA、RVA、既知のalgorithmを埋め込まない。最初のspikeは製品backendではない。

### 2. FEXCore

CPU core、code cache、AVX対応には魅力があるが、upstreamの主対象はArm64 Linux usermodeであり、
Darwin、Mach exception、executable memory、callback integration、現行PE hostとの接合が大きい。
単一Win64 callback round-tripと配布可能buildを示すbounded feasibility testなしには採用しない。

### 3. Windows ARM VM or remote Windows executor

Windows自身のx64 translationとWin APIを使えるため互換性oracleまたは救済経路としては強い。
ただしWindows license、VM容量、起動、GPU、file sharing、offline UXが重く、Mac内蔵backendの
置換にはしない。

whole-PE static lifting、Wasm/LLVMへの全面変換、独自interpreter、effect別Rust/Metal再実装は、
indirect call、SEH、TLS、runtime code、callback、保守コストのため現時点の候補にしない。

## Distribution caveat

現在のrepositoryにはmacOS向けnotarized product packagingの正本がない。native carrierはPE sectionを
anonymous mappingへcopyして実行権限を付与するため、開発用実機で動くこととDeveloper ID +
Hardened Runtimeで配布できることは同一ではない。

notarized配布を始める際は、helperを内側から個別署名し、実際の配布artifactでnative mapping、
Win64 callback、実AEX、fallbackを再検証する。`disable-library-validation`などのentitlementを
根拠なく追加しない。durable側のarm64 UnicornもTCG JITを使用するため、`MAP_JIT`、
`com.apple.security.cs.allow-jit`、thread-local write protectionを含む実配布条件を別途検証する。

worker crateは`GPL-2.0-only`であり、配布時には対応するsource提供とlicense textを含む
GPL-2.0の義務をpackage設計へ組み込む。

## Reproduction record

#585/#586の5-entry結果は64×64 ARGB8で取得した。

- input PNG SHA-256:
  `794c162146e5e94d97ab30863d69bb588441a855b2bab2447e69e7d191c14cb1`
- Windows inventory SHA-256:
  `8b6901084f9dd8d5c2eb8b88f6b877e51bbded86c13b834aa5c7ef20c41089f1`
- Windows summary SHA-256:
  `78d2f2c7ee638b65373c01bf8696b2b19fd83001954515e98a04faba87f99e1a`
- final five-entry Unicorn worker SHA-256:
  `397a53524fb58473318ffa822670df29b62b43ae9510aa8f3af60bdd1bfee61e`
- final five-entry native worker SHA-256:
  `d5476c0d725b5289127c4907bf0499e16351b6140b71c64082f6a4b1095d5e70`
- sweep result: 2 rendered / 2 structured selector / 1 structured import / 0 crash

使用した正本toolは`tools/sweep_macos_x64_aex.py`である。private AEXとlocal evidence pathは
repositoryへcommitしない。Full-HD性能値は上記のnative-carrier result recordにある
10-run commandとfixture SHAを正本とし、この64×64 corpus結果からFull-HD性能を推論しない。
同toolの`--backend unicorn`はnative workerをresolve、hash、launchせず実行できるため、
Rosettaを利用できない将来のclean environmentでもdurable laneだけを再検証できる。

## Next action

第三backendは追加しない。現在のdefault-Unicorn / opt-in-native /
native setup・admission-failure fallback契約を維持し、macOS 28 beta、
notarized distribution、または上記のcorpus/performance triggerが成立した時点で、
この記録を入力として一方式だけ再評価する。

一般用途Rosettaが残る間は、両backendが同じ実AEXをrenderできる交差集合について、
同一入力・parameter・pixel formatのoutput SHAをsweepで保存する。nativeでdispatch不能な
AEXを成功数のために弱いstubへ通さない。このoracle captureは第三backend実装ではなく、
将来失われる独立x86 execution evidenceの保存である。
