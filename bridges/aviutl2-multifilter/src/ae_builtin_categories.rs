// Menu categories for After Effects' own bundled effects (issue #876).
//
// AE's `Support Files\Plug-ins\Effects\*.aex` carry no PiPL resource at all
// (measured: VERSION + MANIFEST only) — their menu categories live in AE's
// internal database, not in the files — so #871's PiPL parse leaves them
// uncategorized. This table maps their file stems to the canonical English
// AE menu category. Display metadata only: it never identifies, gates, or
// enforces anything, and a wrong row costs a cosmetic menu position.
//
// Verified against a real AE 2026 install: an ExtendScript dump of
// `app.effects` (displayName / matchName / category) was joined to these
// stems by display name — 151 rows matched that oracle exactly and 10 were
// corrected from it. Rows marked "uncertain" could not be joined (hidden or
// ambiguously named entries) and stay best-effort; correct them freely.

/// File stem (matched case-insensitively) → canonical English AE category.
static AE_BUILTIN_CATEGORIES: &[(&str, &str)] = &[
    ("3DGlasses", "Perspective"),
    ("AddGrain", "Noise & Grain"),
    ("Alpha_Levels", "Obsolete"),
    ("ApplyColorLUT", "Utility"),
    ("Arithmetic", "Channel"),
    ("AudSpect", "Generate"),
    ("Aud_BT", "Audio"),
    ("Aud_Compressor", "Audio"),
    ("Aud_Delay", "Audio"),
    ("Aud_Distortion", "Audio"),
    ("Aud_Flange", "Audio"),
    ("Aud_Gate", "Audio"),
    ("Aud_Mixer", "Audio"),
    ("Aud_Modulator", "Audio"),
    ("Aud_ParamEQ", "Audio"),
    ("Aud_Reverb", "Audio"),
    ("Aud_Tone", "Audio"),
    ("AutoColor", "Color Correction"),
    ("AutoContrast", "Color Correction"),
    ("AutoLevels", "Color Correction"),
    ("Aux_Channel_Extract", "3D Channel"), // uncertain
    ("B-C", "Color Correction"),
    ("Basic_3D", "Obsolete"),
    ("Beam", "Generate"),
    ("Bevel_Alpha", "Perspective"),
    ("Bevel_Edges", "Perspective"),
    ("BezWarp_New", "Distort"),
    ("Bilateral", "Blur & Sharpen"),
    ("Blend", "Channel"),
    ("Block_Dissolve", "Transition"),
    ("Broadcast_Colors", "Color Correction"),
    ("Brush_Strokes", "Stylize"),
    ("Bulge", "Distort"),
    ("Calculations", "Channel"),
    ("CannedWarp", "Distort"),
    ("Card Dance", "Simulation"),
    ("Card Wipe", "Transition"),
    ("Caustics", "Simulation"),
    ("CellPattern", "Generate"),
    ("ChangeToColor", "Color Correction"),
    ("Change_Color", "Color Correction"),
    ("ChannelCombiner", "Channel"),
    ("Channel_Blur", "Blur & Sharpen"),
    ("Channel_Mixer", "Color Correction"),
    ("CineonEffect", "Utility"),
    ("Circle", "Generate"),
    ("ColorAndContrast", "Color Correction"), // uncertain
    ("ColorLink", "Color Correction"),
    ("ColorShift", "Color Correction"), // uncertain
    ("ColorTexture", "Obsolete"),       // uncertain
    ("Color_Balance", "Color Correction"),
    ("Color_Diff", "Keying"),
    ("Color_Emboss", "Stylize"),
    ("Color_HLS", "Color Correction"),
    ("Color_Key", "Obsolete"),
    ("Color_Range", "Keying"),
    ("Colorama", "Color Correction"),
    ("ColorsQuad", "Generate"),
    ("Compound_Blur", "Blur & Sharpen"),
    ("Contrast", "Color Correction"), // uncertain
    ("Curl_Noise", "Noise & Grain"),
    ("Curves", "Color Correction"),
    ("Deflicker", "Utility"), // uncertain
    ("Depth_Field", "3D Channel"),
    ("Depth_Matte", "3D Channel"),
    ("Difference", "Channel"), // uncertain
    ("DirectionalBlur", "Blur & Sharpen"),
    ("Displacement", "Distort"),
    ("Drop_Shadow", "Perspective"),
    ("Dust", "Noise & Grain"),
    ("Echo", "Time"),
    ("Ellipse", "Generate"),
    ("Equalize", "Color Correction"),
    ("Exposure", "Color Correction"),
    ("ExpressionControls", "Expression Controls"),
    ("Extract", "Keying"),
    ("EyedropperFill", "Generate"),
    ("Fill", "Generate"),
    ("Flare", "Generate"),
    ("Foam", "Simulation"),
    ("Fog_3d", "3D Channel"),
    ("Fractal", "Generate"),
    ("FractalNoise", "Noise & Grain"),
    ("Gaussian_Blur", "Blur & Sharpen"),
    ("Gaussian_Blur_MC", "Blur & Sharpen"),
    ("Gpg", "Color Correction"),
    ("Gradient_Wipe", "Transition"),
    ("Grid", "Generate"),
    ("Grow_Bounds", "Utility"),
    ("Hue_Sat", "Color Correction"),
    ("ID_Matte", "3D Channel"),
    ("Invert", "Channel"),
    ("Iris_Wipe", "Transition"),
    ("KeyCleaner", "Keying"),
    ("Leave_Color", "Color Correction"),
    ("Lens_Flare", "Generate"),
    ("Levels2", "Color Correction"),
    ("Lightning", "Generate"),
    ("Linear_CK", "Keying"),
    ("Linear_Wipe", "Transition"),
    ("Liquify", "Distort"),
    ("Luma_Key", "Obsolete"),
    ("Lumetri", "Color Correction"),
    ("Magnify", "Distort"),
    ("MatchGrain", "Noise & Grain"),
    ("Median", "Noise & Grain"),
    ("Minimax", "Channel"),
    ("Mirror", "Distort"),
    ("Mosaic", "Stylize"),
    ("MshWrp_New", "Distort"),
    ("Noise", "Noise & Grain"),
    ("NoiseHLS", "Noise & Grain"),
    ("NoiseHLSAuto", "Noise & Grain"),
    ("Numbers", "Text"),
    ("OCIOCDLTransform", "Color Correction"),
    ("OCIOColorSpaceTransform", "Color Correction"),
    ("OCIODisplayTransform", "Color Correction"),
    ("OCIOFileTransform", "Color Correction"),
    ("Offset", "Distort"),
    ("OpticsComp", "Distort"),
    ("PS_Arb_Map", "Obsolete"),
    ("PaintBucket", "Generate"),
    ("Path_Text", "Obsolete"),
    ("Photo Filter", "Color Correction"),
    ("Polar", "Distort"),
    ("Posterize_Time", "Time"),
    ("RadialShadow", "Perspective"),
    ("Radial_Blur", "Blur & Sharpen"),
    ("Radial_Wipe", "Transition"),
    ("Radio_Waves", "Generate"),
    ("Ramp", "Generate"),
    ("Reshape_New", "Distort"),
    ("Ripple", "Distort"),
    ("RollingShutter", "Distort"),
    ("RoughenEdges", "Stylize"),
    ("Scatter", "Stylize"),
    ("Scribble", "Generate"),
    ("Set_Channels", "Channel"),
    ("ShadowHighlight", "Color Correction"),
    ("ShapeBlur", "Blur & Sharpen"),
    ("Shatter", "Simulation"),
    ("Shift_Channels", "Channel"),
    ("Simple_Choker", "Matte"),
    ("SmartBlur", "Blur & Sharpen"),
    ("SolidComposite", "Channel"),
    ("Spherize", "Distort"),
    ("Spill", "Obsolete"),
    ("Spill2", "Keying"),
    ("Strobe_Light", "Stylize"),
    ("Stroke", "Generate"),
    ("Three_Way_Color_Corrector", "Obsolete"),
    ("Threshold", "Stylize"),
    ("Tile", "Stylize"),
    ("Time_Displace", "Time"),
    ("Timecode", "Text"),
    ("Tint", "Color Correction"),
    ("Transform", "Distort"),
    ("TurbulentDisplace", "Distort"),
    ("TurbulentNoise", "Noise & Grain"),
    ("Twirl", "Distort"),
    ("Unmult", "Channel"),
    ("Unmultiply", "Channel"),
    ("Unsharp_Mask", "Blur & Sharpen"),
    ("VRConverter", "Immersive Video"),
    ("VRPlaneToSphere", "Immersive Video"),
    ("VRSphereToPlane", "Immersive Video"),
    ("Venetian_Blinds", "Transition"),
    ("VideoLimiter", "Color Correction"),
    ("Wave World", "Simulation"),
    ("Wave_Warp", "Distort"),
    ("Write_on", "Generate"),
];

/// The canonical AE category for a bundled effect's file stem, or `None`
/// for anything the table does not know (then the bare brand label applies,
/// exactly as for any other PiPL-less plug-in).
fn ae_builtin_category(stem: &str) -> Option<&'static str> {
    AE_BUILTIN_CATEGORIES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(stem))
        .map(|(_, category)| *category)
}

/// Adobe's own Japanese names for the standard AE menu categories, so the
/// menu reads in one language whichever source (PiPL or the table above) a
/// category came from. A category outside this set — a third-party pack's
/// own name — is shown as recorded.
static STANDARD_CATEGORY_JA: &[(&str, &str)] = &[
    ("3D Channel", "3Dチャンネル"),
    ("Audio", "オーディオ"),
    ("Blur & Sharpen", "ブラー＆シャープ"),
    ("Channel", "チャンネル"),
    ("Color Correction", "カラー補正"),
    ("Distort", "ディストーション"),
    ("Expression Controls", "エクスプレッション制御"),
    ("Generate", "描画"),
    ("Immersive Video", "イマーシブビデオ"),
    ("Keying", "キーイング"),
    ("Matte", "マット"),
    ("Noise & Grain", "ノイズ＆グレイン"),
    ("Obsolete", "旧バージョン"),
    ("Perspective", "遠近"),
    ("Simulation", "シミュレーション"),
    ("Stylize", "スタイライズ"),
    ("Text", "テキスト"),
    ("Time", "時間"),
    ("Transition", "トランジション"),
    ("Utility", "ユーティリティ"),
    ("Video", "ビデオ"),
];

/// The display form of a category under the configured category language
/// (issue #876): Japanese swaps standard AE categories for Adobe's own
/// Japanese names; English (or an unknown category) passes through.
fn localized_category(canonical: &str, japanese: bool) -> &str {
    if !japanese {
        return canonical;
    }
    STANDARD_CATEGORY_JA
        .iter()
        .find(|(english, _)| english.eq_ignore_ascii_case(canonical))
        .map(|(_, ja)| *ja)
        .unwrap_or(canonical)
}
