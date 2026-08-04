//! 进程运行与比较测试。

use duipai_core::{compare, normalize, parse_command, run_program, RunStatus};

#[test]
fn parse_command_basics() {
    assert_eq!(parse_command(""), Vec::<String>::new());
    assert_eq!(parse_command("  "), Vec::<String>::new());
    assert_eq!(
        parse_command("python3 ./sol.py --seed 42"),
        vec!["python3", "./sol.py", "--seed", "42"]
    );
    // Windows 风格引号（含空格路径）
    assert_eq!(
        parse_command(r#""C:\Program Files\node\node.exe" -e "console.log(1)""#),
        vec!["C:\\Program Files\\node\\node.exe", "-e", "console.log(1)"]
    );
    // 连续空白
    assert_eq!(
        parse_command("a   b\tc"),
        vec!["a", "b", "c"]
    );
}

#[test]
fn run_cmd_echo() {
    let r = run_program("cmd /C echo hello", "", b"", 10.0);
    assert_eq!(r.status, RunStatus::Ok);
    let out = String::from_utf8_lossy(&r.stdout);
    assert!(out.contains("hello"), "{out}");
    assert!(r.elapsed >= 0.0);
}

#[test]
fn run_with_stdin() {
    // powershell 从 stdin 读一行并打印
    let r = run_program("powershell -NoProfile -Command \"$line = [Console]::In.ReadLine(); Write-Output $line\"", "", b"42\n", 10.0);
    assert_eq!(r.status, RunStatus::Ok);
    let out = String::from_utf8_lossy(&r.stdout);
    assert!(out.contains("42"), "{out}");
}

#[test]
fn run_not_found() {
    let r = run_program("this_exe_should_not_exist_xyz_123", "", b"", 5.0);
    assert_eq!(r.status, RunStatus::Error);
    assert!(r.error.contains("找不到"), "{r:?}");
}

#[test]
fn run_timeout_kills() {
    let start = std::time::Instant::now();
    let r = run_program("cmd /C ping -n 20 127.0.0.1 >nul", "", b"", 0.5);
    assert_eq!(r.status, RunStatus::Timeout);
    assert!(start.elapsed().as_secs_f64() < 10.0, "超时应快速返回");
}

#[test]
fn run_nonzero_returncode() {
    let r = run_program("cmd /C exit /b 3", "", b"", 5.0);
    assert_eq!(r.status, RunStatus::Ok);
    assert_eq!(r.returncode, Some(3));
}

#[test]
fn compare_modes() {
    assert!(compare("a b\n", "a b\n", false));
    assert!(!compare("a b\n", "a  b\n", false));
    assert!(compare("a b  \n\n", "a b\n", true));
    assert!(!compare("ab", "a b", true));
    assert!(compare("", "\n\n\n", true));
    assert_eq!(
        normalize("a  \n b\n\n\n"),
        vec!["a".to_string(), " b".to_string()]
    );
}

#[test]
fn stderr_captured() {
    let r = run_program("cmd /C echo err 1>&2", "", b"", 5.0);
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("err"), "{err}");
}
