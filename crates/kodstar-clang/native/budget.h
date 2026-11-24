// Purpose: 한 TU의 독립 작업량·관측 메모리 한도와 취소 검사.
// Pattern: Resource Guard.
// Usage: frontend의 token watcher·template callback·TU 완료가 공유.
// Related: crates/kodstar-clang/native/frontend.cpp, crates/kodstar-clang/native/bridge.h.
#pragma once
#include "bridge.h"
#include <clang/AST/ASTContext.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Lex/Preprocessor.h>
#include <clang/Sema/Sema.h>
#include <clang/Sema/TemplateInstCallback.h>

namespace ks {
class WorkBudget {
  const KsInput &input;
  const KsCallbacks &callbacks;
  clang::CompilerInstance &compiler;
  bool &stopped;
  std::size_t tokens = 0, templates = 0;
  void abort() {
    if (stopped)
      return;
    stopped = true;
    auto id = compiler.getDiagnostics().getCustomDiagID(
        clang::DiagnosticsEngine::Fatal, "native task resource limit reached");
    compiler.getDiagnostics().Report(id);
  }
  void stop(unsigned kind, std::size_t used, std::size_t limit) {
    if (stopped)
      return;
    callbacks.issue(callbacks.state, kind, {nullptr, 0}, used, limit);
    abort();
  }

public:
  WorkBudget(const KsInput &input, const KsCallbacks &callbacks,
             clang::CompilerInstance &compiler, bool &stopped)
      : input(input), callbacks(callbacks), compiler(compiler),
        stopped(stopped) {}
  bool check() {
    if (stopped)
      return false;
    if (callbacks.check(callbacks.state) == 0) {
      abort();
      return false;
    }
    auto &sources = compiler.getSourceManager();
    auto buffers = sources.getMemoryBufferSizes();
    std::size_t bytes = buffers.malloc_bytes + buffers.mmap_bytes +
                        sources.getDataStructureSizes() +
                        compiler.getPreprocessor().getTotalMemory();
    if (compiler.hasASTContext()) {
      bytes += compiler.getASTContext().getASTAllocatedMemory() +
               compiler.getASTContext().getSideTableAllocatedMemory();
    }
    // Compiler arenas and owned source buffers are observable. This estimate
    // does not include every LLVM allocation and is not a process RSS limit.
    if (bytes > input.memory_limit)
      stop(6, bytes, input.memory_limit);
    return !stopped;
  }
  void token() {
    if (stopped)
      return;
    if (++tokens > input.token_limit) {
      stop(3, tokens, input.token_limit);
      return;
    }
    if (tokens % 256 == 0 && callbacks.check(callbacks.state) == 0) {
      abort();
      return;
    }
    if (tokens % 16384 == 0)
      check();
  }
  void instantiate() {
    if (stopped)
      return;
    if (++templates > input.template_limit) {
      stop(4, templates, input.template_limit);
      return;
    }
    if (templates % 16 == 0 && callbacks.check(callbacks.state) == 0) {
      abort();
      return;
    }
    if (templates % 512 == 0)
      check();
  }
};
class TemplateWork final : public clang::TemplateInstantiationCallback {
  std::shared_ptr<WorkBudget> budget;

public:
  explicit TemplateWork(std::shared_ptr<WorkBudget> budget)
      : budget(std::move(budget)) {}
  void initialize(const clang::Sema &) override {}
  void finalize(const clang::Sema &) override {}
  void atTemplateEnd(const clang::Sema &,
                     const clang::Sema::CodeSynthesisContext &) override {}
  void atTemplateBegin(const clang::Sema &,
                       const clang::Sema::CodeSynthesisContext &) override {
    budget->instantiate();
  }
};
} // namespace ks
