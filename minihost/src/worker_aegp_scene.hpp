#pragma once

// Ownership boundary for the clean-room AEGP scene family.
//
// The implementation deliberately has no public SDK-shaped API. Suite tables
// remain private to the worker and are leased through the existing BasicSuite
// adapter in l2_main.cpp. This header is therefore a boundary marker rather
// than an ABI surface; adding declarations here requires an explicit review of
// calling convention, handle provenance, and lifetime semantics.
inline constexpr unsigned kAegpSceneEffectInstanceLimit = 8;
inline constexpr unsigned kAegpSceneEffectLeaseLimit = 16;
inline constexpr unsigned kAegpSceneLegacyEffectStreamLimit = 16;
