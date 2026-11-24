// Purpose: 대상 파일의 함수·enum 선언 USR와 실제 callee를 구조화된 근거로 방문.
// Pattern: AST Visitor.
// Usage: frontend consumer가 분석이 끝난 AST 수명 안에서 호출.
// Related: crates/kodstar-clang/native/bridge.h,
// crates/kodstar-clang/src/model.rs.

#include "bridge.h"
#include <clang/AST/RecursiveASTVisitor.h>
#include <clang/AST/TypeLoc.h>
#include <clang/Basic/SourceManager.h>
#include <clang/Lex/Lexer.h>
#include <clang/UnifiedSymbolResolution/USRGeneration.h>

namespace {
KsText view(const std::string &value) { return {value.data(), value.size()}; }
std::string identity(const clang::NamedDecl *decl) {
  if (auto *function = llvm::dyn_cast<clang::FunctionDecl>(decl))
    if (auto *pattern = function->getTemplateInstantiationPattern())
      decl = pattern;
  llvm::SmallString<256> usr;
  if (clang::index::generateUSRForDecl(decl->getCanonicalDecl(), usr))
    return {};
  return std::string(usr.str());
}
class Visitor final : public clang::RecursiveASTVisitor<Visitor> {
  clang::ASTContext &context;
  const KsCallbacks &callbacks;
  std::string caller;
  std::string target;
  bool calls;
  std::size_t steps = 0;
  std::size_t depth = 0;
  using Base = clang::RecursiveASTVisitor<Visitor>;
  bool enter() {
    // Bound our own recursive visitor separately from compiler token/template work.
    constexpr std::size_t max_depth = 128, max_steps = 4 * 1024 * 1024;
    ++steps;
    ++depth;
    if (depth > max_depth || steps > max_steps) {
      callbacks.issue(callbacks.state, depth > max_depth ? 7 : 8,
                      {nullptr, 0}, depth > max_depth ? depth : steps,
                      depth > max_depth ? max_depth : max_steps);
      --depth;
      return false;
    }
    if (steps % 128 == 0 && callbacks.check(callbacks.state) == 0) {
      --depth;
      return false;
    }
    return true;
  }

public:
  Visitor(clang::ASTContext &context, const KsCallbacks &callbacks, KsText target, bool calls)
      : context(context), callbacks(callbacks), target(target.data, target.size), calls(calls) {}
  bool TraverseDecl(clang::Decl *decl) {
    if (!decl || decl->isInvalidDecl())
      return true;
    if (!enter())
      return false;
    if (auto *function = llvm::dyn_cast<clang::FunctionDecl>(decl)) {
      std::string previous = std::move(caller);
      caller = identity(function);
      bool result = Base::TraverseDecl(decl);
      caller = std::move(previous);
      --depth;
      return result;
    }
    bool result = Base::TraverseDecl(decl);
    --depth;
    return result;
  }
  bool TraverseStmt(clang::Stmt *stmt, Base::DataRecursionQueue *queue = nullptr) {
    if (!stmt)
      return true;
    if (!enter())
      return false;
    bool result = Base::TraverseStmt(stmt, queue);
    --depth;
    return result;
  }
  bool TraverseType(clang::QualType type, bool traverse_qualifier = true) {
    if (!enter())
      return false;
    bool result = Base::TraverseType(type, traverse_qualifier);
    --depth;
    return result;
  }
  bool positions(clang::SourceRange range, std::size_t &begin, std::size_t &end,
                 bool full_macro = false) {
    auto &sources = context.getSourceManager();
    auto expanded = full_macro ? sources.getExpansionRange(range)
                               : clang::CharSourceRange::getTokenRange(range);
    auto first = sources.getExpansionLoc(expanded.getBegin());
    auto last = sources.getExpansionLoc(expanded.getEnd());
    if (first.isInvalid() || last.isInvalid() ||
        sources.getFilename(first) != target ||
        sources.getFilename(last) != target)
      return false;
    begin = sources.getFileOffset(first);
    if (expanded.isTokenRange())
      last = clang::Lexer::getLocForEndOfToken(last, 0, sources,
                                               context.getLangOpts());
    if (last.isInvalid())
      return false;
    end = sources.getFileOffset(last);
    return begin <= end;
  }
  clang::SourceLocation declaration_location(const clang::FunctionDecl *decl) {
    auto &sources = context.getSourceManager();
    auto spelling = sources.getSpellingLoc(decl->getLocation());
    auto expanded = sources.getExpansionRange(decl->getSourceRange());
    auto first = sources.getExpansionLoc(expanded.getBegin());
    auto last = sources.getExpansionLoc(expanded.getEnd());
    if (spelling.isValid() && first.isValid() && last.isValid() &&
        sources.isWrittenInSameFile(spelling, first) &&
        sources.getFileOffset(first) <= sources.getFileOffset(spelling) &&
        sources.getFileOffset(spelling) <= sources.getFileOffset(last))
      return spelling;
    return sources.getExpansionLoc(decl->getLocation());
  }
  bool VisitFunctionDecl(clang::FunctionDecl *decl) {
    if (decl->isImplicit())
      return true;
    KsFact fact{};
    fact.kind = 1;
    if (!positions(decl->getSourceRange(), fact.begin, fact.end, true))
      return true;
    auto name_location = declaration_location(decl);
    if (!positions({name_location, name_location}, fact.name_begin,
                   fact.name_end))
      return true;
    std::string name = decl->getNameAsString(),
                qualified = decl->getQualifiedNameAsString(),
                usr = identity(decl);
    if (usr.empty())
      return true;
    fact.flags = (decl->isThisDeclarationADefinition() ? 1 : 0) |
                 (decl->getLocation().isMacroID() ? 2 : 0);
    fact.name = view(name);
    fact.qualified = view(qualified);
    fact.usr = view(usr);
    if (auto *info = decl->getTypeSourceInfo()) {
      auto parameters = info->getTypeLoc().getAs<clang::FunctionProtoTypeLoc>();
      if (parameters) {
        auto &sources = context.getSourceManager();
        auto first = sources.getSpellingLoc(parameters.getLParenLoc());
        auto last = sources.getSpellingLoc(parameters.getRParenLoc());
        if (first.isValid() && last.isValid() && sources.getFilename(first) == target &&
            sources.getFilename(last) == target) {
          fact.parameter_begin = sources.getFileOffset(first);
          fact.parameter_end = sources.getFileOffset(last) + 1;
        }
      }
    }
    return callbacks.emit(callbacks.state, &fact) != 0;
  }
  bool enum_definition(clang::NamedDecl *decl, unsigned kind, bool defined) {
    if (decl->isImplicit() || decl->isInvalidDecl())
      return true;
    KsFact fact{};
    fact.kind = kind;
    if (!positions(decl->getSourceRange(), fact.begin, fact.end, true) ||
        !positions({decl->getLocation(), decl->getLocation()}, fact.name_begin, fact.name_end))
      return true;
    std::string name = decl->getNameAsString(), qualified = decl->getQualifiedNameAsString(),
                usr = identity(decl), parent;
    if (name.empty()) {
      name = "<anonymous@" + std::to_string(fact.begin) + ">";
      qualified = name;
    }
    if (auto *owner = llvm::dyn_cast<clang::EnumDecl>(decl->getDeclContext())) {
      parent = identity(owner);
      std::string prefix = owner->getQualifiedNameAsString();
      if (owner->getDeclName().isEmpty()) {
        std::size_t begin, end;
        if (positions(owner->getSourceRange(), begin, end, true))
          prefix = "<anonymous@" + std::to_string(begin) + ">";
      }
      qualified = prefix + "::" + name;
    }
    if (usr.empty())
      return true;
    fact.flags = (defined ? 1 : 0) | (decl->getLocation().isMacroID() ? 2 : 0);
    fact.name = view(name);
    fact.qualified = view(qualified);
    fact.usr = view(usr);
    // Declaration facts use the owner USR; call facts use the caller USR.
    fact.caller = view(parent);
    return callbacks.emit(callbacks.state, &fact) != 0;
  }
  bool VisitEnumDecl(clang::EnumDecl *decl) {
    return enum_definition(decl, 3, decl->isCompleteDefinition());
  }
  bool VisitEnumConstantDecl(clang::EnumConstantDecl *decl) {
    const auto *owner = llvm::cast<clang::EnumDecl>(decl->getDeclContext());
    return enum_definition(decl, owner->isScoped() ? 5 : 4, true);
  }
  bool VisitCallExpr(clang::CallExpr *call) {
    if (!calls)
      return true;
    const auto *target = call->getDirectCallee();
    if (!target)
      return true;
    KsFact fact{};
    fact.kind = 2;
    auto callee = call->getCallee()->IgnoreParenImpCasts();
    if (!positions(callee->getSourceRange(), fact.begin, fact.end))
      return true;
    std::string name = target->getNameAsString(),
                qualified = target->getQualifiedNameAsString(),
                usr = identity(target);
    if (usr.empty())
      return true;
    const auto *declaration = target->getCanonicalDecl();
    if (auto *pattern = declaration->getTemplateInstantiationPattern())
      declaration = pattern;
    auto &sources = context.getSourceManager();
    auto location = declaration_location(declaration);
    std::string file = location.isValid() ? sources.getFilename(location).str()
                                          : std::string();
    fact.target_file = view(file);
    fact.target_position =
        location.isValid() ? sources.getFileOffset(location) : 0;
    fact.name = view(name);
    fact.qualified = view(qualified);
    fact.usr = view(usr);
    fact.caller = view(caller);
    return callbacks.emit(callbacks.state, &fact) != 0;
  }
};
} // namespace

bool ks_visit(clang::ASTContext &context, const KsCallbacks &callbacks, KsText target, bool calls) {
  Visitor visitor(context, callbacks, target, calls);
  return visitor.TraverseDecl(context.getTranslationUnitDecl());
}
