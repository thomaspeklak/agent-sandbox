use ags::paths::{PathExpandError, expand_path};

#[test]
fn expands_tilde_to_home() {
    let home = std::env::var("HOME").unwrap();
    let result = expand_path("~/foo/bar").unwrap();
    assert_eq!(result.to_str().unwrap(), format!("{home}/foo/bar"));
}

#[test]
fn expands_env_var_dollar_name() {
    // SAFETY: test runs single-threaded; no other thread reads this var.
    unsafe { std::env::set_var("AGS_TEST_VAR", "/custom/path") };
    let result = expand_path("$AGS_TEST_VAR/sub").unwrap();
    assert_eq!(result.to_str().unwrap(), "/custom/path/sub");
    unsafe { std::env::remove_var("AGS_TEST_VAR") };
}

#[test]
fn expands_env_var_braced() {
    // SAFETY: test runs single-threaded; no other thread reads this var.
    unsafe { std::env::set_var("AGS_TEST_BRACED", "/braced") };
    let result = expand_path("${AGS_TEST_BRACED}/end").unwrap();
    assert_eq!(result.to_str().unwrap(), "/braced/end");
    unsafe { std::env::remove_var("AGS_TEST_BRACED") };
}

#[test]
fn returns_error_for_missing_env_var() {
    let result = expand_path("$NONEXISTENT_AGS_VAR_XYZ/foo");
    assert_eq!(
        result.unwrap_err(),
        PathExpandError::EnvVar("NONEXISTENT_AGS_VAR_XYZ".to_owned())
    );
}

#[test]
fn absolute_path_unchanged() {
    let result = expand_path("/absolute/path").unwrap();
    assert_eq!(result.to_str().unwrap(), "/absolute/path");
}
