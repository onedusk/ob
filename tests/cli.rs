//! End-to-end integration tests driving the real `oober` binary.
//!
//! These tests assert on exit codes and resulting file contents — the tool's
//! actual contract — not on incidental stdout wording. Scan results are
//! written to a file via `-o` so assertions target the formatted output
//! rather than progress chatter.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn ob() -> Command {
    Command::cargo_bin("oober").expect("oober binary must build")
}

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

#[test]
fn scan_finds_pattern_text_format() {
    let temp = TempDir::new().unwrap();
    // Keep the patterns file and the output file outside the scanned tree so
    // their own contents cannot produce extra matches.
    let scanme = temp.path().join("scanme");
    fs::create_dir(&scanme).unwrap();
    fs::write(scanme.join("src.txt"), "hello TODO world\nno match here\n").unwrap();
    let patterns = temp.path().join("patterns.yaml");
    fs::write(
        &patterns,
        "patterns:\n  - name: todo\n    pattern: 'TODO'\n",
    )
    .unwrap();
    let out = temp.path().join("results.txt");

    ob().arg("scan")
        .arg("-p")
        .arg(&patterns)
        .arg("-o")
        .arg(&out)
        .arg(&scanme)
        .assert()
        .success();

    let results = fs::read_to_string(&out).unwrap();
    assert!(results.contains("todo"), "must name the pattern: {results}");
    assert!(
        results.contains("src.txt:1"),
        "must give file:line: {results}"
    );
    assert!(
        results.contains("hello TODO world"),
        "must include the matched line: {results}"
    );
    assert!(
        !results.contains("no match here"),
        "non-matching lines must not appear: {results}"
    );
}

#[test]
fn scan_finds_pattern_json_format() {
    let temp = TempDir::new().unwrap();
    let scanme = temp.path().join("scanme");
    fs::create_dir(&scanme).unwrap();
    fs::write(scanme.join("src.txt"), "hello TODO world\nno match here\n").unwrap();
    let out = temp.path().join("results.json");

    ob().arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("-f")
        .arg("json")
        .arg("-o")
        .arg(&out)
        .arg(&scanme)
        .assert()
        .success();

    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap())
        .expect("output must be valid JSON");
    assert_eq!(json["total_matches"], 1);
    let m = &json["matches"][0];
    assert_eq!(m["line"], 1);
    assert!(
        m["file"].as_str().unwrap().ends_with("src.txt"),
        "match must point at the scanned file: {m}"
    );
    assert!(m["content"].as_str().unwrap().contains("TODO"));
}

#[test]
fn scan_reports_resolved_patterns_file_on_stderr_not_in_results() {
    let temp = TempDir::new().unwrap();
    let scanme = temp.path().join("scanme");
    fs::create_dir(&scanme).unwrap();
    fs::write(scanme.join("src.txt"), "hello TODO world\n").unwrap();
    let patterns = temp.path().join("patterns.yaml");
    fs::write(
        &patterns,
        "patterns:\n  - name: todo\n    pattern: 'TODO'\n",
    )
    .unwrap();

    let assert = ob()
        .arg("scan")
        .arg("-p")
        .arg(&patterns)
        .arg(&scanme)
        .assert()
        .success()
        .stderr(contains(patterns.to_str().unwrap()));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("hello TODO world"),
        "results must be on stdout: {stdout}"
    );
    assert!(
        !stdout.contains(patterns.to_str().unwrap()),
        "the resolved config path must not pollute the results stream: {stdout}"
    );
}

#[test]
fn scan_missing_patterns_file_errors_without_user_config_fallback() {
    // Audit finding 13 repro: `scan -p` routed the patterns file through the
    // config search chain, so a typo'd/missing local file silently fell back
    // to a same-named stale file in $XDG_CONFIG_HOME/oober — wrong scan
    // results with exit 0. The patterns file must resolve only as given
    // (absolute, or relative to the CWD).
    let temp = TempDir::new().unwrap();
    let xdg = temp.path().join("xdg");
    fs::create_dir_all(xdg.join("oober")).unwrap();
    fs::write(
        xdg.join("oober").join("missing.yaml"),
        "patterns:\n  - name: stale\n    pattern: 'STALE'\n",
    )
    .unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("src.txt"), "hello TODO world\n").unwrap();

    ob().current_dir(&work)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("scan")
        .arg("-p")
        .arg("missing.yaml")
        .arg(".")
        .assert()
        .failure()
        .stderr(contains("missing.yaml"))
        .stderr(contains("-e"));

    // A patterns file that does exist relative to the CWD still works.
    fs::write(
        work.join("local.yaml"),
        "patterns:\n  - name: todo\n    pattern: 'TODO'\n",
    )
    .unwrap();
    ob().current_dir(&work)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("scan")
        .arg("-p")
        .arg("local.yaml")
        .arg("src.txt")
        .assert()
        .success()
        .stdout(contains("hello TODO world"))
        .stderr(contains("local.yaml"));
}

#[cfg(unix)]
#[test]
fn scan_warns_and_continues_on_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    // Audit finding 5 repro: an unreadable *file* (vs directory) killed the
    // whole scan with `Error: Io(... PermissionDenied ...)` and exit 1.
    let temp = TempDir::new().unwrap();
    let scanme = temp.path().join("scanme");
    fs::create_dir(&scanme).unwrap();
    fs::write(scanme.join("ok.txt"), "hello TODO world\n").unwrap();
    let locked = scanme.join("locked.txt");
    fs::write(&locked, "TODO locked\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    // Streaming path (text to stdout).
    ob().arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg(&scanme)
        .assert()
        .success()
        .stdout(contains("ok.txt"))
        .stderr(contains("locked.txt"));

    // Collected path (json to a file outside the scanned tree).
    let out = temp.path().join("results.json");
    ob().arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("-f")
        .arg("json")
        .arg("-o")
        .arg(&out)
        .arg(&scanme)
        .assert()
        .success()
        .stderr(contains("locked.txt"));

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

    let json = fs::read_to_string(&out).unwrap();
    assert!(json.contains("ok.txt"), "{json}");
    assert!(
        !json.contains("TODO locked"),
        "the unreadable file must be skipped, not matched: {json}"
    );
}

#[test]
fn errors_are_printed_in_display_form() {
    // `main` returning Result printed errors in Debug form, e.g.
    // `Error: Config("could not open ...")`; they must use Display.
    let temp = TempDir::new().unwrap();
    let missing = temp.path().join("no_such_patterns.yaml");

    let assert = ob()
        .arg("scan")
        .arg("-p")
        .arg(&missing)
        .arg(temp.path())
        .assert()
        .failure()
        .code(1)
        .stderr(contains(
            "Error: Config error: could not open patterns file",
        ));

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        !stderr.contains("Config(\""),
        "the Debug form must not surface: {stderr}"
    );
}

#[test]
fn incremental_scan_with_empty_xdg_cache_home_does_not_cache_in_project() {
    // A set-but-empty XDG_CACHE_HOME resolved the cache dir to the relative
    // "./oober", dropping a plaintext cache of matched lines (potentially
    // secrets) inside the scanned project, where it can get committed.
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("src.txt"), "hello TODO world\n").unwrap();

    ob().current_dir(&work)
        .env("XDG_CACHE_HOME", "")
        .env("HOME", &home)
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("--incremental")
        .arg(".")
        .assert()
        .success();

    assert!(
        !work.join("oober").exists(),
        "an empty XDG_CACHE_HOME must not put the scan cache inside the scanned project"
    );
}

#[test]
fn unknown_scan_format_is_rejected_at_parse_time() {
    let temp = TempDir::new().unwrap();
    // clap usage errors exit with code 2; an unknown format must not silently
    // fall back to text output.
    ob().arg("scan")
        .arg("-f")
        .arg("jsonn")
        .arg(temp.path())
        .assert()
        .code(2);
}

#[test]
fn workers_env_var_is_oober_workers() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("t.txt"), "TODO\n").unwrap();

    // The renamed variable is actually read: an invalid value must fail at
    // parse time (clap usage errors exit with code 2).
    ob().env("OOBER_WORKERS", "not-a-number")
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg(temp.path())
        .assert()
        .code(2);

    // The old UBER_SCANNER_WORKERS name is no longer consulted.
    ob().env_remove("OOBER_WORKERS")
        .env("UBER_SCANNER_WORKERS", "not-a-number")
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg(temp.path())
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// replace / undo
// ---------------------------------------------------------------------------

#[test]
fn replace_undo_round_trip_restores_content_with_gitignored_backups() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();

    // The audit's C1 setup: a git repo whose .gitignore covers "*.bak".
    // Backup discovery must not depend on git filters.
    match std::process::Command::new("git")
        .arg("init")
        .current_dir(dir)
        .output()
    {
        Ok(out) if out.status.success() => {}
        _ => {
            eprintln!("git unavailable; skipping gitignored-backup round-trip test");
            return;
        }
    }
    fs::write(dir.join(".gitignore"), "*.bak\n").unwrap();

    let file = dir.join("a.txt");
    let original = "alpha one\nalpha two\n";
    fs::write(&file, original).unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\nbeta two\n");
    assert_eq!(
        fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
        original,
        "backup must hold the pre-replace original"
    );

    ob().arg("undo").arg("-d").arg(dir).assert().success();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "undo must restore the original content even when *.bak is gitignored"
    );
    assert!(
        !dir.join("a.txt.bak").exists(),
        "undo consumes the backup by default"
    );
}

#[test]
fn replace_dry_run_leaves_files_byte_identical() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("t.txt");
    // Mixed line endings and no trailing newline: any rewrite would show.
    let original = b"alpha one\r\nalpha two".to_vec();
    fs::write(&file, &original).unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--dry-run")
        .assert()
        .success();

    assert_eq!(
        fs::read(&file).unwrap(),
        original,
        "--dry-run must not modify the file"
    );
    assert!(
        !dir.join("t.txt.bak").exists(),
        "--dry-run must not create backups"
    );
}

#[test]
fn replace_dry_run_previews_changed_lines() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("t.txt");
    let original = b"alpha one\nkeep me\nalpha two\n".to_vec();
    fs::write(&file, &original).unwrap();

    let assert = ob()
        .arg("replace")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(contains("- alpha one"))
        .stdout(contains("+ beta one"))
        .stdout(contains("- alpha two"))
        .stdout(contains("+ beta two"));

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("keep me"),
        "unchanged lines must not appear in the preview: {stdout}"
    );

    assert_eq!(
        fs::read(&file).unwrap(),
        original,
        "--dry-run must not modify the file"
    );
    assert!(
        !dir.join("t.txt.bak").exists(),
        "--dry-run must not create backups"
    );
}

#[test]
fn mismatched_patterns_replacements_config_is_rejected_and_file_untouched() {
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    let file = work.join("t.txt");
    let original = "line AAA\nkeep me\nline BBB\ndo not delete\n";
    fs::write(&file, original).unwrap();
    let config = temp.path().join("bad_config.yaml");
    fs::write(
        &config,
        "patterns:\n  - \"AAA\"\n  - \"BBB\"\nreplacements:\n  - \"XXX\"\n",
    )
    .unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(&work)
        .arg("-c")
        .arg(&config)
        .assert()
        .failure()
        .stderr(contains("replacements"));

    assert_eq!(
        fs::read(&file).unwrap(),
        original.as_bytes(),
        "file must be byte-identical after a rejected config"
    );
    assert!(
        !work.join("t.txt.bak").exists(),
        "no backup may be created for a rejected config"
    );
}

#[test]
fn all_invalid_extension_filter_is_rejected_for_replace_and_scan() {
    // Audit repro: `-x .` normalizes to an empty extension set, and an empty
    // set means "process every file" — so `ob replace -d . -x . -p foo -r bar`
    // rewrote extensionless and .py files alike. An explicit filter whose
    // entries all normalize away must be a hard error for both subcommands.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("a.py"), "foo line\n").unwrap();
    fs::write(work.join("noext"), "foo line\n").unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(&work)
        .arg("-x")
        .arg(".")
        .arg("-p")
        .arg("foo")
        .arg("-r")
        .arg("bar")
        .assert()
        .failure()
        .stderr(contains("no valid extensions"));

    assert_eq!(fs::read_to_string(work.join("a.py")).unwrap(), "foo line\n");
    assert_eq!(
        fs::read_to_string(work.join("noext")).unwrap(),
        "foo line\n"
    );

    ob().arg("scan")
        .arg("-e")
        .arg("foo")
        .arg("-x")
        .arg(".")
        .arg(&work)
        .assert()
        .failure()
        .stderr(contains("no valid extensions"));

    // A normal filter still works and stays scoped to its extension.
    ob().arg("replace")
        .arg("-d")
        .arg(&work)
        .arg("-x")
        .arg("py")
        .arg("-p")
        .arg("foo")
        .arg("-r")
        .arg("bar")
        .arg("--no-backup")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(work.join("a.py")).unwrap(), "bar line\n");
    assert_eq!(
        fs::read_to_string(work.join("noext")).unwrap(),
        "foo line\n"
    );
}

#[test]
fn crlf_preserved_through_null_replacement_line_deletion() {
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    let file = work.join("t.txt");
    fs::write(&file, "one\r\nMARKER\r\ndelete me\r\nfour\r\n").unwrap();
    // Config lives outside the processed dir so its own MARKER text is not
    // itself rewritten.
    let config = temp.path().join("delete_config.yaml");
    fs::write(
        &config,
        "patterns:\n  - \"MARKER\"\nreplacements:\n  - null\n",
    )
    .unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(&work)
        .arg("-c")
        .arg(&config)
        .arg("--no-backup")
        .assert()
        .success();

    assert_eq!(
        fs::read(&file).unwrap(),
        b"one\r\nMARKER\r\nfour\r\n".to_vec(),
        "exactly the line after the match is deleted and CRLF endings are kept"
    );
}

// ---------------------------------------------------------------------------
// presets
// ---------------------------------------------------------------------------

#[test]
fn trim_whitespace_preset_strips_interior_trailing_whitespace() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("t.rs");
    fs::write(&file, "fn main() {   \n\tlet x = 1;\t\t\n}\n").unwrap();

    // The documented kebab-case preset name must be accepted (audit C6) and
    // must actually strip interior trailing whitespace (audit C5).
    ob().arg("replace")
        .arg("-d")
        .arg(dir)
        .arg("--preset")
        .arg("trim-whitespace")
        .arg("--no-backup")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn main() {\n\tlet x = 1;\n}\n"
    );
}

#[test]
fn replace_echoes_kebab_case_preset_name() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("a.js"), "let x = 1;\n").unwrap();

    // The confirmation line must echo the name the CLI accepts
    // (`remove-todos`), not the Debug form (`RemoveTodos`), which clap
    // rejects if a user copies it back into --preset.
    let assert = ob()
        .arg("replace")
        .arg("-d")
        .arg(temp.path())
        .arg("--preset")
        .arg("remove-todos")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(contains("Using preset: remove-todos"));
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("RemoveTodos"),
        "PascalCase preset name must not surface: {stdout}"
    );
}

#[test]
fn list_presets_runs_without_dir_and_lists_every_preset() {
    let assert = ob().arg("replace").arg("--list-presets").assert().success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    for name in [
        "remove-copyright",
        "clean-debug",
        "remove-todos",
        "trim-whitespace",
        "remove-empty-comments",
        "tabs-to-spaces",
        "spaces-to-tabs",
    ] {
        assert!(stdout.contains(name), "listing must name {name}: {stdout}");
    }
    assert!(
        stdout.contains("trailing whitespace"),
        "each preset line must carry its description: {stdout}"
    );
}

#[test]
fn replace_without_dir_or_list_presets_is_a_helpful_error() {
    ob().arg("replace")
        .arg("--preset")
        .arg("trim-whitespace")
        .assert()
        .failure()
        .stderr(contains("--dir"))
        .stderr(contains("--list-presets"));
}

#[test]
fn unknown_preset_is_rejected_at_parse_time() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("t.txt");
    fs::write(&file, "unchanged\n").unwrap();

    ob().arg("replace")
        .arg("-d")
        .arg(temp.path())
        .arg("--preset")
        .arg("no-such-preset")
        .assert()
        .code(2);

    assert_eq!(fs::read_to_string(&file).unwrap(), "unchanged\n");
}

// ---------------------------------------------------------------------------
// rename
// ---------------------------------------------------------------------------

#[test]
fn rename_collision_keeps_both_files_and_exits_nonzero() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("file_1.txt"), "one").unwrap();
    fs::write(dir.join("file_2.txt"), "two").unwrap();

    ob().arg("rename")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg(r"file_\d")
        .arg("-r")
        .arg("merged")
        .arg("-w")
        .arg("1")
        .assert()
        .failure();

    // One file won the rename; the loser is skipped with its content intact.
    let survivors: Vec<&Path> = [Path::new("file_1.txt"), Path::new("file_2.txt")]
        .into_iter()
        .filter(|n| dir.join(n).exists())
        .collect();
    assert_eq!(survivors.len(), 1, "exactly one original may be renamed");
    let mut contents = vec![
        fs::read_to_string(dir.join("merged.txt")).unwrap(),
        fs::read_to_string(dir.join(survivors[0])).unwrap(),
    ];
    contents.sort();
    assert_eq!(
        contents,
        ["one", "two"],
        "no file content may be lost in a collision"
    );
}

#[test]
fn rename_collision_with_default_workers_never_loses_data() {
    // Audit C3 verify-repro: under the old parallel execution, the per-file
    // existence check was a check-then-rename race — both files passed it
    // before either rename ran, and one file's content was silently destroyed
    // in most runs. Renames now execute serially with duplicate targets
    // rejected at plan time; loop to catch a reintroduced race.
    for _ in 0..10 {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        fs::write(dir.join("file_1.txt"), "content ONE").unwrap();
        fs::write(dir.join("file_2.txt"), "content TWO").unwrap();

        ob().arg("rename")
            .arg("-d")
            .arg(dir)
            .arg("-p")
            .arg(r"file_\d")
            .arg("-r")
            .arg("merged")
            .assert()
            .failure();

        let survivors: Vec<&Path> = [Path::new("file_1.txt"), Path::new("file_2.txt")]
            .into_iter()
            .filter(|n| dir.join(n).exists())
            .collect();
        assert_eq!(survivors.len(), 1, "exactly one original may be renamed");
        let mut contents = vec![
            fs::read_to_string(dir.join("merged.txt")).unwrap(),
            fs::read_to_string(dir.join(survivors[0])).unwrap(),
        ];
        contents.sort();
        assert_eq!(
            contents,
            ["content ONE", "content TWO"],
            "no file content may be lost under parallel execution"
        );
    }
}
