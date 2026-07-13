#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <new>
#include <sstream>
#include <string>
#include <unordered_set>
#include <vector>

namespace {
constexpr std::size_t kInSize = 408;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kParamSize = 176;
constexpr std::size_t kMaxParams = 64;
constexpr std::size_t kInAddParam = 16;
constexpr std::size_t kInUtils = 176;
constexpr std::size_t kInEffectRef = 184;
constexpr std::size_t kInVersion = 196;
constexpr std::size_t kInApplicationId = 204;
constexpr std::size_t kInNumParams = 208;
constexpr std::size_t kInGlobalData = 312;
constexpr std::size_t kInPicaBasic = 384;
constexpr std::size_t kOutGlobalData = 40;
constexpr std::size_t kOutNumParams = 48;
constexpr std::size_t kOutFlags = 96;
constexpr std::size_t kOutMessage = 100;
constexpr std::size_t kOutFlags2 = 400;
constexpr std::size_t kParamType = 12;
constexpr std::size_t kParamName = 16;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = 48;
constexpr int32_t kGlobalSetup = 1;
constexpr int32_t kGlobalSetdown = 3;
constexpr int32_t kParamsSetup = 4;
constexpr int32_t kRender = 11;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsNewHandle = 160;
constexpr std::size_t kUtilsLockHandle = 168;
constexpr std::size_t kUtilsUnlockHandle = 176;
constexpr std::size_t kUtilsDisposeHandle = 184;

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AddParamCallback = int32_t(__cdecl*)(void*, int32_t, void*);

struct ParamRecord {
  int32_t index{};
  int32_t type{};
  uint32_t flags{};
  std::string name;
  bool has_numeric{};
  double valid_min{};
  double valid_max{};
  double slider_min{};
  double slider_max{};
  double default_value{};
  int32_t precision{-1};
  std::string choices;
  std::string label;
  std::array<std::byte, kParamSize> raw{};
};
std::vector<ParamRecord> g_params;
struct HandleRecord { void* data{}; std::size_t size{}; };
std::unordered_set<HandleRecord*> g_handles;

void** __cdecl new_handle(uint64_t size) {
  std::cerr << "callback:new_handle size=" << size << "\n" << std::flush;
  if (size > 64 * 1024 * 1024) return nullptr;
  auto* record = new (std::nothrow) HandleRecord;
  if (!record) return nullptr;
  record->data = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!record->data && size != 0) { delete record; return nullptr; }
  if (record->data) std::memset(record->data, 0, static_cast<std::size_t>(size));
  record->size = static_cast<std::size_t>(size);
  g_handles.insert(record);
  return &record->data;
}

void* __cdecl lock_handle(void** handle) {
  std::cerr << "callback:lock_handle\n" << std::flush;
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  return record && g_handles.count(record) ? record->data : nullptr;
}

void __cdecl unlock_handle(void**) {}

void __cdecl dispose_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  if (!record || !g_handles.erase(record)) return;
  ::operator delete(record->data);
  delete record;
}

uint64_t __cdecl handle_size(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  return record && g_handles.count(record) ? record->size : 0;
}

int32_t __cdecl resize_handle(uint64_t size, void*** handle) {
  if (!handle || !*handle || size > 64 * 1024 * 1024) return 4;
  auto* record = reinterpret_cast<HandleRecord*>(*handle);
  if (!g_handles.count(record)) return 4;
  void* replacement = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!replacement && size) return 1;
  if (replacement) {
    std::memset(replacement, 0, static_cast<std::size_t>(size));
    std::memcpy(replacement, record->data, (std::min)(record->size, static_cast<std::size_t>(size)));
  }
  ::operator delete(record->data);
  record->data = replacement;
  record->size = static_cast<std::size_t>(size);
  return 0;
}

struct HandleSuite {
  decltype(&new_handle) create;
  decltype(&lock_handle) lock;
  decltype(&unlock_handle) unlock;
  decltype(&dispose_handle) dispose;
  decltype(&handle_size) size;
  decltype(&resize_handle) resize;
};
HandleSuite g_handle_suite{&new_handle, &lock_handle, &unlock_handle,
                           &dispose_handle, &handle_size, &resize_handle};

int32_t __cdecl acquire_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return 4;
  *suite = nullptr;
  if (name && std::strcmp(name, "PF Handle Suite") == 0 && version == 2) {
    *suite = &g_handle_suite;
    return 0;
  }
  return 1;
}

int32_t __cdecl release_suite(const char*, int32_t) { return 0; }

struct BasicSuite {
  decltype(&acquire_suite) acquire;
  decltype(&release_suite) release;
  void* unsupported[5]{};
};
BasicSuite g_basic_suite{&acquire_suite, &release_suite};

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

int32_t __cdecl add_param(void*, int32_t index, void* definition) {
  if (!definition || g_params.size() >= kMaxParams) return 4;
  std::array<std::byte, kParamSize> bytes{};
  std::memcpy(bytes.data(), definition, bytes.size());
  const char* name = reinterpret_cast<const char*>(bytes.data() + kParamName);
  const auto length = strnlen_s(name, kParamNameSize);
  ParamRecord record{index, read<int32_t>(bytes, kParamType),
                     read<uint32_t>(bytes, kParamFlags), std::string(name, length)};
  constexpr std::size_t u = 56;
  if (record.type == 1) {
    record.has_numeric = true;
    record.valid_min = read<int32_t>(bytes, u + 68);
    record.valid_max = read<int32_t>(bytes, u + 72);
    record.slider_min = read<int32_t>(bytes, u + 76);
    record.slider_max = read<int32_t>(bytes, u + 80);
    record.default_value = read<int32_t>(bytes, u + 84);
  } else if (record.type == 7) {
    record.has_numeric = true;
    record.valid_min = 1;
    record.valid_max = read<int16_t>(bytes, u + 4);
    record.slider_min = record.valid_min;
    record.slider_max = record.valid_max;
    record.default_value = read<int16_t>(bytes, u + 6);
    const char* choices = read<const char*>(bytes, u + 8);
    if (choices) record.choices.assign(choices, strnlen_s(choices, 4096));
  } else if (record.type == 4) {
    record.has_numeric = true;
    record.valid_min = 0;
    record.valid_max = 1;
    record.slider_min = 0;
    record.slider_max = 1;
    record.default_value = read<uint8_t>(bytes, u + 4) ? 1 : 0;
    const char* label = read<const char*>(bytes, u + 8);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  } else if (record.type == 10) {
    record.has_numeric = true;
    record.valid_min = read<float>(bytes, u + 48);
    record.valid_max = read<float>(bytes, u + 52);
    record.slider_min = read<float>(bytes, u + 56);
    record.slider_max = read<float>(bytes, u + 60);
    record.default_value = read<float>(bytes, u + 64);
    record.precision = read<int16_t>(bytes, u + 68);
  }
  record.raw = bytes;
  g_params.push_back(std::move(record));
  return 0;
}

int32_t __cdecl checkout_param(void*, int32_t, int32_t, int32_t, uint32_t, void*) {
  return 4;
}
int32_t __cdecl checkin_param(void*, void*) { return 0; }

std::string escape(const std::string& input) {
  std::string output;
  for (unsigned char ch : input) {
    if (ch == '"' || ch == '\\') output.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) output.push_back(static_cast<char>(ch));
  }
  return output;
}

bool sha256(const std::filesystem::path& path, std::string& result) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0 ||
      BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                        &returned, 0) < 0) return false;
  std::vector<unsigned char> object(object_size);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0) < 0) return false;
  std::ifstream input(path, std::ios::binary);
  std::array<unsigned char, 65536> buffer{};
  while (input) {
    input.read(reinterpret_cast<char*>(buffer.data()), buffer.size());
    if (input.gcount() > 0 && BCryptHashData(hash, buffer.data(), static_cast<ULONG>(input.gcount()), 0) < 0) return false;
  }
  std::array<unsigned char, 32> digest{};
  const bool ok = input.eof() && BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0;
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  if (!ok) return false;
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  result = text.str();
  return true;
}

std::string sha256_bytes(const unsigned char* data, std::size_t size) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
  BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                    reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &returned, 0);
  std::vector<unsigned char> object(object_size);
  BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0);
  BCryptHashData(hash, const_cast<PUCHAR>(data), static_cast<ULONG>(size), 0);
  std::array<unsigned char, 32> digest{};
  BCryptFinishHash(hash, digest.data(), digest.size(), 0);
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  return text.str();
}

#ifdef AEXCOMPAT_RENDER_WORKER
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& command_output,
                    std::string& input_hash, std::string& output_hash,
                    bool& guards_intact) {
  constexpr int32_t width = 16, height = 12, rowbytes = width * 4;
  constexpr std::size_t guard = 64;
  std::vector<unsigned char> source(width * height * 4);
  for (int32_t y = 0; y < height; ++y) {
    for (int32_t x = 0; x < width; ++x) {
      auto* pixel = &source[(y * width + x) * 4];
      pixel[0] = 255;
      pixel[1] = static_cast<unsigned char>(x * 255 / (width - 1));
      pixel[2] = static_cast<unsigned char>(y * 255 / (height - 1));
      pixel[3] = static_cast<unsigned char>((x + y) * 255 / (width + height - 2));
    }
  }
  std::vector<unsigned char> guarded(width * height * 4 + guard * 2, 0xA5);
  unsigned char* destination = guarded.data() + guard;
  std::memset(destination, 0xCC, width * height * 4);

  std::array<std::byte, 120> input_world{}, output_world{};
  auto setup_world = [&](auto& world, void* pixels) {
    write<void*>(world, 24, pixels);
    write<int32_t>(world, 32, rowbytes);
    write<int32_t>(world, 36, width);
    write<int32_t>(world, 40, height);
    write<int32_t>(world, 44, 0);
    write<int32_t>(world, 48, 0);
    write<int32_t>(world, 52, height);
    write<int32_t>(world, 56, width);
  };
  setup_world(input_world, source.data());
  setup_world(output_world, destination);

  std::array<std::array<std::byte, kParamSize>, 8> definitions{};
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  for (std::size_t i = 0; i < g_params.size() && i + 1 < definitions.size(); ++i) {
    definitions[i + 1] = g_params[i].raw;
    constexpr std::size_t u = 56;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(definitions[i + 1], u, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(definitions[i + 1], u, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 10)
      write<double>(definitions[i + 1], u, g_params[i].default_value);
  }
  std::array<void*, 9> params{};
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  write<int32_t>(input, 224, 0);
  write<int32_t>(input, 228, 1);
  write<int32_t>(input, 232, 1);
  write<uint32_t>(input, 240, 1);
  write<int32_t>(input, 252, width);
  write<int32_t>(input, 256, height);
  input_hash = sha256_bytes(source.data(), source.size());
  const int32_t error = entry(kRender, input.data(), command_output.data(), params.data(),
                              output_world.data(), nullptr);
  output_hash = sha256_bytes(destination, width * height * 4);
  guards_intact = std::all_of(guarded.begin(), guarded.begin() + guard, [](auto b) { return b == 0xA5; }) &&
      std::all_of(guarded.end() - guard, guarded.end(), [](auto b) { return b == 0xA5; });
  return error;
}
#endif

void report(const char* status, int32_t global_error, int32_t params_error,
            int32_t setdown_error, const std::array<std::byte, kOutSize>& output) {
  const char* message = reinterpret_cast<const char*>(output.data() + kOutMessage);
  std::cout << "{\"schema_version\":1,\"stage\":\"L2\",\"status\":\"" << status
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"reported_num_params\":" << read<int32_t>(output, kOutNumParams)
            << ",\"out_flags\":" << read<uint32_t>(output, kOutFlags)
            << ",\"out_flags2\":" << read<uint32_t>(output, kOutFlags2)
            << ",\"return_message\":\"" << escape(std::string(message, strnlen_s(message, 256)))
            << "\",\"parameters\":[";
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (i) std::cout << ',';
    const auto& param = g_params[i];
    std::cout << "{\"index\":" << param.index << ",\"type\":" << param.type
              << ",\"flags\":" << param.flags << ",\"name\":\""
              << escape(param.name) << "\"";
    if (param.has_numeric) {
      std::cout << ",\"valid_min\":" << param.valid_min
                << ",\"valid_max\":" << param.valid_max
                << ",\"slider_min\":" << param.slider_min
                << ",\"slider_max\":" << param.slider_max
                << ",\"default\":" << param.default_value;
    }
    if (param.precision >= 0) std::cout << ",\"precision\":" << param.precision;
    if (!param.choices.empty()) std::cout << ",\"choices\":\"" << escape(param.choices) << "\"";
    if (!param.label.empty()) std::cout << ",\"label\":\"" << escape(param.label) << "\"";
    std::cout << '}';
  }
  std::cout << "],\"selectors_executed\":true,\"render_performed\":false}\n";
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  if (argc != 4) return 2;
#ifdef AEXCOMPAT_RENDER_WORKER
  if (std::wstring(argv[1]) != L"--render") return 2;
#else
  if (std::wstring(argv[1]) != L"--l2") return 2;
#endif
  std::string expected;
  for (const wchar_t* p = argv[3]; *p; ++p) {
    if (*p > 0x7f) return 2;
    expected.push_back(static_cast<char>(*p));
  }
  std::string actual;
  if (!sha256(argv[2], actual) || actual != expected) return 10;
  SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS);
  HMODULE module = LoadLibraryExW(argv[2], nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) return 11;
  auto entry = reinterpret_cast<EffectEntry>(GetProcAddress(module, "EffectMain"));
  if (!entry) entry = reinterpret_cast<EffectEntry>(GetProcAddress(module, "EntryPointFunc"));
  if (!entry) { FreeLibrary(module); return 12; }

  alignas(8) std::array<std::byte, kInSize> input{};
  alignas(8) std::array<std::byte, kOutSize> output{};
  alignas(8) std::array<std::byte, kUtilsSize> utils{};
  write(input, 0, &checkout_param);
  write(input, 8, &checkin_param);
  write(input, kInAddParam, static_cast<AddParamCallback>(&add_param));
  write(utils, kUtilsNewHandle, &new_handle);
  write(utils, kUtilsLockHandle, &lock_handle);
  write(utils, kUtilsUnlockHandle, &unlock_handle);
  write(utils, kUtilsDisposeHandle, &dispose_handle);
  write<void*>(input, kInUtils, utils.data());
  write<void*>(input, kInPicaBasic, &g_basic_suite);
  write<void*>(input, kInEffectRef, nullptr);
  write<uint32_t>(input, kInVersion, 0);
  write<uint32_t>(input, kInApplicationId, 0x46585443u);
  write<int32_t>(input, kInNumParams, 1);
  std::cerr << "stage:global_setup_begin\n" << std::flush;
  const int32_t global_error = entry(kGlobalSetup, input.data(), output.data(), nullptr, nullptr, nullptr);
  std::cerr << "stage:global_setup_end error=" << global_error << "\n" << std::flush;
  write<void*>(input, kInGlobalData, read<void*>(output, kOutGlobalData));
  std::cerr << "stage:params_setup_begin\n" << std::flush;
  const int32_t params_error = global_error == 0
      ? entry(kParamsSetup, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:params_setup_end error=" << params_error << "\n" << std::flush;
#ifdef AEXCOMPAT_RENDER_WORKER
  std::string input_hash, output_hash;
  bool guards_intact = false;
  std::cerr << "stage:render_begin\n" << std::flush;
  const int32_t render_error = params_error == 0
      ? render_once(entry, input, output, input_hash, output_hash, guards_intact) : -1;
  std::cerr << "stage:render_end error=" << render_error << "\n" << std::flush;
#endif
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = global_error == 0
      ? entry(kGlobalSetdown, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:global_setdown_end error=" << setdown_error << "\n" << std::flush;
#ifdef AEXCOMPAT_RENDER_WORKER
  std::cout << "{\"schema_version\":1,\"stage\":\"classic_render\",\"status\":\""
            << (render_error == 0 && guards_intact ? "render_completed" : "render_failed")
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"render_error\":" << render_error
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"pixel_format\":\"argb8\",\"width\":16,\"height\":12,\"rowbytes\":64"
            << ",\"input_sha256\":\"" << input_hash << "\",\"output_sha256\":\""
            << output_hash << "\",\"guard_bytes_intact\":" << (guards_intact ? "true" : "false")
            << ",\"render_performed\":true}\n";
#else
  report(global_error == 0 && params_error == 0 ? "selectors_completed" : "selector_error",
         global_error, params_error, setdown_error, output);
#endif
  FreeLibrary(module);
#ifdef AEXCOMPAT_RENDER_WORKER
  return global_error == 0 && params_error == 0 && render_error == 0 && guards_intact ? 0 : 21;
#else
  return global_error == 0 && params_error == 0 ? 0 : 20;
#endif
}
