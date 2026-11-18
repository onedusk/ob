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
use std::sync::OnceLock;
use tempfile::TempDir;

/// A per-process directory standing in for the user's cache and config
/// dirs. It lives under the cargo target dir and is never dropped, so it
/// stays valid for every test in this process.
fn isolated_xdg_home() -> &'static Path {
    static XDG_HOME: OnceLock<TempDir> = OnceLock::new();
    XDG_HOME
        .get_or_init(|| {
            TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).expect("temp dir must be creatable")
        })
        .path()
}

/// The `oober` binary with `XDG_CACHE_HOME` and `XDG_CONFIG_HOME` pointed at
/// a temp dir, so no test can read or write the real user cache or config
/// dir. A test that sets its own values afterwards keeps them.
fn ob() -> Command {
    let mut cmd = Command::cargo_bin("oober").expect("oober binary must build");
    cmd.env("XDG_CACHE_HOME", isolated_xdg_home().join("cache"))
        .env("XDG_CONFIG_HOME", isolated_xdg_home().join("config"));
    cmd
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
fn ob_helper_keeps_the_scan_cache_out_of_the_user_cache_dir() {
    // Without the helper's XDG defaults an incremental scan writes into the
    // platform cache dir under HOME, i.e. the real user cache on a dev
    // machine.
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    fs::create_dir(&home).unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("src.txt"), "hello TODO world\n").unwrap();

    ob().env("HOME", &home)
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("--incremental")
        .arg(&work)
        .assert()
        .success();

    assert_eq!(
        fs::read_dir(&home).unwrap().count(),
        0,
        "the scan cache must not land under HOME"
    );
    assert!(
        isolated_xdg_home().join("cache").join("oober").is_dir(),
        "the scan cache must land in the helper's temp dir"
    );
}

/// Returns the contents of the single per-project cache file under
/// `<cache>/oober/`.
fn read_cache_file(cache: &Path) -> String {
    let mut entries: Vec<_> = fs::read_dir(cache.join("oober"))
        .expect("cache dir must exist")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one cache file");
    fs::read_to_string(entries.pop().unwrap()).unwrap()
}

#[test]
fn incremental_no_cache_content_keeps_matched_text_out_of_cache() {
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    let secret = "hunter2-FAKE-SECRET-TOKEN";
    fs::write(
        work.join("secret.txt"),
        format!("password = {secret}\nplain line\n"),
    )
    .unwrap();

    let scan = |incremental: bool, out: &Path| {
        let mut cmd = ob();
        cmd.env("XDG_CACHE_HOME", &cache)
            .arg("scan")
            .arg("-e")
            .arg("FAKE-SECRET-TOKEN")
            .arg("-w")
            .arg("1")
            .arg("-o")
            .arg(out);
        if incremental {
            cmd.arg("--incremental").arg("--no-cache-content");
        }
        cmd.arg(&work);
        cmd
    };

    // Cold run: the match is reported, but the cache on disk must hold only
    // its location, never the matched text.
    let out1 = temp.path().join("out1.txt");
    scan(true, &out1).assert().success();
    assert!(
        fs::read_to_string(&out1).unwrap().contains(secret),
        "the scan results must still show the matched line"
    );
    let cached = read_cache_file(&cache);
    assert!(
        !cached.contains(secret),
        "the cache must not contain the matched text: {cached}"
    );
    assert!(
        cached.contains("secret.txt"),
        "the cache must still record the match location: {cached}"
    );

    // Warm run (cache hit): the emitted results reconstruct the full matched
    // line and are identical to a fresh non-incremental scan.
    let out2 = temp.path().join("out2.txt");
    scan(true, &out2).assert().success();
    let fresh = temp.path().join("fresh.txt");
    scan(false, &fresh).assert().success();
    let warm_results = fs::read_to_string(&out2).unwrap();
    assert!(
        warm_results.contains(&format!("password = {secret}")),
        "a cache hit must reconstruct the full matched line: {warm_results}"
    );
    assert_eq!(
        warm_results,
        fs::read_to_string(&fresh).unwrap(),
        "warm incremental output must match a fresh non-incremental scan"
    );
}

#[test]
fn incremental_no_cache_content_falls_back_to_rescan_when_cached_line_vanishes() {
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    let file = work.join("s.txt");
    let original = "padding line\nkey AKIAAAAAAAAAAAAAAAAA\n";
    fs::write(&file, original).unwrap();

    let scan = || {
        let mut cmd = ob();
        cmd.env("XDG_CACHE_HOME", &cache)
            .arg("scan")
            .arg("-e")
            .arg("AKIA[0-9A-Z]{16}")
            .arg("--incremental")
            .arg("--no-cache-content")
            .arg(&work);
        cmd
    };

    // Prime the content-less cache (match recorded on line 2).
    scan()
        .assert()
        .success()
        .stdout(contains("AKIAAAAAAAAAAAAAAAAA"));

    // Rewrite the file with the same byte length but a single line, so the
    // recorded line 2 no longer exists, and restore the modification time so
    // the quick fingerprint still reports the file unchanged.
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let replacement = "key AKIABBBBBBBBBBBBBBBBxxxxxxxxxxxxxx";
    assert_eq!(
        replacement.len(),
        original.len(),
        "the rewrite must keep the file size for the fingerprint to hold"
    );
    fs::write(&file, replacement).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    // The warm run must not fail: reconstruction fails, warns, and the file
    // is re-scanned fresh — surfacing its current content.
    scan()
        .assert()
        .success()
        .stdout(contains("AKIABBBBBBBBBBBBBBBB"))
        .stderr(contains("re-scanning"));
}

#[test]
fn no_cache_content_without_incremental_is_rejected() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("t.txt");
    fs::write(&file, "TODO\n").unwrap();

    ob().arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("--no-cache-content")
        .arg(temp.path())
        .assert()
        .failure()
        .stderr(contains("--no-cache-content"))
        .stderr(contains("--incremental"));
}

/// Makes `path` unreadable (mode 000). Returns false, with the mode put
/// back, when the file can still be opened: the tests run as root, where
/// the mode does not block reads.
#[cfg(unix)]
fn make_unreadable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(path).is_ok() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        return false;
    }
    true
}

#[cfg(unix)]
#[test]
fn incremental_scan_does_not_cache_a_file_it_could_not_read() {
    use std::os::unix::fs::PermissionsExt;

    // A file that failed to scan was saved in the cache as scanned with no
    // matches. Making it readable changes neither its size nor its
    // modification time, so every later run skipped it: a false all-clear.
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("ok.txt"), "hello TODO world\n").unwrap();
    let locked = work.join("locked.txt");
    fs::write(&locked, "TODO locked\n").unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    let scan = || {
        let mut cmd = ob();
        cmd.env("XDG_CACHE_HOME", &cache)
            .arg("scan")
            .arg("-e")
            .arg("TODO")
            .arg("--incremental")
            .arg(&work);
        cmd
    };

    let first = scan().assert();
    // Restore permissions before asserting so TempDir cleanup succeeds.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    first
        .success()
        .stdout(contains("ok.txt"))
        .stderr(contains("locked.txt"));

    // The file is readable now and was never scanned: it must be scanned.
    scan().assert().success().stdout(contains("TODO locked"));
}

#[cfg(unix)]
#[test]
fn incremental_content_hash_scan_survives_an_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    // With --content-hash, saving the cache hashed every file again, so one
    // unreadable file failed the whole run after the results were written.
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("ok.txt"), "hello TODO world\n").unwrap();
    let locked = work.join("locked.txt");
    fs::write(&locked, "TODO locked\n").unwrap();
    if !make_unreadable(&locked) {
        return;
    }

    let assert = ob()
        .env("XDG_CACHE_HOME", &cache)
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("--incremental")
        .arg("--content-hash")
        .arg(&work)
        .assert();
    // Restore permissions before asserting so TempDir cleanup succeeds.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    assert
        .success()
        .stdout(contains("ok.txt"))
        .stderr(contains("locked.txt"));

    let cached = read_cache_file(&cache);
    assert!(
        cached.contains("ok.txt"),
        "the cache must still be saved for the readable file: {cached}"
    );
    assert!(
        !cached.contains("locked.txt"),
        "the unreadable file must stay out of the cache: {cached}"
    );
}

#[cfg(unix)]
#[test]
fn incremental_scan_survives_a_cached_file_that_became_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    // Change detection hashes a cached file under --content-hash. When that
    // failed, the error ended the whole scan before any file was scanned.
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("ok.txt"), "hello TODO world\n").unwrap();
    let locked = work.join("locked.txt");
    fs::write(&locked, "TODO locked\n").unwrap();

    let scan = || {
        let mut cmd = ob();
        cmd.env("XDG_CACHE_HOME", &cache)
            .arg("scan")
            .arg("-e")
            .arg("TODO")
            .arg("--incremental")
            .arg("--content-hash")
            .arg(&work);
        cmd
    };

    // Prime the cache while both files are readable.
    scan().assert().success().stdout(contains("TODO locked"));

    if !make_unreadable(&locked) {
        return;
    }
    let second = scan().assert();
    // Restore permissions before asserting so TempDir cleanup succeeds.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
    second
        .success()
        .stdout(contains("ok.txt"))
        .stderr(contains("locked.txt"));

    // The file was not scanned on the second run, so it must not be cached
    // as unchanged: readable again, it is scanned and its match is back.
    scan().assert().success().stdout(contains("TODO locked"));
}

#[test]
fn scan_overlapping_inputs_report_each_match_once() {
    // `.` and `sub` reach the same file under two spellings (`./sub/a.txt`
    // and `sub/a.txt`); every match was printed once per spelling.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir_all(work.join("sub")).unwrap();
    fs::write(work.join("sub").join("a.txt"), "hello TODO world\n").unwrap();

    let assert = ob()
        .current_dir(&work)
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg(".")
        .arg("sub")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "the match must be reported once: {stdout}"
    );
    assert!(
        stdout.contains("./sub/a.txt:1"),
        "the first spelling of the path must be kept: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn scan_reports_a_file_and_a_symlink_to_it_once() {
    // A symlink and its target are one file. Which of the two spellings is
    // kept depends on the walk order, so only the count is asserted.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("a.txt"), "hello TODO world\n").unwrap();
    std::os::unix::fs::symlink("a.txt", work.join("link.txt")).unwrap();

    let assert = ob()
        .arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg(&work)
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "the match must be reported once: {stdout}"
    );
}

#[test]
fn incremental_scan_overlapping_inputs_do_not_multiply_cached_matches() {
    // A file named explicitly and found again in an input directory was
    // listed twice, so each run read its cached matches twice and saved the
    // doubled list: the match count doubled on every run.
    let temp = TempDir::new().unwrap();
    let cache = temp.path().join("cache");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    let file = work.join("a.txt");
    fs::write(&file, "hello TODO world\n").unwrap();

    for run in 1..=3 {
        let assert = ob()
            .env("XDG_CACHE_HOME", &cache)
            .arg("scan")
            .arg("-e")
            .arg("TODO")
            .arg("--incremental")
            .arg(&work)
            .arg(&file)
            .assert()
            .success();

        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        assert_eq!(
            stdout.lines().count(),
            1,
            "run {run} must report the match once: {stdout}"
        );
    }
}

#[test]
fn scan_keeps_the_existing_output_file_when_an_input_is_missing() {
    // The output file was created, and so truncated, before the inputs were
    // collected: a typo in an input path wiped the previous results.
    let temp = TempDir::new().unwrap();
    let out = temp.path().join("results.txt");
    fs::write(&out, "previous results\n").unwrap();

    ob().arg("scan")
        .arg("-e")
        .arg("TODO")
        .arg("-o")
        .arg(&out)
        .arg(temp.path().join("no_such_dir"))
        .assert()
        .failure()
        .stderr(contains("Input path not found"));

    assert_eq!(fs::read_to_string(&out).unwrap(), "previous results\n");
}

/// Fills `dir` with enough matching lines that the scan output is far
/// larger than the output writer's buffer, so results reach the output file
/// while the scan is still running.
fn write_many_todo_files(dir: &Path) {
    for i in 0..40 {
        let content: String = (0..200).map(|n| format!("TODO item {n}\n")).collect();
        fs::write(dir.join(format!("f{i}.txt")), content).unwrap();
    }
}

#[test]
fn scan_never_scans_its_own_output_file() {
    // With the output file inside the scanned directory, the scan read its
    // own results while writing them, and reported them as matches.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    write_many_todo_files(&work);
    let out = work.join("results.txt");

    // The second run starts with the first run's results in place.
    for run in 1..=2 {
        ob().arg("scan")
            .arg("-e")
            .arg("TODO")
            .arg("-w")
            .arg("1")
            .arg("-o")
            .arg(&out)
            .arg(&work)
            .timeout(std::time::Duration::from_secs(60))
            .assert()
            .success();

        let results = fs::read_to_string(&out).unwrap();
        assert_eq!(
            results.lines().count(),
            40 * 200,
            "run {run} must report exactly the matches of the scanned files"
        );
        assert!(
            !results.contains("results.txt"),
            "run {run} must not report the output file"
        );
    }
}

#[test]
fn scan_never_scans_its_own_output_file_given_as_a_relative_path() {
    // `-o results.txt` has no directory part; the output file must still be
    // recognized among the files found under `.`.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    write_many_todo_files(&work);

    for run in 1..=2 {
        ob().current_dir(&work)
            .arg("scan")
            .arg("-e")
            .arg("TODO")
            .arg("-w")
            .arg("1")
            .arg("-o")
            .arg("results.txt")
            .arg(".")
            .timeout(std::time::Duration::from_secs(60))
            .assert()
            .success();

        let results = fs::read_to_string(work.join("results.txt")).unwrap();
        assert_eq!(
            results.lines().count(),
            40 * 200,
            "run {run} must report exactly the matches of the scanned files"
        );
        assert!(
            !results.contains("results.txt"),
            "run {run} must not report the output file"
        );
    }
}

#[test]
fn scan_config_exclusions_skip_directories_even_with_no_ignore() {
    // `exclusions` used to be documented but read by no code, so vendor/
    // was scanned. The patterns file lives outside the scanned tree.
    let temp = TempDir::new().unwrap();
    let scanme = temp.path().join("scanme");
    fs::create_dir_all(scanme.join("vendor").join("lib")).unwrap();
    fs::create_dir_all(scanme.join("src")).unwrap();
    fs::write(scanme.join("src").join("app.txt"), "TODO app\n").unwrap();
    fs::write(scanme.join("vendor").join("dep.txt"), "TODO dep\n").unwrap();
    fs::write(
        scanme.join("vendor").join("lib").join("deep.txt"),
        "TODO deep\n",
    )
    .unwrap();
    let patterns = temp.path().join("patterns.yaml");
    fs::write(
        &patterns,
        "patterns:\n  - name: todo\n    pattern: 'TODO'\nexclusions:\n  - vendor\n",
    )
    .unwrap();
    let out = temp.path().join("results.txt");

    ob().arg("scan")
        .arg("-p")
        .arg(&patterns)
        .arg("--hidden")
        .arg("--no-ignore")
        .arg("-o")
        .arg(&out)
        .arg(&scanme)
        .assert()
        .success();
    let results = fs::read_to_string(&out).unwrap();
    assert!(results.contains("TODO app"), "got: {results}");
    assert!(
        !results.contains("vendor"),
        "vendor/ must be skipped: {results}"
    );

    // A file named as an input is scanned even inside an excluded directory.
    ob().arg("scan")
        .arg("-p")
        .arg(&patterns)
        .arg("-o")
        .arg(&out)
        .arg(scanme.join("vendor").join("dep.txt"))
        .assert()
        .success();
    let results = fs::read_to_string(&out).unwrap();
    assert!(results.contains("TODO dep"), "got: {results}");
}

#[test]
fn scan_config_with_an_unknown_key_fails_naming_the_key() {
    // A typo in a key used to be ignored, so the setting silently did
    // nothing. The scan must fail and name the key.
    let temp = TempDir::new().unwrap();
    let scanme = temp.path().join("scanme");
    fs::create_dir(&scanme).unwrap();
    fs::write(scanme.join("a.txt"), "TODO\n").unwrap();
    let patterns = temp.path().join("patterns.yaml");
    fs::write(
        &patterns,
        "patterns:\n  - name: todo\n    pattern: 'TODO'\nexclusion:\n  - vendor\n",
    )
    .unwrap();

    ob().arg("scan")
        .arg("-p")
        .arg(&patterns)
        .arg(&scanme)
        .assert()
        .failure()
        .stderr(contains("unknown field `exclusion`"));
}

#[test]
fn replace_config_with_an_unknown_key_fails_naming_the_key_and_changes_nothing() {
    // `replacement:` (singular) used to be ignored, leaving `replacements`
    // empty. The run must fail before any file is touched.
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();
    fs::write(work.join("a.txt"), "TODO\n").unwrap();
    let config = temp.path().join("config.yaml");
    fs::write(&config, "patterns: ['TODO']\nreplacement: ['DONE']\n").unwrap();

    ob().arg("replace")
        .arg(&work)
        .arg("-c")
        .arg(&config)
        .assert()
        .failure()
        .stderr(contains("unknown field `replacement`"));
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), "TODO\n");
    assert!(!work.join("a.txt.bak").exists());
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

/// Runs `ob replace <dir> -p alpha -r beta` and asserts success.
fn replace_alpha_with_beta(dir: &Path) {
    ob().arg("replace")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .assert()
        .success();
}

#[test]
fn undo_and_clean_backups_leave_bak_files_oober_did_not_create() {
    // Verified failure: undo treated every *.bak in the tree as its own. A
    // user's settings.json.bak was copied over the live settings.json and
    // deleted, an orphan db/prod.sql.bak was "restored" into a new
    // db/prod.sql, and a gitignored node_modules/z.js.bak was restored too;
    // clean-backups deleted all three.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();

    // Best effort: without git the .gitignore is inert and the assertions
    // below still hold.
    let _ = std::process::Command::new("git")
        .arg("init")
        .current_dir(dir)
        .output();
    fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();

    // None of the user's live files contains the pattern.
    fs::write(dir.join("settings.json"), "{\"theme\": \"dark\"}\n").unwrap();
    fs::write(dir.join("settings.json.bak"), "{\"theme\": \"light\"}\n").unwrap();
    fs::create_dir(dir.join("db")).unwrap();
    fs::write(dir.join("db").join("prod.sql.bak"), "-- dump\n").unwrap();
    fs::create_dir(dir.join("node_modules")).unwrap();
    fs::write(dir.join("node_modules").join("z.js"), "live()\n").unwrap();
    fs::write(dir.join("node_modules").join("z.js.bak"), "old()\n").unwrap();
    let file = dir.join("a.txt");
    fs::write(&file, "alpha one\n").unwrap();

    let assert_user_files_untouched = |step: &str| {
        let read = |rel: &str| fs::read_to_string(dir.join(rel)).unwrap();
        assert_eq!(
            read("settings.json"),
            "{\"theme\": \"dark\"}\n",
            "after {step}"
        );
        assert_eq!(
            read("settings.json.bak"),
            "{\"theme\": \"light\"}\n",
            "after {step}"
        );
        assert_eq!(read("db/prod.sql.bak"), "-- dump\n", "after {step}");
        assert!(
            !dir.join("db").join("prod.sql").exists(),
            "after {step}: an orphan .bak must not be restored into a new file"
        );
        assert_eq!(read("node_modules/z.js"), "live()\n", "after {step}");
        assert_eq!(read("node_modules/z.js.bak"), "old()\n", "after {step}");
    };

    replace_alpha_with_beta(dir);
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");
    assert_user_files_untouched("replace");

    // undo restores oober's own backup only, and says what it left alone.
    ob().arg("undo")
        .arg("-d")
        .arg(dir)
        .assert()
        .success()
        .stdout(contains("--all-bak"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
    assert!(!dir.join("a.txt.bak").exists());
    assert_user_files_untouched("undo");

    // clean-backups removes oober's own backup only.
    replace_alpha_with_beta(dir);
    assert!(dir.join("a.txt.bak").exists());
    ob().arg("clean-backups")
        .arg("-d")
        .arg(dir)
        .assert()
        .success()
        .stdout(contains("--all-bak"));
    assert!(!dir.join("a.txt.bak").exists());
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");
    assert_user_files_untouched("clean-backups");
}

#[test]
fn undo_restores_the_backup_of_a_hidden_file_named_as_input() {
    // The backup walk skips hidden paths; the backup record still finds it.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join(".env");
    fs::write(&file, "alpha one\n").unwrap();

    replace_alpha_with_beta(&file);
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");

    ob().arg("undo").arg("-d").arg(dir).assert().success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
    assert!(!dir.join(".env.bak").exists());
}

#[test]
fn undo_and_clean_backups_all_bak_act_on_unrecorded_backups() {
    // Backups made by 0.1.1, or whose record was lost with the cache dir.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("a.txt"), "modified\n").unwrap();
    fs::write(dir.join("a.txt.bak"), "original\n").unwrap();
    fs::write(dir.join("b.txt.bak"), "stale\n").unwrap();

    // The preview changes nothing.
    ob().arg("undo")
        .arg("-d")
        .arg(dir)
        .arg("--all-bak")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(contains("Would restore"));
    assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "modified\n");
    assert!(!dir.join("b.txt").exists());

    ob().arg("undo")
        .arg("-d")
        .arg(dir)
        .arg("--all-bak")
        .arg("--keep-backups")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "original\n");
    assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "stale\n");

    ob().arg("clean-backups")
        .arg("-d")
        .arg(dir)
        .arg("--all-bak")
        .assert()
        .success();
    assert!(!dir.join("a.txt.bak").exists());
    assert!(!dir.join("b.txt.bak").exists());
}

#[test]
fn undo_dry_run_changes_nothing() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("a.txt");
    fs::write(&file, "alpha one\n").unwrap();
    replace_alpha_with_beta(dir);

    ob().arg("undo")
        .arg("-d")
        .arg(dir)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(contains("Would restore"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");
    assert_eq!(
        fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
        "alpha one\n"
    );

    // The record survived the preview: a real undo still restores.
    ob().arg("undo").arg("-d").arg(dir).assert().success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
}

#[test]
fn undo_continues_past_a_failed_restore_and_exits_nonzero() {
    // Verified failure: the first restore that failed aborted the run, so
    // the files after it in walk order were never restored.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let names = ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"];
    for name in names {
        fs::write(dir.join(name), "alpha one\n").unwrap();
    }
    replace_alpha_with_beta(dir);

    // Three files can no longer be restored: a directory sits at each path.
    // Whichever of them the walk reaches first, the other files must still
    // be restored.
    let blocked = ["a.txt", "c.txt", "e.txt"];
    for name in blocked {
        fs::remove_file(dir.join(name)).unwrap();
        fs::create_dir(dir.join(name)).unwrap();
    }

    ob().arg("undo").arg("-d").arg(dir).assert().failure();

    for name in names {
        if blocked.contains(&name) {
            assert!(dir.join(name).is_dir(), "the directory must be left alone");
            assert_eq!(
                fs::read_to_string(dir.join(format!("{name}.bak"))).unwrap(),
                "alpha one\n",
                "the backup of a failed restore must be kept"
            );
        } else {
            assert_eq!(
                fs::read_to_string(dir.join(name)).unwrap(),
                "alpha one\n",
                "{name} must be restored despite the failures"
            );
        }
    }
}

#[test]
fn replace_refuses_a_file_edited_since_its_backup_was_made() {
    // Verified failure: with a.txt.bak already there, a second replace
    // modified a.txt with no backup of its current state and no warning, so
    // a later undo rolled back to before the first run and wiped every edit
    // made in between.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("a.txt");
    fs::write(&file, "alpha one\n").unwrap();
    replace_alpha_with_beta(dir);
    assert_eq!(fs::read_to_string(&file).unwrap(), "beta one\n");

    // A hand edit; the added line changes the size, so the check does not
    // hang on the clock's resolution.
    let edited = "beta one\nalpha by hand\n";
    fs::write(&file, edited).unwrap();

    let second_run = |dry_run: bool| {
        let mut cmd = ob();
        cmd.arg("replace").arg(dir).arg("-p").arg("alpha");
        cmd.arg("-r").arg("beta");
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.assert().failure().stderr(contains("a.txt.bak"));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            edited,
            "the hand edit must be intact (dry_run: {dry_run})"
        );
        assert_eq!(
            fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
            "alpha one\n"
        );
    };
    // The dry run reports the refusal the real run makes.
    second_run(true);
    second_run(false);

    // --no-backup is the stated way through.
    ob().arg("replace")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--no-backup")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "beta one\nbeta by hand\n"
    );
}

#[test]
fn replace_twice_without_an_edit_between_keeps_the_oldest_backup() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let file = dir.join("a.txt");
    fs::write(&file, "alpha one\n").unwrap();

    replace_alpha_with_beta(dir);
    ob().arg("replace")
        .arg(dir)
        .arg("-p")
        .arg("beta")
        .arg("-r")
        .arg("gamma")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "gamma one\n");
    assert_eq!(
        fs::read_to_string(dir.join("a.txt.bak")).unwrap(),
        "alpha one\n"
    );

    ob().arg("undo").arg("-d").arg(dir).assert().success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha one\n");
}

#[test]
fn replace_refuses_a_file_whose_bak_path_is_taken() {
    // Verified failure: a user's own .bak (or a directory at that path) made
    // replace modify the file with no backup at all, and a dangling symlink
    // there had the backup written through it to wherever it pointed.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let outside = TempDir::new().unwrap();
    let link_target = outside.path().join("elsewhere.txt");

    fs::write(dir.join("user.txt"), "alpha user\n").unwrap();
    fs::write(dir.join("user.txt.bak"), "my own copy\n").unwrap();
    fs::write(dir.join("dir.txt"), "alpha dir\n").unwrap();
    fs::create_dir(dir.join("dir.txt.bak")).unwrap();
    #[cfg(unix)]
    {
        fs::write(dir.join("link.txt"), "alpha link\n").unwrap();
        std::os::unix::fs::symlink(&link_target, dir.join("link.txt.bak")).unwrap();
    }
    fs::write(dir.join("free.txt"), "alpha free\n").unwrap();

    ob().arg("replace")
        .arg(dir)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .assert()
        .failure()
        .stderr(contains("user.txt.bak"))
        .stderr(contains("dir.txt.bak"));

    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(read("user.txt"), "alpha user\n");
    assert_eq!(read("user.txt.bak"), "my own copy\n");
    assert_eq!(read("dir.txt"), "alpha dir\n");
    assert!(dir.join("dir.txt.bak").is_dir());
    assert_eq!(fs::read_dir(dir.join("dir.txt.bak")).unwrap().count(), 0);
    #[cfg(unix)]
    {
        assert_eq!(read("link.txt"), "alpha link\n");
        assert!(
            fs::symlink_metadata(dir.join("link.txt.bak"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert!(
        !link_target.exists(),
        "nothing may be created through a symlink at the .bak path"
    );

    // A file whose .bak path is free is still processed and backed up.
    assert_eq!(read("free.txt"), "beta free\n");
    assert_eq!(read("free.txt.bak"), "alpha free\n");
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
fn replace_accepts_positional_directory_inputs() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    let lib = temp.path().join("lib");
    let other = temp.path().join("other");
    for d in [&src, &lib, &other] {
        fs::create_dir(d).unwrap();
        fs::write(d.join("a.txt"), "alpha one\n").unwrap();
    }

    ob().arg("replace")
        .arg(&src)
        .arg(&lib)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--no-backup")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "beta one\n");
    assert_eq!(fs::read_to_string(lib.join("a.txt")).unwrap(), "beta one\n");
    assert_eq!(
        fs::read_to_string(other.join("a.txt")).unwrap(),
        "alpha one\n",
        "a directory not named as an input must be untouched"
    );
}

#[cfg(unix)]
#[test]
fn replace_does_not_descend_into_excluded_directories() {
    use std::os::unix::fs::PermissionsExt;

    // An excluded directory must be pruned, not walked and then filtered:
    // reading an unreadable one would print a walk warning.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    let excluded = dir.join("build");
    fs::create_dir(&excluded).unwrap();
    fs::set_permissions(&excluded, fs::Permissions::from_mode(0o000)).unwrap();

    let output = ob()
        .arg("replace")
        .arg(dir)
        .args(["-p", "alpha", "-r", "beta", "-e", "build", "--no-backup"])
        .output()
        .unwrap();

    // Restore permissions before asserting so TempDir cleanup succeeds.
    fs::set_permissions(&excluded, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("skipping unreadable path"),
        "the excluded directory must not be read: {stderr}"
    );
    assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "beta\n");
}

#[test]
fn replace_accepts_a_single_file_input() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("target.txt"), "alpha one\n").unwrap();
    fs::write(dir.join("sibling.txt"), "alpha two\n").unwrap();

    ob().arg("replace")
        .arg(dir.join("target.txt"))
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--no-backup")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(dir.join("target.txt")).unwrap(),
        "beta one\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("sibling.txt")).unwrap(),
        "alpha two\n",
        "a sibling of an explicit file input must be untouched"
    );
}

#[test]
fn replace_mixed_file_and_dir_inputs_process_each_file_once() {
    // The same file named explicitly and reached via a directory walk must be
    // transformed once: X -> XX applied twice would leave XXXX behind.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("a.txt"), "X\n").unwrap();

    ob().arg("replace")
        .arg(dir.join("a.txt"))
        .arg(dir)
        .arg("-p")
        .arg("X")
        .arg("-r")
        .arg("XX")
        .arg("--no-backup")
        .arg("-w")
        .arg("1")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "XX\n");
}

#[test]
fn replace_legacy_dir_flag_appends_to_positional_inputs() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    let lib = temp.path().join("lib");
    for d in [&src, &lib] {
        fs::create_dir(d).unwrap();
        fs::write(d.join("a.txt"), "alpha one\n").unwrap();
    }

    ob().arg("replace")
        .arg(&src)
        .arg("-d")
        .arg(&lib)
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .arg("--no-backup")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "beta one\n");
    assert_eq!(
        fs::read_to_string(lib.join("a.txt")).unwrap(),
        "beta one\n",
        "the legacy -d directory must be processed alongside positional inputs"
    );
}

#[test]
fn replace_pattern_without_replacement_is_a_usage_error_and_touches_nothing() {
    // `-p` with no `-r` used to become a null replacement, which deletes the
    // line after each match.
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("a.txt");
    fs::write(&file, "alpha\nkeep me\n").unwrap();

    // clap usage errors exit with code 2.
    ob().arg("replace")
        .arg(temp.path())
        .arg("-p")
        .arg("alpha")
        .assert()
        .code(2)
        .stderr(contains("--replacement"));
    ob().arg("replace")
        .arg(temp.path())
        .arg("-r")
        .arg("beta")
        .assert()
        .code(2)
        .stderr(contains("--pattern"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha\nkeep me\n");
    assert!(!temp.path().join("a.txt.bak").exists());
}

#[test]
fn replace_with_undefined_group_reference_errors_and_touches_nothing() {
    // `$5` is a reference to group 5, which the pattern does not define; it
    // used to expand to nothing and erase the matched text.
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("a.txt");
    fs::write(&file, "cost PRICE now\n").unwrap();

    ob().arg("replace")
        .arg(temp.path())
        .arg("-p")
        .arg("PRICE")
        .arg("-r")
        .arg("$5")
        .assert()
        .failure()
        .stderr(contains("${1}"))
        .stderr(contains("$$"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "cost PRICE now\n");
    assert!(!temp.path().join("a.txt.bak").exists());

    // The documented escape writes a literal dollar sign.
    ob().arg("replace")
        .arg(temp.path())
        .arg("-p")
        .arg("PRICE")
        .arg("-r")
        .arg("$$5")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&file).unwrap(), "cost $5 now\n");
}

#[test]
fn replace_nonexistent_input_errors_like_scan() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("a.txt"), "alpha one\n").unwrap();

    ob().arg("replace")
        .arg(dir)
        .arg(dir.join("missing"))
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .assert()
        .failure()
        .stderr(contains("Input path not found"))
        .stderr(contains("missing"));

    assert_eq!(
        fs::read_to_string(dir.join("a.txt")).unwrap(),
        "alpha one\n",
        "no file may be modified when an input path is invalid"
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
fn replace_leaves_its_own_config_file_untouched() {
    // `ob replace . -c cfg.yaml` with the config inside the processed tree
    // and no extension filter: rewriting the config would turn its pattern
    // into `[TODO]`, a character class, for the next run.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    let config = "patterns:\n  - \"TODO\"\nreplacements:\n  - \"[TODO]\"\n";
    fs::write(dir.join("cfg.yaml"), config).unwrap();
    fs::write(dir.join("notes.txt"), "TODO item\n").unwrap();

    ob().current_dir(dir)
        .args(["replace", ".", "-c", "cfg.yaml", "--no-backup"])
        .assert()
        .success();

    assert_eq!(
        fs::read(dir.join("cfg.yaml")).unwrap(),
        config.as_bytes(),
        "replace must not rewrite the config file it was given"
    );
    assert_eq!(
        fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "[TODO] item\n",
        "other files in the tree must still be processed"
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
fn replace_without_inputs_or_list_presets_is_a_helpful_error() {
    ob().arg("replace")
        .arg("--preset")
        .arg("trim-whitespace")
        .assert()
        .failure()
        .stderr(contains("input"))
        .stderr(contains("--dir"))
        .stderr(contains("--list-presets"));

    // The bare-`ob replace` banner names the positional grammar, not --dir.
    ob().arg("replace")
        .assert()
        .failure()
        .stderr(contains("<INPUTS>"));
}

#[test]
fn replace_preset_with_pattern_is_a_usage_error_and_touches_nothing() {
    // --preset used to win silently, dropping the -p/-r the user asked for.
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("a.txt");
    fs::write(&file, "alpha   \n").unwrap();

    // clap usage errors exit with code 2.
    ob().arg("replace")
        .arg(temp.path())
        .arg("--preset")
        .arg("trim-whitespace")
        .arg("-p")
        .arg("alpha")
        .arg("-r")
        .arg("beta")
        .assert()
        .code(2);

    assert_eq!(fs::read_to_string(&file).unwrap(), "alpha   \n");
    assert!(!temp.path().join("a.txt.bak").exists());
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

#[test]
fn rename_with_undefined_group_reference_errors_and_renames_nothing() {
    // `$2_` is a reference to a group named `2_`, which the pattern does
    // not define; it used to expand to nothing and rename the file to `01`.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("01_intro.md"), "intro").unwrap();

    ob().arg("rename")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg(r"(\d+)_(.*)")
        .arg("-r")
        .arg("$2_$1")
        .assert()
        .failure()
        .stderr(contains("${1}"))
        .stderr(contains("$$"));

    let names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["01_intro.md"]);
}

#[test]
fn rename_reorder_example_from_help_swaps_name_parts() {
    // The example shipped in `ob rename --help` and the usage banner.
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    fs::write(dir.join("12_intro.md"), "intro").unwrap();

    ob().arg("rename")
        .arg("-d")
        .arg(dir)
        .arg("-p")
        .arg(r"^(\d+)_(.+?)(\.[^.]+)?$")
        .arg("-r")
        .arg("${2}_${1}${3}")
        .assert()
        .success();

    assert!(!dir.join("12_intro.md").exists());
    assert_eq!(
        fs::read_to_string(dir.join("intro_12.md")).unwrap(),
        "intro"
    );
}

// ---------------------------------------------------------------------------
// completions & manpage
// ---------------------------------------------------------------------------

#[test]
fn completions_zsh_emits_a_compdef_script_for_oober() {
    let assert = ob().arg("completions").arg("zsh").assert().success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(!stdout.is_empty(), "completion script must not be empty");
    assert!(
        stdout.contains("#compdef") && stdout.contains("_oober"),
        "zsh script must be a compdef for the oober binary: {}",
        &stdout[..stdout.len().min(200)]
    );
}

#[test]
fn completions_bash_emits_a_complete_registration() {
    ob().arg("completions")
        .arg("bash")
        .assert()
        .success()
        .stdout(contains("complete"));
}

#[test]
fn manpage_emits_a_roff_title_header() {
    // `ob manpage` takes no arguments, so it also proves main.rs's
    // missing-arg banner (which intercepts two-element argv by command name)
    // does not swallow argument-less subcommands.
    let assert = ob().arg("manpage").assert().success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    // roff output opens with a two-line apostrophe-handling preamble; the
    // title header must follow as its own control line.
    assert!(
        stdout.lines().any(|l| l.starts_with(".TH oober")),
        "man page must carry a .TH title line: {}",
        &stdout[..stdout.len().min(200)]
    );
}

#[test]
fn completions_unknown_shell_is_rejected_at_parse_time() {
    ob().arg("completions")
        .arg("tcsh")
        .assert()
        .code(2)
        .stderr(contains("invalid value 'tcsh'"));
}
