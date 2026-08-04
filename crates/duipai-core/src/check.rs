//! 对拍主循环（移植 legacy/duipai.py 的 _run_loop）。
//!
//! 纯逻辑实现：一次编译 → 逐组（生成 → 跑正解 → 跑暴力 → 比较）→ 统计 →
//! WA/TLE/RE 现场保存到 `./fail/`。进度通过事件回调上抛，取消走 AtomicBool。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ast::Config;
use crate::error::DslResult;
use crate::generator::generate_with;
use crate::runner::{compile_cpp, compare, parse_command, run_argv_ex, run_program_ex, RunStatus};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// 程序模式：直接运行命令 / C++ 源码（先编译）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgMode {
    RunCmd,
    CppSource,
}

/// 数据来源：内置生成器（DSL）/ 外置生成器（程序 stdout 即测试数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenMode {
    Builtin,
    External,
}

/// 单个程序配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSpec {
    pub mode: ProgMode,
    pub cmd: String,
    pub dir: String,
    pub label: String,
}

/// 对拍参数（一次性快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckParams {
    pub sol: ProgramSpec,
    pub brute: ProgramSpec,
    /// 数据来源
    pub gen_mode: GenMode,
    /// 外置生成器（gen_mode == External 时使用）
    pub ext: Option<ProgramSpec>,
    /// 组数；-1 表示无限。
    pub total: i64,
    /// 单程序超时（秒）。
    pub timeout: f64,
    /// 单程序内存限制（MB）；None 表示不限。
    pub memory_limit_mb: Option<u64>,
    pub seed: Option<u64>,
    pub ignore_ws: bool,
    pub compiler: String,
    pub compile_flags: String,
    pub config: Config,
}

/// 统计结果。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckStats {
    pub pass: u64,
    pub wa: u64,
    pub tle: u64,
    pub re: u64,
    /// 内存超限
    pub mle: u64,
    pub error: u64,
    pub tested: u64,
}

/// 进度事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CheckEvent {
    Log { msg: String },
    Status { tested: u64, total: i64 },
    Finish {
        stats: CheckStats,
        tested: u64,
        reason: String,
        fail_dir: Option<String>,
    },
}

struct Work {
    tmp: std::path::PathBuf,
}

impl Work {
    fn new() -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "duipai_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self { tmp: dir })
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.tmp.join(name)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

/// 编译（若为 C++ 源码模式），返回可运行的命令字符串与目录。
fn prepare(
    spec: &ProgramSpec,
    workdir: &str,
    name: &str,
    compiler: &str,
    flags: &str,
) -> DslResult<(String, String)> {
    match spec.mode {
        ProgMode::RunCmd => Ok((spec.cmd.clone(), spec.dir.clone())),
        ProgMode::CppSource => {
            let exe = compile_cpp(&spec.cmd, workdir, name, compiler, flags)?;
            Ok((exe, workdir.to_string()))
        }
    }
}

/// 下一个失败编号（./fail/fail_<N>.in 递增）。
fn next_fail_index(fail_dir: &str) -> u64 {
    let mut max = 0u64;
    if let Ok(entries) = std::fs::read_dir(fail_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("fail_") {
                if let Some(num) = rest.strip_suffix(".in") {
                    if let Ok(n) = num.parse::<u64>() {
                        max = max.max(n);
                    }
                }
            }
        }
    }
    max + 1
}

/// 保存失败现场（test.in / prog.out / std.out）到 ./fail/。
fn save_fail(work: &Work, emit: &mut dyn FnMut(CheckEvent)) -> Option<String> {
    let fail_dir = std::env::current_dir()
        .ok()?
        .join("fail")
        .to_string_lossy()
        .into_owned();
    if std::fs::create_dir_all(&fail_dir).is_err() {
        emit(CheckEvent::Log { msg: "    无法创建 fail 目录".to_string() });
        return None;
    }
    let idx = next_fail_index(&fail_dir);
    let mappings = [
        ("test.in", format!("fail_{idx}.in")),
        ("prog.out", format!("fail_{idx}_prog.out")),
        ("std.out", format!("fail_{idx}_std.out")),
    ];
    let mut saved: Vec<String> = Vec::new();
    for (src, dst) in mappings {
        let sp = work.path(src);
        if sp.is_file() {
            if std::fs::copy(&sp, Path::new(&fail_dir).join(&dst)).is_ok() {
                saved.push(dst);
            }
        }
    }
    let msg = format!("    现场已保存到 {}{}（{}）", fail_dir, std::path::MAIN_SEPARATOR, saved.join(", "));
    emit(CheckEvent::Log { msg });
    Some(fail_dir)
}

/// 运行对拍循环。阻塞直至结束；`emit` 回调同步抛事件（在调用线程）。
pub fn run_check(
    params: &CheckParams,
    cancel: Arc<AtomicBool>,
    emit: &mut dyn FnMut(CheckEvent),
) {
    let mut stats = CheckStats::default();
    let mut reason = String::new();
    let mut fail_dir: Option<String> = None;
    let total = params.total;
    let timeout = params.timeout;

    let work = match Work::new() {
        Ok(w) => w,
        Err(e) => {
            emit(CheckEvent::Log { msg: format!("创建临时目录失败：{e}") });
            stats.error += 1;
            reason = "因出错中止".to_string();
            emit(CheckEvent::Finish { stats, tested: 0, reason, fail_dir });
            return;
        }
    };

    let sol = prepare(&params.sol, work.tmp.to_str().unwrap_or("."), "sol", &params.compiler, &params.compile_flags);
    let brute = prepare(&params.brute, work.tmp.to_str().unwrap_or("."), "brute", &params.compiler, &params.compile_flags);
    let (sol_run, sol_dir) = match sol {
        Ok(x) => x,
        Err(e) => {
            emit(CheckEvent::Log { msg: format!("[编译] 正解 失败：{e}") });
            stats.error += 1;
            reason = "编译失败".to_string();
            emit(CheckEvent::Finish { stats, tested: 0, reason, fail_dir });
            return;
        }
    };
    let (brute_run, brute_dir) = match brute {
        Ok(x) => x,
        Err(e) => {
            emit(CheckEvent::Log { msg: format!("[编译] 暴力 失败：{e}") });
            stats.error += 1;
            reason = "编译失败".to_string();
            emit(CheckEvent::Finish { stats, tested: 0, reason, fail_dir });
            return;
        }
    };
    // 外置生成器：同样先编译（C++ 源码模式）
    let ext_ready = if params.gen_mode == GenMode::External {
        let ext = params.ext.as_ref().expect("External 模式必须有 ext 配置");
        match prepare(ext, work.tmp.to_str().unwrap_or("."), "gen", &params.compiler, &params.compile_flags) {
            Ok(x) => Some(x),
            Err(e) => {
                emit(CheckEvent::Log { msg: format!("[编译] 外置生成器 失败：{e}") });
                stats.error += 1;
                reason = "编译失败".to_string();
                emit(CheckEvent::Finish { stats, tested: 0, reason, fail_dir });
                return;
            }
        }
    } else {
        None
    };

    let mut rng = match params.seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_os_rng(),
    };

    let mut tested: i64 = 0;
    while !cancel.load(Ordering::Relaxed) && (total == -1 || tested < total) {
        tested += 1;
        let n = tested;

        // 1) 生成输入
        let input_bytes: Vec<u8> = match &params.gen_mode {
            GenMode::Builtin => match generate_with(&params.config, &mut rng) {
                Ok(l) => (l.join("\n") + "\n").into_bytes(),
                Err(e) => {
                    stats.error += 1;
                    emit(CheckEvent::Log { msg: format!("第 {n} 组：数据生成失败：{e}") });
                    reason = "因出错中止".to_string();
                    break;
                }
            },
            GenMode::External => {
                let (ext_cmd, ext_dir) = ext_ready.as_ref().expect("ext_ready");
                // legacy：设置种子时给外置生成器追加 --seed <seed>
                let mut args = parse_command(ext_cmd);
                if args.is_empty() {
                    stats.error += 1;
                    emit(CheckEvent::Log { msg: format!("第 {n} 组：外置生成器命令为空") });
                    reason = "因出错中止".to_string();
                    break;
                }
                if let Some(s) = params.seed {
                    args.push("--seed".to_string());
                    args.push(s.to_string());
                }
                let prog = args[0].clone();
                let r = run_argv_ex(args, ext_dir, b"", timeout, params.memory_limit_mb);
                if r.status == RunStatus::Timeout {
                    stats.error += 1;
                    emit(CheckEvent::Log { msg: format!("第 {n} 组：外置生成器超时（>{timeout}s）") });
                    reason = "因出错中止".to_string();
                    break;
                }
                if r.status == RunStatus::Error {
                    stats.error += 1;
                    emit(CheckEvent::Log { msg: format!("第 {n} 组：找不到外置生成器：{prog}") });
                    reason = "因出错中止".to_string();
                    break;
                }
                if r.returncode != Some(0) {
                    let msg = String::from_utf8_lossy(&r.stderr);
                    let snippet: String = msg.chars().take(200).collect();
                    stats.error += 1;
                    emit(CheckEvent::Log { msg: format!("第 {n} 组：外置生成器返回码 {}：{}", r.returncode.unwrap_or(-1), snippet) });
                    reason = "因出错中止".to_string();
                    break;
                }
                r.stdout
            }
        };
        if std::fs::write(work.path("test.in"), &input_bytes).is_err() {
            stats.error += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：写入 test.in 失败") });
            reason = "因出错中止".to_string();
            break;
        }

        // 2) 跑正解
        let r1 = run_program_ex(&sol_run, &sol_dir, &input_bytes, timeout, params.memory_limit_mb);
        if r1.status == RunStatus::Memory {
            stats.mle += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：正解 内存超限（峰值 {:.1}MB > {}MB）", r1.peak_bytes as f64 / 1048576.0, params.memory_limit_mb.unwrap_or(0)) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r1.status == RunStatus::Timeout {
            stats.tle += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：正解 超时（TLE，>{timeout}s）") });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r1.status == RunStatus::Error {
            stats.error += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：正解 运行出错：{}", r1.error) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r1.returncode != Some(0) {
            stats.re += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：正解 返回码 {}（RE）", r1.returncode.unwrap_or(-1)) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }

        // 3) 跑暴力
        let r2 = run_program_ex(&brute_run, &brute_dir, &input_bytes, timeout, params.memory_limit_mb);
        if r2.status == RunStatus::Memory {
            stats.mle += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：暴力 内存超限（峰值 {:.1}MB > {}MB）", r2.peak_bytes as f64 / 1048576.0, params.memory_limit_mb.unwrap_or(0)) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r2.status == RunStatus::Timeout {
            stats.tle += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：暴力 超时（TLE，>{timeout}s）") });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r2.status == RunStatus::Error {
            stats.error += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：暴力 运行出错：{}", r2.error) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }
        if r2.returncode != Some(0) {
            stats.re += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：暴力 返回码 {}（RE）", r2.returncode.unwrap_or(-1)) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }

        // 4) 比较
        let out1 = String::from_utf8_lossy(&r1.stdout).into_owned();
        let out2 = String::from_utf8_lossy(&r2.stdout).into_owned();
        let _ = std::fs::write(work.path("prog.out"), &out1);
        let _ = std::fs::write(work.path("std.out"), &out2);

        if compare(&out1, &out2, params.ignore_ws) {
            stats.pass += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：PASS（正解 {:.3}s / 暴力 {:.3}s）", r1.elapsed, r2.elapsed) });
        } else {
            stats.wa += 1;
            emit(CheckEvent::Log { msg: format!("第 {n} 组：答案不一致（WA）") });
            emit(CheckEvent::Log { msg: format!("    正解输出：{:?}", out1.chars().take(200).collect::<String>()) });
            emit(CheckEvent::Log { msg: format!("    暴力输出：{:?}", out2.chars().take(200).collect::<String>()) });
            fail_dir = save_fail(&work, emit);
            reason = "因出错中止".to_string();
            break;
        }

        stats.tested = tested as u64;
        emit(CheckEvent::Status { tested: tested as u64, total });
    }

    if reason.is_empty() {
        reason = if cancel.load(Ordering::Relaxed) {
            "手动停止".to_string()
        } else {
            "正常完成".to_string()
        };
    }
    stats.tested = tested as u64;
    emit(CheckEvent::Finish { stats, tested: tested as u64, reason, fail_dir });
}

/// 对拍摘要文本（供结束日志）。
pub fn finish_summary(stats: &CheckStats) -> String {
    let errs = stats.re + stats.error;
    format!(
        "  通过：{}    不一致(WA)：{}    超时(TLE)：{}    内存超限(MLE)：{}    运行错误(RE/Error)：{}",
        stats.pass, stats.wa, stats.tle, stats.mle, errs
    )
}
