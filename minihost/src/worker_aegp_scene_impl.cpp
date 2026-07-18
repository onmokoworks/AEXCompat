// The scene implementation owns the worker's compiled callback/table TU.
// l2_main remains the source of the worker entrypoint and shared ABI prelude.
#define AEXCOMPAT_COMPILE_AEGP_SCENE_CALLBACKS 1
#include "l2_main.cpp"
