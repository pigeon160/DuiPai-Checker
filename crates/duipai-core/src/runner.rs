//! 程序运行与输出比较（移植 legacy/duipai.py 的进程管理部分）。
//!
//! 纯 std 实现：命令解析（兼容 Windows 引号）、子进程执行（stdin/stdout/stderr、
//! 超时、杀进程、Windows 无控制台窗口）、C++ 源码编译、输出比较。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{DslError, DslResult};

/// 切分命令字符串为参数列表，兼容 Windows 路径与引号（legacy `_parse_command`）。
pub fn parse_command(cmd: &str) -> Vec<String> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Vec::new();
    }
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c.is_whitespace() {
                    if !cur.is_empty() {
                        args.push(std::mem::take(&mut cur));
                    }
                    // 跳过连续空白
                    while chars.peek().map_or(false, |c| c.is_whitespace()) {
                        chars.next();
                    }
                } else {
                    cur.push(c);
                }
            }
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

/// 把带路径/相对路径的程序名解析为绝对路径；纯命令名走 PATH。
fn resolve_program_path(args: &[String], cwd: &str) -> Vec<String> {
    let Some(prog) = args.first() else {
        return args.to_vec();
    };
    if Path::new(prog).is_absolute() {
        return args.to_vec();
    }
    if prog.contains(['/', '\\']) || prog.starts_with('.') {
        let joined = Path::new(cwd).join(prog);
        let mut out = vec![joined.to_string_lossy().into_owned()];
        out.extend(args[1..].iter().cloned());
        return out;
    }
    args.to_vec()
}

/// Windows 下禁止子进程创建控制台窗口（防止终端闪现）。

/// 运行结果状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunStatus {
    Ok,
    /// 超时（子进程已被终止）
    Timeout,
    /// 启动失败（找不到程序等）
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub status: RunStatus,
    pub returncode: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// 启动失败原因
    pub error: String,
    /// 实际运行耗时（秒）
    pub elapsed: f64,
}

impl RunResult {
    pub fn ok(stdout: Vec<u8>, stderr: Vec<u8>, returncode: i32, elapsed: f64) -> Self {
        Self {
            status: RunStatus::Ok,
            returncode: Some(returncode),
            stdout,
            stderr,
            error: String::new(),
            elapsed,
        }
    }
}

fn spawn(cmd_str: &str, base_dir: &str) -> std::io::Result<std::process::Child> {
    let args = parse_command(cmd_str);
    let cwd = if base_dir.is_empty() || !Path::new(base_dir).is_dir() {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string())
    } else {
        base_dir.to_string()
    };
    let args = resolve_program_path(&args, &cwd);
    let mut cmd = Command::new(&args[0]);
    cmd.args(&args[1..])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
}

/// 运行一个程序，输入 bytes，返回结果。超时杀进程。
pub fn run_program(cmd_str: &str, base_dir: &str, input: &[u8], timeout: f64) -> RunResult {
    if parse_command(cmd_str).is_empty() {
        return RunResult {
            status: RunStatus::Error,
            returncode: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error: "命令为空".to_string(),
            elapsed: 0.0,
        };
    }
    let start = std::time::Instant::now();
    let mut child = match spawn(cmd_str, base_dir) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let args = parse_command(cmd_str);
            return RunResult {
                status: RunStatus::Error,
                returncode: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error: format!("找不到程序或解释器：{}", args[0]),
                elapsed: 0.0,
            };
        }
        Err(e) => {
            return RunResult {
                status: RunStatus::Error,
                returncode: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                error: e.to_string(),
                elapsed: 0.0,
            };
        }
    };
    // 写入 stdin 并读取输出
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let input_owned = input.to_vec();
    let reader = std::thread::spawn(move || {
        if let Some(mut s) = stdin {
            let _ = s.write_all(&input_owned);
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        if let Some(mut so) = stdout {
            let _ = std::io::Read::read_to_end(&mut so, &mut out);
        }
        if let Some(mut se) = stderr {
            let _ = std::io::Read::read_to_end(&mut se, &mut err);
        }
        (out, err)
    });

    // 等待子进程（带超时轮询）
    let timeout_dur = Duration::from_secs_f64(timeout);
    let deadline = start + timeout_dur;
    let timed_out = loop {
        match child.try_wait() {
            Ok(Some(_status)) => break false,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    break true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break false,
        }
    };

    if timed_out {
        let _ = child.kill();
        let _ = child.wait();
        let elapsed = start.elapsed().as_secs_f64();
        return RunResult {
            status: RunStatus::Timeout,
            returncode: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            error: String::new(),
            elapsed,
        };
    }

    let status = child.wait().unwrap_or_default();
    let (out, err) = reader.join().unwrap_or((Vec::new(), Vec::new()));
    RunResult {
        status: RunStatus::Ok,
        returncode: status.code(),
        stdout: out,
        stderr: err,
        error: String::new(),
        elapsed: start.elapsed().as_secs_f64(),
    }
}

/// 编译 C++ 源码，返回可执行文件路径。超时 60 秒，失败含错误摘要（前 40 行）。
pub fn compile_cpp(
    source: &str,
    workdir: &str,
    name: &str,
    compiler: &str,
    flags: &str,
) -> DslResult<String> {
    if !Path::new(source).is_file() {
        return Err(DslError::bare(format!("找不到源码文件：{source}")));
    }
    let exe = Path::new(workdir).join(format!(
        "{}{}",
        name,
        if cfg!(target_os = "windows") { ".exe" } else { "" }
    ));
    let exe_str = exe.to_string_lossy().into_owned();
    let cwd = Path::new(source)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    let mut args: Vec<String> = vec![compiler.to_string()];
    args.extend(parse_command(flags));
    args.push(source.to_string());
    args.push("-o".to_string());
    args.push(exe_str.clone());

    let start = std::time::Instant::now();
    let output = match Command::new(compiler).args(&args[1..]).current_dir(&cwd).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(DslError::bare(format!("找不到编译器：{compiler}")));
        }
        Err(e) => return Err(DslError::bare(e.to_string())),
    };
    if start.elapsed() > Duration::from_secs(60) {
        return Err(DslError::bare("编译超时（>60s）"));
    }
    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = msg.lines().filter(|l| !l.trim().is_empty()).collect();
        return Err(DslError::bare(format!(
            "编译失败，返回码 {}：\n{}",
            output.status.code().unwrap_or(-1),
            lines.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
        )));
    }
    Ok(exe_str)
}

/// 比较两份输出；`ignore_ws` 时忽略行末空格与末尾空行。
pub fn compare(out1: &str, out2: &str, ignore_ws: bool) -> bool {
    if ignore_ws {
        normalize(out1) == normalize(out2)
    } else {
        out1 == out2
    }
}

/// 每行 rstrip，并去除末尾连续空行。
pub fn normalize(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    while lines.last().map_or(false, |l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}
