// Purpose: 동기적 Clang 실행의 C ABI와 호출 중에만 유효한 빌린 byte view.
// Pattern: FFI Contract.
// Usage: Rust adapter와 native frontend·VFS·visitor가 공유.
// Related: crates/kodstar-clang/src/ffi.rs,
// crates/kodstar-clang/native/frontend.cpp.
#pragma once
#include <cstddef>
#include <cstdint>

struct KsText {
  const char *data;
  std::size_t size;
};
struct KsFile {
  const unsigned char *data;
  std::size_t size;
};
struct KsFact {
  std::uint32_t kind, flags;
  std::size_t begin, end, name_begin, name_end;
  KsText name, qualified, usr, caller, target_file;
  std::size_t target_position;
  std::size_t parameter_begin, parameter_end;
};
struct KsCallbacks {
  void *state;
  int (*read)(void *, KsText, int, KsFile *);
  void (*release)(void *, KsFile);
  int (*emit)(void *, const KsFact *);
  int (*check)(void *);
  void (*issue)(void *, std::uint32_t, KsText, std::size_t, std::size_t);
};
struct KsInput {
  KsText path, source, target;
  const char *const *arguments;
  std::size_t argument_count, token_limit, memory_limit, template_limit,
      path_limit;
  int cpp;
};
extern "C" int ks_clang_analyze(const KsInput *, const KsCallbacks *);

namespace llvm {
template <class T> class IntrusiveRefCntPtr;
namespace vfs {
class FileSystem;
}
} // namespace llvm
namespace clang {
class ASTContext;
}
llvm::IntrusiveRefCntPtr<llvm::vfs::FileSystem>
ks_filesystem(const KsInput &, const KsCallbacks &);
bool ks_visit(clang::ASTContext &, const KsCallbacks &, KsText, bool);
