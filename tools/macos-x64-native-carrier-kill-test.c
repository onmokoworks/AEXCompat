// Build and run on Apple Silicon:
//   clang -arch x86_64 -O2 tools/macos-x64-native-carrier-kill-test.c \
//     -o target/macos-x64-native-carrier-kill-test
//   target/macos-x64-native-carrier-kill-test
//
// This is a bounded feasibility probe for an out-of-process x86_64 carrier.
// It proves that dynamically mapped code can use the Win64 register ABI and
// round-trip through a host callback. It does not load an AEX or select Rosetta
// as the production backend.

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#if !defined(__x86_64__)
#error "build this probe for x86_64"
#endif

typedef uint64_t(__attribute__((ms_abi)) *win64_binary_fn)(uint64_t, uint64_t);
typedef uint64_t(__attribute__((ms_abi)) *win64_callback_fn)(uint64_t, uint64_t);

static uint64_t callback_calls;

static uint64_t __attribute__((ms_abi))
host_callback(uint64_t left, uint64_t right) {
  callback_calls++;
  return (left * 3) + (right * 5);
}

static int run_direct_rx_test(void) {
  // mov rax, rcx; add rax, rdx; ret
  static const uint8_t code[] = {0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3};
  const size_t page_size = (size_t)sysconf(_SC_PAGESIZE);
  void *mapping = mmap(NULL, page_size, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANON, -1, 0);
  if (mapping == MAP_FAILED) {
    fprintf(stderr, "mmap failed: %s\n", strerror(errno));
    return 1;
  }
  memcpy(mapping, code, sizeof(code));
  if (mprotect(mapping, page_size, PROT_READ | PROT_EXEC) != 0) {
    fprintf(stderr, "mprotect failed: %s\n", strerror(errno));
    munmap(mapping, page_size);
    return 1;
  }
  const uint64_t result = ((win64_binary_fn)mapping)(17, 25);
  munmap(mapping, page_size);
  if (result != 42) {
    fprintf(stderr, "direct RX result mismatch: %" PRIu64 "\n", result);
    return 1;
  }
  return 0;
}

static int run_callback_test(void) {
  // sub rsp, 40
  // mov rax, <host_callback>
  // call rax
  // add rsp, 40
  // ret
  //
  // The 40-byte adjustment provides Win64 shadow space and preserves the
  // required 16-byte alignment at the nested callback entry.
  uint8_t code[] = {
      0x48, 0x83, 0xec, 0x28, 0x48, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0,
      0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
  };
  const uint64_t callback_address = (uint64_t)(uintptr_t)host_callback;
  memcpy(&code[6], &callback_address, sizeof(callback_address));

  const size_t page_size = (size_t)sysconf(_SC_PAGESIZE);
  void *mapping = mmap(NULL, page_size, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANON, -1, 0);
  if (mapping == MAP_FAILED) {
    fprintf(stderr, "mmap failed: %s\n", strerror(errno));
    return 1;
  }
  memcpy(mapping, code, sizeof(code));
  if (mprotect(mapping, page_size, PROT_READ | PROT_EXEC) != 0) {
    fprintf(stderr, "mprotect failed: %s\n", strerror(errno));
    munmap(mapping, page_size);
    return 1;
  }
  callback_calls = 0;
  const uint64_t result = ((win64_binary_fn)mapping)(7, 11);
  munmap(mapping, page_size);
  if (result != 76 || callback_calls != 1) {
    fprintf(stderr,
            "callback mismatch: result=%" PRIu64 ", calls=%" PRIu64 "\n",
            result, callback_calls);
    return 1;
  }
  return 0;
}

int main(void) {
  if (run_direct_rx_test() != 0 || run_callback_test() != 0) {
    return 1;
  }
  puts("{\"schema\":\"aexcompat.x64-native-carrier-kill-test\","
       "\"version\":1,\"dynamic_rx\":true,\"win64_abi\":true,"
       "\"host_callback_roundtrip\":true}");
  return 0;
}
