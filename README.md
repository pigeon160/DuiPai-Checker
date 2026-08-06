# 对拍检查器（DuiPai Checker）

GitHub：[pigeon160/DuiPai-Checker](https://github.com/pigeon160/DuiPai-Checker)

离线桌面应用：用随机数据反复运行"正解"与"暴力"两份程序并比较输出，自动找出
**WA / TLE / RE / MLE** 反例，并保存现场供分析。

技术栈：**Rust（duipai-core）+ Tauri 2 + React/TypeScript**。完全离线，跨平台
（Windows / macOS / Linux）。旧版 Python（tkinter）实现已移入 [`legacy/`](legacy/)。

## 快速开始

```bash
npm install
npm run tauri dev        # 开发
npm run tauri build      # 打包安装包
```

## 目录结构

- `crates/duipai-core/`：纯 Rust 核心库（DSL 解析/校验/序列化、数据生成、对拍循环、进程管理、自然语言转换）
- `src-tauri/`：Tauri 薄胶水层（IPC 命令 + 事件）
- `src/`：React 前端（图形化变量编辑、Monaco DSL 编辑器、生成预览、对拍面板、自然语言面板）
- `docs/DSL.md`：完整 DSL 语法文档

## DSL 规则速览

### 结构：行块 + 顶层命令

```
line (3):                    # 行块（(3) 重复 3 行，可省=1 行；可写表达式如 line (k):）
    int n: 1, 100            # 行内子项：类型 名字: 参数
    float x: 0, 1, 4
    text s: "---"
    expr e: 2 * n
    str c: 10, "ab"
a = ints(3, 1, 9)            # 顶层命令（与行同级）
t = tree(n, int(1, 10))
```

- **行内项类型**（必须放在行块内，缩进一行一项）：
  `int`（随机整数）、`float`（随机浮点+精度）、`expr`（任意表达式）、
  `text`（固定文本）、`str`（随机字符串，长度可为 `int(3,5)` 区间随机）
- **顶层命令**（`name = 命令(...)`）：`ints` `floats` `matrix` `matf` `perm`
  `binseq` `intervals` `points` `tree` `graph` `ring` `base_ring`
- **行重复**：`line (3):` 重复 3 行、每行独立随机；重复后行内数值项**数组化**，
  引用须 `n[k]`（1 起取第 k 行）；同一行内后者可引用前者

### 表达式（任何数值位置）

```
常数 5 / 1.5    引用前面变量 n    数组索引 a[2]、M[i][j]
算术 2*n、n+1、n//2、n % 3、2 ** k
随机 int(1,5)、float(0,1,4)
```

### 关键规则

- 变量名：字母/下划线开头，仅字母/数字/下划线；不能是保留字；不能重复
- 只能引用**前面**定义的名字；数组/区间/点集不可整体引用（数组可用 `a[i]`）
- 树/图引用取其规模值 n；树/图**只输出边**（无规模行）
- 边权直接写范围：`tree(5, int(1, 9))`；图选项 `multi=1`（重边）/ `loop=1`（自环）/ `type="dag"|"bipartite"`
- 树类型 `type="star"`（菊花图）/ `type="chain"`（链）/ `type="parent"`（1 为根，输出父节点序列）
- 已废弃：`val=`（节点权值）、`w=`（边权）、`prec=`（精度）等关键字写法，统一用位置参数
- repeat 块：`repeat (N):` + 缩进语句，普通顶层语句（可多个/混排），整体重复 N 次、
  变量每轮覆盖（无组数行），块内变量块外不可见

完整语法见 [docs/DSL.md](docs/DSL.md)。

## 功能

- **图形化变量编辑**：行/数组/排列/树/图等类型卡片，参数表单 + 表达式自由输入，
  与 Monaco DSL 编辑器双向实时同步（语法高亮/补全/错误行标记）
- **数据生成预览**：种子可复现，复制/导出；外置生成器试运行
- **对拍**：正解/暴力（运行命令或 C++ 源码自动编译）、组数/超时/**内存限制**/
  种子/忽略行末空格；外置生成器（stdout 即测试数据）；PASS/WA/TLE/RE/MLE 统计，
  失败现场保存 `./fail/`
- **自然语言 → DSL**：输入中文/英文输入格式描述，规则引擎零延迟转换
  （置信度 + 来源标签 + 一键载入编辑器）；模型未命中或勾选「直接用模型」时，
  由本地大模型（llama.cpp）转换，界面显示模型思维链

## 模型获取（可选，启用大模型翻译）

安装包不含模型文件（单个模型超 GitHub 100MB 上限），首次使用大模型功能请在
「自然语言 → DSL」面板操作，二选一：

1. **一键下载（推荐）**：模型区「一键下载」下拉选择模型 → 点「下载所选模型」
   （自动从 hf-mirror 镜像下载，完成后自动设置路径 → 直接点「加载」）
2. **手动放置**：从 [Qwen2.5 GGUF](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF)
   下载 `qwen2.5-3b-instruct-q4_k_m.gguf` 放入程序同目录 `models/`，
   在路径框填入后点「设置路径」→「加载」

| 模型 | 大小 | 说明 |
|---|---|---|
| Qwen2.5-3B q4_k_m | ~2GB | 推荐：准确度最好（16 核 CPU 约 10-20s/次） |
| Qwen2.5-1.5B q4_k_m | ~1GB | 更快，准确度尚可 |
| Qwen2.5-0.5B q4_k_m | ~470MB | 最快，适合低配机器/简单题面 |

> 国内网络：程序内一键下载走 hf-mirror 镜像，无需代理；也可手动访问
> `https://hf-mirror.com/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf`

## 界面

五面板（可折叠）：图形化变量列表 / DSL 编辑器 / 数据生成预览 /
对拍 / 自然语言 → DSL。

## 安装

- **v1.0.0 起提供安装包**：GitHub Releases 下载
  `duipai-checker_<版本>_x64-setup.exe`（NSIS 安装包，安装后打开不弹命令行窗口）
- 开发模式：`npm install` + `npm run tauri dev`（需 Rust 工具链）
