//! 对拍循环测试：PASS / WA 落盘 / RE / 手动停止。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use duipai_core::{
    parse, run_check, CheckEvent, CheckParams, CheckStats, GenMode, ProgMode, ProgramSpec,
};

/// chdir 测试串行锁（save_fail 写当前工作目录）。
static CHDIR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn chdir_guard() -> std::sync::MutexGuard<'static, ()> {
    CHDIR_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

/// 读取 stdin 原样输出。
const ECHO: &str = "powershell -NoProfile -Command \"$s=[Console]::In.ReadToEnd(); Write-Output $s\"";

/// 读取 stdin，行首加 X 输出。
const ECHO_X: &str = "powershell -NoProfile -Command \"$s=[Console]::In.ReadToEnd(); Write-Output ('X' + $s)\"";

fn specs(sol: &str, brute: &str) -> (ProgramSpec, ProgramSpec) {
    (
        ProgramSpec { mode: ProgMode::RunCmd, cmd: sol.to_string(), dir: String::new(), label: "正解".into() },
        ProgramSpec { mode: ProgMode::RunCmd, cmd: brute.to_string(), dir: String::new(), label: "暴力".into() },
    )
}

fn params(sol: &str, brute: &str, total: i64) -> CheckParams {
    let cfg = parse("n = int(1, 5)\na = ints(n, 1, 9)\n").unwrap();
    let (sol, brute) = specs(sol, brute);
    CheckParams {
        sol,
        brute,
        gen_mode: GenMode::Builtin,
        ext: None,
        total,
        timeout: 10.0,
        memory_limit_mb: None,
        seed: Some(1),
        ignore_ws: false,
        compiler: "g++".into(),
        compile_flags: "-O2 -std=c++17".into(),
        config: cfg,
    }
}

fn collect(p: CheckParams) -> (Vec<CheckEvent>, CheckStats, std::path::PathBuf) {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut captured: Vec<CheckEvent> = Vec::new();
    let tmp;
    {
        let _guard = chdir_guard();
        let old = std::env::current_dir().unwrap();
        tmp = std::env::temp_dir().join(format!("duipai_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        run_check(&p, cancel, &mut |e| captured.push(e));
        std::env::set_current_dir(&old).unwrap();
    }
    let stats = captured
        .iter()
        .find_map(|e| match e {
            CheckEvent::Finish { stats, .. } => Some(stats.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (captured, stats, tmp)
}

#[test]
fn check_pass_all_rounds() {
    let (events, stats, tmp) = collect(params(ECHO, ECHO, 3));
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(stats.pass, 3, "events={events:?}");
    assert_eq!(stats.tested, 3);
    assert_eq!(stats.wa, 0);
    let finish = events.iter().find_map(|e| match e {
        CheckEvent::Finish { reason, .. } => Some(reason.clone()),
        _ => None,
    });
    assert_eq!(finish.as_deref(), Some("正常完成"));
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("PASS")), "{logs:?}");
}

#[test]
fn check_wa_saves_fail() {
    let (events, stats, tmp) = collect(params(ECHO, ECHO_X, 3));
    let fail_dir = std::path::Path::new(&tmp).join("fail");
    // 第一组即 WA，中止
    assert_eq!(stats.wa, 1, "events={events:?}");
    assert_eq!(stats.tested, 1);
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("WA")), "{logs:?}");
    assert!(logs.iter().any(|m| m.contains("现场已保存")), "{logs:?}");
    // 现场文件
    assert!(fail_dir.join("fail_1.in").exists());
    assert!(fail_dir.join("fail_1_prog.out").exists());
    assert!(fail_dir.join("fail_1_std.out").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn check_re_aborts() {
    let (events, stats, tmp) = collect(params(ECHO, "cmd /C exit /b 3", 3));
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(stats.re, 1, "events={events:?}");
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("RE")), "{logs:?}");
}

#[test]
fn check_cancel_stops() {
    let p = params(ECHO, ECHO, -1);
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let mut events: Vec<CheckEvent> = Vec::new();
    let tmp;
    {
        let _guard = chdir_guard();
        let old = std::env::current_dir().unwrap();
        tmp = std::env::temp_dir().join(format!("duipai_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        run_check(&p, c, &mut |e| {
            // 第一组状态事件后请求停止
            if matches!(e, CheckEvent::Status { .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
            events.push(e);
        });
        std::env::set_current_dir(&old).unwrap();
    }
    let _ = std::fs::remove_dir_all(&tmp);
    let finish = events.iter().find_map(|e| match e {
        CheckEvent::Finish { reason, tested, .. } => Some((reason.clone(), *tested)),
        _ => None,
    });
    let (reason, tested) = finish.expect("finish event");
    assert_eq!(reason, "手动停止");
    assert!(tested <= 2, "取消后应尽快停止，实际 {tested} 组");
}

#[test]
fn check_compilation_error_reported() {
    // C++ 源码模式：源码不存在 -> 编译失败 -> 立即结束
    let (_, brute) = specs(ECHO, ECHO);
    let cfg = parse("n = int(1, 5)\n").unwrap();
    let p = CheckParams {
        sol: ProgramSpec {
            mode: ProgMode::CppSource,
            cmd: "D:/definitely/not/exists.cpp".into(),
            dir: String::new(),
            label: "正解".into(),
        },
        brute,
        gen_mode: GenMode::Builtin,
        ext: None,
        total: 1,
        timeout: 10.0,
        memory_limit_mb: None,
        seed: Some(1),
        ignore_ws: false,
        compiler: "g++".into(),
        compile_flags: "-O2".into(),
        config: cfg,
    };
    let (events, stats, tmp) = collect(p);
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(stats.error, 1, "events={events:?}");
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("编译")), "{logs:?}");
}

#[test]
fn check_external_generator() {
    // 外置生成器：stdout 即测试数据；正解/暴力 echo stdin -> PASS
    let (sol, brute) = specs(ECHO, ECHO);
    let cfg = parse("").unwrap();
    let p = CheckParams {
        sol,
        brute,
        gen_mode: GenMode::External,
        ext: Some(ProgramSpec {
            mode: ProgMode::RunCmd,
            cmd: "cmd /C echo 5".into(),
            dir: String::new(),
            label: "外置生成器".into(),
        }),
        total: 3,
        timeout: 10.0,
        memory_limit_mb: None,
        seed: Some(1),
        ignore_ws: false,
        compiler: "g++".into(),
        compile_flags: "-O2".into(),
        config: cfg,
    };
    let (events, stats, tmp) = collect(p);
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(stats.pass, 3, "events={events:?}");
    // 测试数据 = "5\n"（echo 输出）
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("PASS")), "{logs:?}");
}

#[test]
fn check_external_generator_fails() {
    // 外置生成器返回码非 0 -> 数据生成失败中止
    let (sol, brute) = specs(ECHO, ECHO);
    let p = CheckParams {
        sol,
        brute,
        gen_mode: GenMode::External,
        ext: Some(ProgramSpec {
            mode: ProgMode::RunCmd,
            cmd: "cmd /C exit /b 3".into(),
            dir: String::new(),
            label: "外置生成器".into(),
        }),
        total: 3,
        timeout: 10.0,
        memory_limit_mb: None,
        seed: None,
        ignore_ws: false,
        compiler: "g++".into(),
        compile_flags: "-O2".into(),
        config: parse("").unwrap(),
    };
    let (events, stats, tmp) = collect(p);
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(stats.error, 1, "events={events:?}");
    assert_eq!(stats.tested, 1);
    let logs: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            CheckEvent::Log { msg } => Some(msg.clone()),
            _ => None,
        })
        .collect();
    assert!(logs.iter().any(|m| m.contains("外置生成器返回码 3")), "{logs:?}");
}
