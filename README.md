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
- 树类型 `type="star"`（菊花图）/ `type="chain"`（链）
- 已废弃：`val=`（节点权值）、`w=`（边权）、`prec=`（精度）等关键字写法，统一用位置参数
- 多测模式：顶部注释 `# 多测模式：重复 3 次`，首行输出组数，整块独立随机重复

完整语法见 [docs/DSL.md](docs/DSL.md)。

## 功能

- **图形化变量编辑**：行/数组/排列/树/图等类型卡片，参数表单 + 表达式自由输入，
  与 Monaco DSL 编辑器双向实时同步（语法高亮/补全/错误行标记）
- **数据生成预览**：种子可复现，复制/导出；外置生成器试运行
- **对拍**：正解/暴力（运行命令或 C++ 源码自动编译）、组数/超时/**内存限制**/
  种子/忽略行末空格；外置生成器（stdout 即测试数据）；PASS/WA/TLE/RE/MLE 统计，
  失败现场保存 `./fail/`
- **自然语言 → DSL**：输入中文/英文输入格式描述，规则引擎零延迟转换
  （置信度 + 来源标签 + 一键载入编辑器；llama.cpp 模型通道规划中，支持
  模型路径设置/加载/GitHub Releases 下载占位）

## 界面

五面板（可折叠/拖拽调高）：图形化变量列表 / DSL 编辑器 / 数据生成预览 /
对拍 / 自然语言 → DSL。
