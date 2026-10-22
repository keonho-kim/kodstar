// Purpose: 이동·복제된 프로젝트 등록과 제거 전 데이터 보호 검증.
// Pattern: Integration Test.
// Usage: cargo test -p kodstar-cli --test installation_data.
// Related: crates/kodstar-cli/tests/support/mod.rs, crates/kodstar-core/src/infrastructure/installation/removal.rs.

mod support;
use std::fs;
use support::*;

#[test]
fn moved_repository_is_registered_at_its_current_location() -> TestResult {
    let project = Project::new()?;
    project.write("fn moved() {}")?;
    assert!(project.run(&["init"])?.status.success());
    let moved = project.home.join("moved-project");
    fs::rename(&project.root, &moved)?;
    let output = project
        .command()
        .current_dir(&moved)
        .args(["history", "--no-refresh"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = project
        .command()
        .current_dir(&moved)
        .args(["uninstall", "--yes"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!moved.join(".kodstar").exists());
    assert!(moved.join("main.rs").exists());
    assert!(!project.home.join(".kodstar").exists());
    Ok(())
}

#[test]
fn repository_copies_are_all_kept_in_the_inventory() -> TestResult {
    let project = Project::new()?;
    project.write("fn copied() {}")?;
    assert!(project.run(&["init"])?.status.success());
    let copy = project.home.join("copy");
    fs::create_dir_all(copy.join(".kodstar"))?;
    fs::copy(
        project.root.join(".kodstar/repository.sqlite"),
        copy.join(".kodstar/repository.sqlite"),
    )?;
    fs::copy(project.root.join("main.rs"), copy.join("main.rs"))?;
    let output = project
        .command()
        .current_dir(&copy)
        .args(["history", "--no-refresh"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = project.run(&["uninstall", "--yes"])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.root.join(".kodstar").exists());
    assert!(!copy.join(".kodstar").exists());
    assert!(copy.join("main.rs").exists());
    Ok(())
}

#[test]
fn unknown_global_files_are_detected_before_any_project_history_is_removed() -> TestResult {
    let project = Project::new()?;
    project.write("fn preserved() {}")?;
    assert!(project.run(&["init"])?.status.success());
    let note = project.home.join(".kodstar/user-note.txt");
    fs::write(&note, "keep me")?;
    assert!(!project.run(&["uninstall", "--yes"])?.status.success());
    assert!(project.root.join(".kodstar/repository.sqlite").exists());
    assert_eq!(fs::read_to_string(&note)?, "keep me");
    fs::remove_file(note)?;
    assert!(project.run(&["uninstall", "--yes"])?.status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
fn dangling_history_links_are_preserved_with_an_error() -> TestResult {
    let project = Project::new()?;
    project.write("fn preserved() {}")?;
    assert!(project.run(&["init"])?.status.success());
    let history = project.root.join(".kodstar");
    let saved = project.home.join("saved-history");
    fs::rename(&history, &saved)?;
    std::os::unix::fs::symlink(project.home.join("absent"), &history)?;
    assert!(!project.run(&["uninstall", "--yes"])?.status.success());
    assert!(fs::symlink_metadata(&history)?.file_type().is_symlink());
    assert!(saved.join("repository.sqlite").exists());
    fs::remove_file(&history)?;
    fs::rename(saved, history)?;
    assert!(project.run(&["uninstall", "--yes"])?.status.success());
    Ok(())
}

#[test]
fn engine_api_rejects_filesystem_root_without_creating_data() -> TestResult {
    let project = Project::new()?;
    let cache = project.home.join("direct-engine-cache");
    let engine = kodstar_core::Engine::new(cache.clone())?;
    let result = engine.init(
        &kodstar_core::ProjectOptions::new("/".into()),
        kodstar_core::InitOptions::default(),
        &kodstar_core::Cancellation::default(),
        &mut |_| Ok(()),
    );
    assert!(result.is_err());
    assert!(!cache.exists());
    Ok(())
}
