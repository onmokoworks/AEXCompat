#include "worker_memory_fill.hpp"
#include "worker_output_coverage.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_world_registry.hpp"
#include "worker_classic_execution.hpp"
#include "worker_smart_dispatch.hpp"
#include "extended_inter_memory.hpp"
#include <windows.h>
#include <algorithm>
#include <cstring>

namespace aexcompat::memory_fill {
bool verify() {
  using namespace worker_runtime;
  bool ok = true;
  const auto check = [&](bool value, int line) { if (!value) { std::cerr << "fill selftest line " << line << '\n'; ok = false; } };
#define CHECK(v) check((v), __LINE__)
  CHECK(!configure(L"v1|0|11111"));
  CHECK(!configure(L"v1|zz|11111"));
  CHECK(!configure(L"v1|00|11112"));
  // Absence keeps existing zero-handle and position-dependent output defaults.
  auto h = handles::new_handle(12);
  CHECK(h && std::all_of(static_cast<unsigned char*>(*h), static_cast<unsigned char*>(*h) + 12, [](auto b) { return b == 0; }));
  handles::dispose_handle(h);
  CHECK(parameter_execution::verify_declared_defaults());
  std::vector<unsigned char> baseline(48);
  CHECK(output_coverage::seed(baseline.data(), baseline.size(), 3, 4, 12, 4));
  CHECK(report().empty());
  std::array<std::vector<unsigned char>, 2> observed;
  std::array<std::array<unsigned char, Count>, 2> deliberate_reads{};
  for (int trial = 0; trial != 2; ++trial) {
    config = {}; events.clear(); applied = {}; unsupported = {}; dropped = 0;
    CHECK(configure(trial ? L"v1|3f9a|11111" : L"v1|0012|11111"));
    CHECK(!configure(L"v1|ff|11111"));
    CHECK(parameter_execution::verify_declared_defaults());
    CHECK(smart_dispatch::verify_video_frame_memory_fill());
    for (int depth : {4, 8, 16}) {
      const int row = 3 * depth + 16;
      std::vector<unsigned char> input(row * 4 + 8, 0x6d);
      padding(input.data(), row * 4, 3, 4, row, depth, "selftest_input");
      deliberate_reads[trial][Padding] = input[3 * depth];
      for (int y = 0; y < 4; ++y) {
        for (int b = 0; b < 3 * depth; ++b) CHECK(input[y * row + b] == 0x6d);
        for (int b = 3 * depth; b < row; ++b) CHECK(input[y * row + b] == byte(y * row + b));
      }
      CHECK(input.back() == 0x6d);
      CHECK(output_coverage::seed(input.data(), row * 4, 3, 4, row, depth));
      deliberate_reads[trial][Output] = input[0];
      // Classic's guard checker must accept the configured padding and still
      // detect a one-byte overrun into it at every depth.
      classic_execution::Hooks hooks{};
      hooks.hash = hash;
      hooks.copy_packed = [](const unsigned char* src, int32_t rb, int32_t w,
          int32_t h, int32_t bpp, std::vector<unsigned char>& dest) {
        dest.resize(w * h * bpp);
        for (int y = 0; y < h; ++y)
          std::memcpy(dest.data() + y * w * bpp, src + y * rb, w * bpp);
        return true;
      };
      bool guards = false;
      classic_execution::Context context{};
      context.destination = input.data(); context.rowbytes = row;
      context.width = 3; context.height = 4; context.pixel_bytes = depth;
      context.error = 1; context.sentinels_intact = true;
      context.guards_intact = &guards;
      CHECK(classic_execution::finalize(context, hooks) == 1 && guards);
      input[3 * depth] ^= 1;
      CHECK(classic_execution::finalize(context, hooks) == 1 && !guards);
      input[3 * depth] ^= 1;
      std::vector<unsigned char> packed(3 * depth * 4);
      for (int y = 0; y < 4; ++y) std::memcpy(packed.data() + y * 3 * depth, input.data() + y * row, 3 * depth);
      CHECK(output_coverage::inspect(packed.data(), packed.size(), 3, 4, depth, {0,0,3,4}).unwritten_pixels == 12);
      packed[depth] ^= 1;
      CHECK(output_coverage::inspect(packed.data(), packed.size(), 3, 4, depth, {0,0,3,4}).unwritten_pixels == 11);
      if (depth == 4) observed[trial] = packed;
    }
    for (int type : {1, 4, 7, 10, 11}) {
      auto& records = parameters::state().records;
      const auto saved = records;
      parameters::ParamRecord record{};
      record.index = 1; record.type = type; record.name = "fill parameter";
      record.has_numeric = true; record.valid_min = 1; record.valid_max = 100;
      record.default_value = 101; // normalized only in the dispatched definition
      record.raw.fill(std::byte{0x69});
      std::memcpy(record.raw.data() + 12, &type, sizeof(type));
      auto expected = record.raw;
      if (type == 10) {
        const double value = 100.0;
        std::memcpy(expected.data() + 56, &value, sizeof(value));
      } else if (type != 11) {
        const int32_t value = type == 4 ? 1 : 100;
        std::memcpy(expected.data() + 56, &value, sizeof(value));
      }
      records = {record};
      parameter_execution::Definitions definitions(2);
      definitions[0].fill(std::byte{0x37});
      const auto primary = definitions[0];
      parameter_execution::initialize_parameter_definitions(definitions, 23, 17);
      const auto& param = definitions[1];
      deliberate_reads[trial][Parameter] = static_cast<unsigned char>(param.back());
      const std::size_t active = type == 1 || type == 10 ? 88 : type == 11 ? 32 : 16;
      for (std::size_t i = 0; i < 56 + active; ++i) CHECK(param[i] == expected[i]);
      for (std::size_t i = 56 + active; i < param.size(); ++i) CHECK(param[i] == std::byte{byte(i)});
      CHECK(definitions[0] == primary);
      CHECK(records[0].raw == record.raw && records[0].default_value == 101);
      if (type != 11) CHECK(records[0].has_current && records[0].current_value == (type == 4 ? 1 : 100));
      records = saved;
    }
    h = handles::new_handle(12);
    auto* data = static_cast<unsigned char*>(handles::lock_handle(h));
    deliberate_reads[trial][Handle] = data[0];
    for (int i = 0; i < 12; ++i) CHECK(data[i] == byte(i));
    std::memset(data, 0x72, 12); handles::unlock_handle(h);
    CHECK(handles::resize_handle(20, &h) == 0);
    data = static_cast<unsigned char*>(handles::lock_handle(h));
    for (int i = 0; i < 20; ++i) CHECK(data[i] == (i < 12 ? 0x72 : byte(i)));
    handles::unlock_handle(h);
    CHECK(handles::resize_handle(6, &h) == 0);
    CHECK(handles::resize_handle(12, &h) == 0);
    data = static_cast<unsigned char*>(*h);
    for (int i = 0; i < 12; ++i) CHECK(data[i] == (i < 6 ? 0x72 : byte(i)));
    handles::dispose_handle(h);
    for (int flags : {0,1,2,3}) {
      void* memory{}; void* locked{};
      CHECK(handles::new_aegp_mem_handle(1, "selftest", 12, flags, &memory) == 0);
      CHECK(handles::lock_aegp_mem_handle(memory, &locked) == 0);
      data = static_cast<unsigned char*>(locked);
      if (!(flags & 1)) deliberate_reads[trial][Suite] = data[0];
      for (int i = 0; i < 12; ++i) CHECK(data[i] == ((flags & 1) ? 0 : byte(i)));
      std::memset(data, 0x72, 12);
      CHECK(handles::unlock_aegp_mem_handle(memory) == 0);
      CHECK(handles::resize_aegp_mem_handle("selftest", 20, memory) == 0);
      CHECK(handles::lock_aegp_mem_handle(memory, &locked) == 0);
      data = static_cast<unsigned char*>(locked);
      for (int i = 0; i < 20; ++i) CHECK(data[i] == (i < 12 ? 0x72 : (flags & 1) ? 0 : byte(i)));
      CHECK(handles::unlock_aegp_mem_handle(memory) == 0);
      CHECK(handles::free_aegp_mem_handle(memory) == 0);
    }
    for (int clear : {0,1}) for (int format : {world_registry::kPixelFormatArgb32, world_registry::kPixelFormatArgb64, world_registry::kPixelFormatArgb128}) {
      std::array<std::byte, 120> world{};
      CHECK(world_registry::new_world(nullptr, 3, 4, clear, format, world.data()) == 0);
      void* pixels{}; int row{};
      std::memcpy(&pixels, world.data() + 24, 8); std::memcpy(&row, world.data() + 32, 4);
      data = static_cast<unsigned char*>(pixels);
      for (int i = 0; i < row * 4; ++i) CHECK(data[i] == (clear ? 0 : byte(i)));
      CHECK(world_registry::dispose_world(nullptr, world.data()) == 0);
    }
    void* cleared{};
    CHECK(extended_inter::allocate(&cleared, 20) == 0);
    CHECK(std::all_of(static_cast<unsigned char*>(cleared), static_cast<unsigned char*>(cleared) + 20, [](auto b) { return b == 0; }));
    CHECK(extended_inter::release(&cleared) == 0);
    for (auto count : applied) CHECK(count > 0);
    CHECK(handles::handle_lifetimes_balanced());
    CHECK(handles::aegp_memory_balanced());
  }
  CHECK(observed[0] != observed[1]);
  // Intentional reads of each uninitialized class respond separately; a
  // regression that silently stops filling any one scope fails here.
  for (int scope = 0; scope < Count; ++scope)
    CHECK(deliberate_reads[0][scope] != deliberate_reads[1][scope]);
  config = {};
  CHECK(smart_dispatch::verify_video_frame_memory_fill());
  std::vector<unsigned char> after(48);
  CHECK(output_coverage::seed(after.data(), after.size(), 3, 4, 12, 4));
  CHECK(after == baseline);
  CHECK(report().empty());
  return ok;
#undef CHECK
}
} // namespace aexcompat::memory_fill

namespace aexcompat::memory_fill {
int journal_probe(int argc, wchar_t** argv) {
  if (argc != 4 || !configure_journal(argv[2]) || !configure(L"v1|3f9a|00010")) return 2;
  auto handle = worker_runtime::handles::new_handle(32);
  if (!handle) return 3;
  if (std::wstring(argv[3]) == L"interrupt") Sleep(INFINITE);
  if (std::wstring(argv[3]) == L"failure") ExitProcess(83);
  worker_runtime::handles::dispose_handle(handle);
  return 0;
}
}
