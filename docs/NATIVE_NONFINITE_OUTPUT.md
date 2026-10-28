# Native 32 bpc出力の非有限値（#1805）

SmartFX finalizeはNaN/Infを持つ32 bpc出力を拒否せず、packed worldとsession slotへ
全wordを保持する。NaNのpayload/sign、signalling/quiet区別、±0を変更しない。
プラグイン名・hashによる分岐や許容差はない。

- `output_pixels_valid`: 出力の存在、約束領域の書込coverage、および輸送可能性。
  32 bpcでは有限性を含まない。guard、extent、selector結果、所有権検査は従来通り別途適用。
- `output_samples_finite`: finalizeで実際に検査した内容の診断。NaN/Infがあればfalse。
  出力なしは有限性に反するsampleなしとしてtrue。finalize到達前の値は描画成功を意味しない。
  brokerの公開image reportにも伝播する。旧workerでfieldが無ければnullで、trueを捏造しない。
- Premiere GPUのfloat出力を8/16 bpcへ狭める経路では非有限wordが保存できないため、
  `output_non_finite`による拒否を維持し、有限性診断もfalseにする。

適用はSmartFXの共通finalizeとimage report。harness effect-chainの有限出力契約、
整数変換・previewの契約を変更しない。完全一致の証拠にはnative rawを使う。

背景: 保存AE CPU/Software Tag2には非有限wordが残るが、従来はfinalizeの有限性要件で
5ケースの出力比較ができなかった。今回の変更は輸送を可能にするもので、AE一致自体の保証ではない。
ケースごとの完全一致・差数・最大有限ULP・firstはolm-win-checksのT23に記録する。

検証: `aex_worker --kind smart --self-test-nonfinite-output` は本番finalizeに
padded worldを渡し、全word保持、session用の並べ替え、非有限診断、empty passthrough、
selectorエラー保持、guard破損検知、不正stride/寸法拒否、全深度の未書込拒否と有限出力成功、
8/16 bpcへの非有限narrowing拒否を確認する。broker unit testは公開reportへの診断伝播を確認する。
