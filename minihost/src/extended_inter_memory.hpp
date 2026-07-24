#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::extended_inter {

// The extended inter ABI uses a pointer-to-pointer for both allocation and
// release.  Keep the ownership boundary explicit so a plug-in cannot make
// the host free an arbitrary static or foreign pointer.
int32_t __cdecl allocate(void** out, std::size_t size);
int32_t __cdecl release(void** ptr);

}  // namespace aexcompat::extended_inter
