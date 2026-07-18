#include "render_pixel_buffer.hpp"

#include <windows.h>

#include <algorithm>
#include <cstdint>
#include <cstring>

namespace aexcompat::render_safety {

InputPixelBuffer::InputPixelBuffer(std::size_t size)
    : size_(size), data_(static_cast<unsigned char*>(VirtualAlloc(
          nullptr, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE))) {}

InputPixelBuffer::~InputPixelBuffer() {
  if (data_) VirtualFree(data_, 0, MEM_RELEASE);
}

InputPixelBuffer::operator bool() const { return data_ != nullptr; }
unsigned char* InputPixelBuffer::data() { return data_; }
const unsigned char* InputPixelBuffer::data() const { return data_; }
unsigned char& InputPixelBuffer::operator[](std::size_t index) { return data_[index]; }

bool InputPixelBuffer::set_plugin_writable(bool writable) {
  DWORD previous = 0;
  return data_ && VirtualProtect(data_, size_, writable ? PAGE_READWRITE : PAGE_READONLY,
                                 &previous) != FALSE;
}

OutputPixelBuffer::OutputPixelBuffer(std::size_t size) { reset(size); }
OutputPixelBuffer::~OutputPixelBuffer() { release(); }

bool OutputPixelBuffer::reset(std::size_t size) {
  SYSTEM_INFO info{};
  GetSystemInfo(&info);
  const std::size_t page_size = info.dwPageSize;
  if (!size || !page_size || size > SIZE_MAX - 2 * kSentinelBytes) return false;
  const std::size_t payload = size + 2 * kSentinelBytes;
  if (payload > SIZE_MAX - (page_size - 1)) return false;
  const std::size_t committed_size = (payload + page_size - 1) / page_size * page_size;
  if (committed_size < payload || committed_size > SIZE_MAX - 2 * page_size) return false;
  const std::size_t allocation_size = committed_size + 2 * page_size;
  auto* allocation = static_cast<unsigned char*>(VirtualAlloc(
      nullptr, allocation_size, MEM_RESERVE, PAGE_NOACCESS));
  if (!allocation || !VirtualAlloc(allocation + page_size, committed_size,
                                   MEM_COMMIT, PAGE_READWRITE)) {
    if (allocation) VirtualFree(allocation, 0, MEM_RELEASE);
    return false;
  }
  release();
  allocation_ = allocation;
  page_size_ = page_size;
  committed_size_ = committed_size;
  allocation_size_ = allocation_size;
  size_ = size;
  data_ = allocation_ + page_size_ + kSentinelBytes;
  std::memset(allocation_ + page_size_, 0xA5, committed_size_);
  std::memset(data_, 0xCC, size_);
  return true;
}

OutputPixelBuffer::operator bool() const { return data_ != nullptr; }
unsigned char* OutputPixelBuffer::data() { return data_; }
const unsigned char* OutputPixelBuffer::data() const { return data_; }
std::size_t OutputPixelBuffer::size() const { return size_; }

bool OutputPixelBuffer::sentinels_intact() const {
  if (!data_) return false;
  const auto intact = [](const unsigned char* begin, const unsigned char* end) {
    return std::all_of(begin, end, [](unsigned char byte) { return byte == 0xA5; });
  };
  const unsigned char* committed_begin = allocation_ + page_size_;
  return intact(committed_begin, data_) &&
      intact(data_ + size_, committed_begin + committed_size_);
}

bool OutputPixelBuffer::guard_pages_intact() const {
  if (!allocation_) return false;
  MEMORY_BASIC_INFORMATION before{}, after{};
  const auto inaccessible_reservation = [](const MEMORY_BASIC_INFORMATION& page) {
    return page.State == MEM_RESERVE && page.AllocationProtect == PAGE_NOACCESS;
  };
  return VirtualQuery(allocation_, &before, sizeof(before)) == sizeof(before) &&
      VirtualQuery(allocation_ + page_size_ + committed_size_, &after, sizeof(after)) ==
          sizeof(after) &&
      inaccessible_reservation(before) && inaccessible_reservation(after);
}

void OutputPixelBuffer::release() {
  if (allocation_) VirtualFree(allocation_, 0, MEM_RELEASE);
  allocation_ = data_ = nullptr;
  size_ = page_size_ = committed_size_ = allocation_size_ = 0;
}

}  // namespace aexcompat::render_safety
