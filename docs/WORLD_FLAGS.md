# PF world flags (#1728)

AE 25.1 CPU/Software の同一 capture AEX による T25 oracle は、SmartFX
input/output と同深度の `PF_NewWorld(3, 2, TRUE, format)` で次を観測した。

| bpc | world_flags |
|---:|---:|
| 8 | `0x00000000` |
| 16 | `0x01000001` |
| 32 | `0x02000000` |

根拠: [SmartFX 記録](https://github.com/onmokoworks/AEXCompat/issues/1728#issuecomment-6465590662)、
[PF_NewWorld 記録](https://github.com/onmokoworks/AEXCompat/issues/1728#issuecomment-6465885483)。
capture AEX SHA-256: `8fb1dca410861089d44bdc182dbc3780cd3ebb7f841eb44b17f896834cd6de39`。

native の layout publisher、PF/AEGP allocator、AEGP receipt と GPU world、
guest の frame/PF/GPU world、native carrier の PF allocator を同じ深度別の値にする。
Classic、AEGP、GPU、cross-depth allocation へ適用するのは共通 PF world 表現に
基づく実装上の判断であり、それらの経路の AE 直接観測を追加したという意味ではない。
borrowed world や plug-in が作る world の flags を一律に書き換えない。

登録済み world と owned AEGP projection は従来どおり登録形式を優先する。
fill/premultiply が既存の bounded fallback で扱う未登録 view は、session depth
ではなく world 自身の RESERVED1/DEEP を使う。両 bit が立つ曖昧な fallback は拒否する。
float の DEEP が clear でも、8/16-bit の typed fallback に入れない。
registry の exact reference/layout、所有権、lifetime、stride/geometry 検証は維持する。

behavioral self-test は実 descriptor の flags、各 session 深度における padded/offset
view の fill と premultiply の全 byte、保護領域、不正 geometry の拒否、allocate/dispose
を検査する。guest の callback テストは各深度と clear 有無を実行し、DEEP 優先の
plug-in 型判定で選んだ画素サイズが allocation と一致することも検査する。

PF_NewWorld rowbytes は今回変更しない。AE の既存データは3×2で各深度64だけであり、
64-byte alignment と minimum stride を区別できない。
[追加 oracle の条件](https://github.com/onmokoworks/AEXCompat/issues/1728#issuecomment-6496971698)
として、各深度の64-byte境界直前/一致/直後、複数 height、clear 有無、cross-depth を残す。
