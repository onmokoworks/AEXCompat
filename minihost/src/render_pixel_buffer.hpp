#pragma once

#include <cstddef>

namespace aexcompat::render_safety {

class InputPixelBuffer {
 public:
  explicit InputPixelBuffer(std::size_t size);
  ~InputPixelBuffer();
  InputPixelBuffer(const InputPixelBuffer&) = delete;
  InputPixelBuffer& operator=(const InputPixelBuffer&) = delete;

  explicit operator bool() const;
  unsigned char* data();
  const unsigned char* data() const;
  unsigned char& operator[](std::size_t index);
  bool set_plugin_writable(bool writable);

 private:
  std::size_t size_{};
  unsigned char* data_{};
};

class OutputPixelBuffer {
 public:
  static constexpr std::size_t kSentinelBytes = 64;

  OutputPixelBuffer() = default;
  explicit OutputPixelBuffer(std::size_t size);
  ~OutputPixelBuffer();
  OutputPixelBuffer(const OutputPixelBuffer&) = delete;
  OutputPixelBuffer& operator=(const OutputPixelBuffer&) = delete;

  bool reset(std::size_t size);
  explicit operator bool() const;
  unsigned char* data();
  const unsigned char* data() const;
  std::size_t size() const;
  bool sentinels_intact() const;
  bool guard_pages_intact() const;

 private:
  void release();

  unsigned char* allocation_{};
  unsigned char* data_{};
  std::size_t size_{};
  std::size_t page_size_{};
  std::size_t committed_size_{};
  std::size_t allocation_size_{};
};

}  // namespace aexcompat::render_safety
