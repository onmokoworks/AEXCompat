#pragma once
struct Packet { int version; };
inline constexpr int kVersion = 42;
int peer_version();
int peer_size();
