// Purpose: compiler frontend의 동기 실행·토큰 예산·취소·진단 수명 소유.
// Pattern: Native Adapter.
// Usage: Rust가 정규화한 입력과 callback을 한 번 호출.
// Related: crates/kodstar-clang/native/bridge.h,
// crates/kodstar-clang/native/visitor.cpp.

#include "bridge.h"
#include "budget.h"
#include <clang/AST/ASTConsumer.h>
#include <clang/Basic/DiagnosticLex.h>
#include <clang/Basic/SourceManager.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/FrontendActions.h>
#include <clang/Lex/PPCallbacks.h>
#include <clang/Lex/Preprocessor.h>
#include <clang/Sema/SemaConsumer.h>
#include <clang/Sema/TemplateInstCallback.h>
#include <clang/Tooling/Tooling.h>
#include <llvm/Support/VirtualFileSystem.h>

namespace {
class Diagnostics final : public clang::DiagnosticConsumer {
  const KsCallbacks &callbacks;

public:
  explicit Diagnostics(const KsCallbacks &callbacks) : callbacks(callbacks) {}
  void HandleDiagnostic(clang::DiagnosticsEngine::Level level,
                        const clang::Diagnostic &info) override {
    clang::DiagnosticConsumer::HandleDiagnostic(level, info);
    if (level < clang::DiagnosticsEngine::Error)
      return;
    std::string subject;
    unsigned kind = 2;
    if (info.getID() == clang::diag::err_pp_file_not_found &&
        info.getNumArgs() > 0 &&
        info.getArgKind(0) == clang::DiagnosticsEngine::ak_std_string) {
      kind = 1;
      subject = info.getArgStdStr(0);
    } else {
      // Preserve actionable identity without source excerpts or user #error text.
      subject = "diagnostic " + std::to_string(info.getID());
      subject += ": " + info.getDiags()
                            ->getDiagnosticIDs()
                            ->getDescription(info.getID())
                            .str();
      if (info.hasSourceManager() && info.getLocation().isValid()) {
        auto location =
            info.getSourceManager().getPresumedLoc(info.getLocation());
        if (location.isValid())
          subject += " at " + std::string(location.getFilename()) + ":" +
                     std::to_string(location.getLine());
      }
    }
    callbacks.issue(callbacks.state, kind, {subject.data(), subject.size()}, 0,
                    0);
  }
};
class DeterministicMacros final : public clang::PPCallbacks {
  clang::DiagnosticsEngine &diagnostics;
  const KsCallbacks &callbacks;

public:
  DeterministicMacros(clang::DiagnosticsEngine &diagnostics, const KsCallbacks &callbacks)
      : diagnostics(diagnostics), callbacks(callbacks) {}
  void MacroExpands(const clang::Token &token, const clang::MacroDefinition &,
                    clang::SourceRange, const clang::MacroArgs *) override {
    const auto *identifier = token.getIdentifierInfo();
    if (!identifier)
      return;
    auto name = identifier->getName();
    if (name == "__DATE__" || name == "__TIME__" || name == "__TIMESTAMP__") {
      callbacks.issue(callbacks.state, 9, {nullptr, 0}, 0, 0);
      auto id = diagnostics.getCustomDiagID(
          clang::DiagnosticsEngine::Fatal,
          "clock dependent macros cannot provide reusable semantic facts");
      diagnostics.Report(token.getLocation(), id);
    }
  }
};

class Consumer final : public clang::SemaConsumer {
  std::shared_ptr<ks::WorkBudget> budget;
  const KsCallbacks &callbacks;
  clang::CompilerInstance &compiler;
  bool &complete, &stopped;
  KsText target;

public:
  Consumer(std::shared_ptr<ks::WorkBudget> budget, const KsCallbacks &callbacks,
           clang::CompilerInstance &compiler, bool &complete, bool &stopped, KsText target)
      : budget(std::move(budget)), callbacks(callbacks), compiler(compiler),
        complete(complete), stopped(stopped), target(target) {}
  void InitializeSema(clang::Sema &sema) override {
    sema.TemplateInstCallbacks.push_back(
        std::make_unique<ks::TemplateWork>(budget));
  }
  void HandleTranslationUnit(clang::ASTContext &context) override {
    if (stopped || !budget->check())
      return;
    // Valid declarations remain evidence even when another declaration failed.
    // Calls are trusted only in a compilation without recovery diagnostics.
    complete = ks_visit(context, callbacks, target,
                        !compiler.getDiagnostics().hasErrorOccurred());
  }
};
class Action final : public clang::ASTFrontendAction {
  const KsInput &input;
  const KsCallbacks &callbacks;
  bool &complete, &stopped;
  std::shared_ptr<ks::WorkBudget> budget;

public:
  Action(const KsInput &input, const KsCallbacks &callbacks, bool &complete,
         bool &stopped)
      : input(input), callbacks(callbacks), complete(complete),
        stopped(stopped) {}
  std::unique_ptr<clang::ASTConsumer>
  CreateASTConsumer(clang::CompilerInstance &compiler,
                    llvm::StringRef) override {
    compiler.getPreprocessor().addPPCallbacks(
        std::make_unique<DeterministicMacros>(compiler.getDiagnostics(), callbacks));
    budget =
        std::make_shared<ks::WorkBudget>(input, callbacks, compiler, stopped);
    compiler.getPreprocessor().setTokenWatcher(
        [active = budget](const clang::Token &) { active->token(); });
    return std::make_unique<Consumer>(budget, callbacks, compiler, complete,
                                      stopped, input.target);
  }
};
} // namespace

extern "C" int ks_clang_analyze(const KsInput *input,
                                const KsCallbacks *callbacks) {
  if (!input || !callbacks || callbacks->check(callbacks->state) == 0)
    return 2;
  auto fs = ks_filesystem(*input, *callbacks);
  auto files = llvm::makeIntrusiveRefCnt<clang::FileManager>(
      clang::FileSystemOptions{}, fs);
  std::vector<std::string> arguments = {"/kodstar",
                                        "-fsyntax-only",
                                        "-x",
                                        input->cpp ? "c++" : "c",
                                        input->cpp ? "-std=c++20" : "-std=c17",
                                        "-nostdinc",
                                        "-resource-dir=/__kodstar_clang",
                                        "-ferror-limit=1",
                                        "-fbracket-depth=256",
                                        "-ftemplate-depth=64",
                                        "-fconstexpr-depth=64",
                                        "-fconstexpr-steps=100000",
                                        "-fno-modules",
                                        "-fno-implicit-modules",
                                        "-fno-implicit-module-maps",
                                        "-Wno-everything",
                                        "-Werror=implicit-function-declaration",
                                        "-Werror=implicit-int",
                                        "-Werror=date-time",
                                        "-fno-caret-diagnostics"};
  for (std::size_t i = 0; i < input->argument_count; ++i)
    arguments.emplace_back(input->arguments[i]);
  arguments.emplace_back("-I/__kodstar_project");
  arguments.emplace_back("-I/__kodstar_project/include");
  // Project -I directories precede target library system headers. libc++
  // wrappers use include_next to reach the target's C ABI declarations.
  if (input->cpp) {
    arguments.emplace_back("-isystem");
    arguments.emplace_back("/__kodstar_clang/cxx");
  }
  arguments.emplace_back("-isystem");
  arguments.emplace_back("/__kodstar_clang/include");
  arguments.emplace_back("-isystem");
  arguments.emplace_back("/__kodstar_clang/system");
  arguments.emplace_back(input->path.data, input->path.size);
  bool complete = false, stopped = false;
  Diagnostics diagnostics(*callbacks);
  clang::tooling::ToolInvocation invocation(
      arguments,
      std::make_unique<Action>(*input, *callbacks, complete, stopped),
      files.get());
  invocation.setDiagnosticConsumer(&diagnostics);
  bool success = invocation.run();
  if (stopped)
    return 2;
  if (!success)
    return 1;
  return complete ? 0 : 2;
}
