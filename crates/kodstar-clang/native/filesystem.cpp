// Purpose: 체크포인트와 builtin header callback만 읽는 Clang 메모리 파일시스템.
// Pattern: Virtual Filesystem Adapter.
// Usage: frontend가 실행 수명 동안 소유하며 host filesystem을 노출하지 않음.
// Related: crates/kodstar-clang/native/bridge.h,
// crates/kodstar-clang/src/callbacks.rs.

#include "bridge.h"
#include <llvm/ADT/IntrusiveRefCntPtr.h>
#include <llvm/Support/MemoryBuffer.h>
#include <llvm/Support/Path.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <map>
#include <string>

namespace {
using namespace llvm;
class CallbackBuffer final : public MemoryBuffer {
  const KsCallbacks &callbacks;
  KsFile allocation;
  std::string name;

public:
  CallbackBuffer(const KsCallbacks &callbacks, KsFile allocation, StringRef name)
      : callbacks(callbacks), allocation(allocation), name(name) {
    // Rust transfers an owned allocation with a trailing NUL. The synchronous
    // frontend and its buffers are destroyed before callback state returns.
    const auto *start = reinterpret_cast<const char *>(allocation.data);
    init(start, start + allocation.size, true);
  }
  ~CallbackBuffer() override { callbacks.release(callbacks.state, allocation); }
  StringRef getBufferIdentifier() const override { return name; }
  BufferKind getBufferKind() const override { return MemoryBuffer_Malloc; }
};

class File final : public vfs::File {
  vfs::Status info;
  std::unique_ptr<MemoryBuffer> content;

public:
  File(vfs::Status info, std::unique_ptr<MemoryBuffer> data)
      : info(std::move(info)), content(std::move(data)) {}
  ErrorOr<vfs::Status> status() override { return info; }
  ErrorOr<std::unique_ptr<MemoryBuffer>> getBuffer(const Twine &, std::int64_t,
                                                   bool, bool) override {
    if (!content)
      return std::make_error_code(std::errc::io_error);
    return std::move(content);
  }
  std::error_code close() override { return {}; }
};

class Filesystem final : public vfs::FileSystem {
  const KsCallbacks &callbacks;
  const std::size_t path_limit;
  std::map<std::string, vfs::Status> records;
  std::string cwd = "/__kodstar_project";

public:
  explicit Filesystem(const KsCallbacks &callbacks, std::size_t path_limit)
      : callbacks(callbacks), path_limit(path_limit) {}
  ErrorOr<vfs::Status> status(const Twine &path) override {
    SmallString<256> normalized(path.str());
    sys::path::remove_dots(normalized, true);
    std::string name(normalized.str());
    if (auto found = records.find(name); found != records.end())
      return found->second;
    if (records.size() >= path_limit) {
      callbacks.issue(callbacks.state, 5, {name.data(), name.size()},
                      records.size() + 1, path_limit);
      return std::make_error_code(std::errc::not_enough_memory);
    }
    KsFile file{};
    int kind =
        callbacks.read(callbacks.state, {name.data(), name.size()}, 0, &file);
    if (kind < 0)
      return std::make_error_code(std::errc::no_such_file_or_directory);
    auto type = kind == 1 ? sys::fs::file_type::directory_file
                          : sys::fs::file_type::regular_file;
    auto info = vfs::Status(name, vfs::getNextVirtualUniqueID(), {}, 0, 0,
                            file.size, type, sys::fs::perms::all_read);
    records.emplace(name, info);
    return info;
  }
  ErrorOr<std::unique_ptr<vfs::File>>
  openFileForRead(const Twine &path) override {
    auto info = status(path);
    if (!info)
      return info.getError();
    if (!info->isRegularFile())
      return std::make_error_code(std::errc::is_a_directory);
    auto name = info->getName();
    KsFile file{};
    if (callbacks.read(callbacks.state, {name.data(), name.size()}, 1, &file) !=
        0)
      return std::make_error_code(std::errc::no_such_file_or_directory);
    auto buffer = std::make_unique<CallbackBuffer>(callbacks, file, name);
    return std::unique_ptr<vfs::File>(new File(*info, std::move(buffer)));
  }
  vfs::directory_iterator dir_begin(const Twine &,
                                    std::error_code &ec) override {
    ec = {};
    return {};
  }
  std::error_code setCurrentWorkingDirectory(const Twine &path) override {
    cwd = path.str();
    return {};
  }
  ErrorOr<std::string> getCurrentWorkingDirectory() const override {
    return cwd;
  }
  std::error_code getRealPath(const Twine &path,
                              SmallVectorImpl<char> &output) override {
    path.toVector(output);
    sys::path::remove_dots(output, true);
    return {};
  }
};
} // namespace

llvm::IntrusiveRefCntPtr<llvm::vfs::FileSystem>
ks_filesystem(const KsInput &input, const KsCallbacks &callbacks) {
  auto project =
      llvm::makeIntrusiveRefCnt<Filesystem>(callbacks, input.path_limit);
  auto source = llvm::makeIntrusiveRefCnt<llvm::vfs::InMemoryFileSystem>();
  source->addFile(llvm::StringRef(input.path.data, input.path.size), 0,
                  llvm::MemoryBuffer::getMemBufferCopy(
                      llvm::StringRef(input.source.data, input.source.size)));
  auto overlay =
      llvm::makeIntrusiveRefCnt<llvm::vfs::OverlayFileSystem>(project);
  overlay->pushOverlay(source);
  return overlay;
}
