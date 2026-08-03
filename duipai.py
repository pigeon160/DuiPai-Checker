#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
竞赛对拍机（图形化对拍工具）

功能：
  1. 指定"正解代码"与"暴力代码"的运行命令；
  2. 通过内置生成器（图形化配置变量）或外置生成器产生随机测试数据；
  3. 数组的"长度来源"可引用前面变量的值作为长度（取整）；
  4. 反复运行两份程序并比较输出，找出 WA / TLE / RE 的反例；
  5. 失败时把测试数据与双方输出保存到 ./fail/ 目录供分析。
  6. 内置生成器面板内提供"生成样例预览"区，预览不弹窗。

纯标准库实现：tkinter / ttk / subprocess / threading / random / queue 等。
跨平台：Linux / Windows / macOS。Windows 下可用 PyInstaller 打包为无控制台 exe
（build.bat），且所有子进程都带 CREATE_NO_WINDOW，不会闪现终端窗口。

线程安全说明：
  对拍主循环在独立后台线程中运行，该线程只访问纯 Python 数据与 subprocess，
  绝不直接调用任何 Tcl/Tk 接口；所有 UI 更新（日志、状态栏、结束处理）都通过
  一个 queue.Queue 投递，由主线程的轮询回调（after 轮询）统一处理。
"""

import os
import queue
import random
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import json
import tkinter as tk
from tkinter import ttk, filedialog, messagebox, scrolledtext


def format_float(v, precision):
    """把浮点数 v 按指定小数位格式化，去掉多余的尾零与小数点，避免输出过长。"""
    s = f"{v:.{precision}f}".rstrip("0").rstrip(".")
    return s if s not in ("", "-0") else "0"


_DSL_EMPTY_HINT = """\
# 当前无变量。
# 在上方点击“+ 整数变量”等添加，或在下方直接输入 DSL 脚本后点“应用（转为图形化）”。
# 示例：
#   n  = int(1, 100)
#   a  = ints(n, 1, 9)
#   s  = str(a, "01")
"""

_DSL_HELP = """\
对拍输入 DSL 语法：

# 注释；一条语句一行，等号赋值，顺序执行，可引用前面定义的名字。

表达式（任何数值位置都可写）：
    常数        5、1.5
    引用        前面变量的名字，如 n
    表达式      2*n、n+1、n//2
    范围随机    int(1,5)、float(0,1,4)（min~max 随机取一个）

命令一览：
    n  = int(1, 100)              # 整数变量，输出一行
    x  = float(0, 1)              # 浮点变量（默认6位精度）
    x4 = float(0, 1, 4)           # 浮点变量，精度4位
    a  = ints(n, 1, 9)            # 数组：一行 n 个整数
    b  = floats(3, 0, 1)          # 数组：一行 3 个浮点
    c  = ints(int(1,5), 1, 9)     # 数组：个数随机 1~5
    M  = matrix(3, n, 0, 1)       # 数组：3 行 × n 列整数（多行）
    p  = perm(n)                  # 排列：一行 1..n 随机排列
    t  = tree(n)                  # 树：首行 n + n-1 条边，无边权
    t2 = tree(n, w=int(1, 10))    # 树 + 整数边权
    t3 = tree(n, w=float(0, 1, 4))# 树 + 浮点边权
    t4 = tree(n, w=int(1,5), val=int(1,9))  # 树 + 节点权值
    g  = graph(n, m, 1, 0, w=int(1,10))     # 图：n 顶点 m 边（3/4位为有向/连通）
    g2 = graph(n, m, 1, 0, type="dag")      # 有向无环图
    g3 = graph(n, m, 0, 1, type="bipartite")# 二分图
    r  = ring(n)                  # 环：n 条边首尾相连
    br = base_ring(n, k)          # 基环树：n 顶点，环大小 k
    s  = str(n, "ab")             # 字符串：一行 n 个字符（字符集可省）
    M  = strs(2, n, "01")         # 字符网格：2 行 × n 列
    z  = binseq(n, k)             # 0/1 序列：n 位，恰好 k 个 1
    iv = intervals(n, 1, 100)     # 区间：n 行，每行 l r
    pt = points(n, 1, 10, 1, 10)  # 点集：n 行，每行 x y

引用规则：只能引用前面定义的名字；数组/字符串/序列等不可被引用；
perm/tree/graph 被引用时取其规模值。
多组数据：在“多测模式”勾选框里填“重复次数”并勾选，首行输出组数 N，
随后整块变量独立随机重复 N 次（每轮可引用仅当轮前面的值）。
等价于在 DSL 顶部写：# 多测模式：重复 N 次
"""


class VariableRow:
    """内置生成器中的一个变量条目（一行或多行 UI）。"""

    KIND_NAMES = {"int": "整数", "float": "浮点数", "array": "数组",
                  "perm": "排列", "tree": "树", "graph": "图",
                  "string": "字符串", "binseq": "0/1序列",
                  "intervals": "区间", "points": "点集"}

    def __init__(self, parent, kind, app):
        self.kind = kind          # 'int'/'float'/'array'/'perm'/'tree'/'graph'
        self.app = app
        self.name = ""            # DSL 变量名（GUI 自动生成时为空，快照时补全）
        self._extra_sources = {}  # attr -> [(显示文本, 表达式字符串), ...]
        self.frame = tk.Frame(parent, relief="ridge", bd=1)
        self._build()

    def _cell(self):
        """在主体流式容器 self.body 中创建一个单元格（子帧）。"""
        cell = tk.Frame(self.body, bg=self.body.cget("bg"))
        return cell

    def _build_source(self, label, attr, refs_attr, entries, defaults):
        """创建一个“随机范围/引用变量”来源单元格，返回该单元格。"""
        cell = self._cell()
        _p = {"pady": (3, 3)}
        lbl = ttk.Label(cell, text=label)
        lbl.pack(side="left", padx=(6, 0), **_p)
        setattr(self, attr + "_label", lbl)
        var = tk.StringVar(value="随机范围")
        setattr(self, attr + "_var", var)
        cb = ttk.Combobox(cell, textvariable=var, values=["随机范围"], width=9,
                          state="readonly")
        setattr(self, attr, cb)
        setattr(self, refs_attr, [])          # [(显示文本, VariableRow), ...]
        cb.pack(side="left", padx=2, **_p)
        cb.bind("<<ComboboxSelected>>",
                lambda e: self._apply_source_state(var, entries))
        for ename, dflt in zip(entries, defaults):
            en = ttk.Entry(cell, width=4)
            en.insert(0, dflt)
            setattr(self, ename, en)
            en.pack(side="left", padx=1, **_p)
            if ename.endswith("_max"):
                ttk.Label(cell, text="~").pack(side="left", padx=1, **_p)
        return cell

    def _apply_source_state(self, var, entries):
        """“随机范围”时启用对应输入框，引用变量/表达式时禁用。"""
        state = "normal" if var.get() == "随机范围" else "disabled"
        for name in entries:
            getattr(self, name).configure(state=state)

    def _set_sources(self, attr, refs_attr, entries, values, refs, current,
                     extra=None):
        """刷新某个“来源”下拉的选项与引用列表。extra: [(显示文本, 表达式),...]"""
        setattr(self, refs_attr, refs)
        self._extra_sources[attr] = extra or []
        all_values = values + [t for t, _ in (extra or [])]
        getattr(self, attr).configure(values=all_values)
        getattr(self, attr + "_var").set(current)
        self._apply_source_state(getattr(self, attr + "_var"), entries)

    def _ref_row(self, refs_attr, label):
        """根据下拉选项文本找到对应的变量行对象。"""
        for lbl, row in getattr(self, refs_attr):
            if lbl == label:
                return row
        return None

    def _type_label(self, f, text):
        """创建行首的折叠按钮 + 类型标签容器（放在 column=0）。

        折叠按钮在最前，点击折叠/展开该行；折叠时隐藏主体控件。"""
        container = tk.Frame(f, bg=self.frame.cget("bg"))
        container.grid(row=0, column=0, padx=(2, 0), sticky="ns")
        self._fold_btn = ttk.Button(container, text="▾", width=2,
                                    command=self._toggle_collapse)
        self._fold_btn.pack(side="left", padx=(0, 2), pady=2)
        ttk.Label(container, text=text, width=10, style="Tag.TLabel").pack(
            side="left", padx=(0, 4))
        self.collapsed = False
        self._body_widgets = [self.body]   # 折叠时隐藏整个主体流容器
        return container

    def _toggle_collapse(self):
        """折叠/展开变量行（隐藏或恢复主体控件）。"""
        self.collapsed = not self.collapsed
        self._fold_btn.configure(text="▸" if self.collapsed else "▾")
        try:
            if self.collapsed:
                self.body.grid_remove()
            else:
                self.body.grid(row=0, column=1, sticky="nsew")
                self.app._flow_pack(self.body)
        except tk.TclError:
            pass
        self.app._refresh_sources()
        self.app._fit_var_inner_size()

    def _build(self):
        f = self.frame
        f.columnconfigure(1, weight=1)
        self.body = tk.Frame(f, bg=f.cget("bg"))
        self.body.grid(row=0, column=1, sticky="nsew")
        self.body.bind("<Configure>", self._on_body_configure)
        self.body._hidden_cells = set()
        self._type_label(f, self.KIND_NAMES.get(self.kind, self.kind))

        if self.kind == "int":
            c = self._cell()
            ttk.Label(c, text="最小值:").pack(side="left", padx=(4, 0), pady=(3, 3))
            self.min_entry = ttk.Entry(c, width=8)
            self.min_entry.insert(0, "1")
            self.min_entry.pack(side="left", padx=2, pady=(3, 3))
            c = self._cell()
            ttk.Label(c, text="最大值:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.max_entry = ttk.Entry(c, width=8)
            self.max_entry.insert(0, "100")
            self.max_entry.pack(side="left", padx=2, pady=(3, 3))
        elif self.kind == "float":
            c = self._cell()
            ttk.Label(c, text="最小值:").pack(side="left", padx=(4, 0), pady=(3, 3))
            self.min_entry = ttk.Entry(c, width=8)
            self.min_entry.insert(0, "0.0")
            self.min_entry.pack(side="left", padx=2, pady=(3, 3))
            c = self._cell()
            ttk.Label(c, text="最大值:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.max_entry = ttk.Entry(c, width=8)
            self.max_entry.insert(0, "1.0")
            self.max_entry.pack(side="left", padx=2, pady=(3, 3))
            c = self._cell()
            ttk.Label(c, text="精度:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.prec_entry = ttk.Entry(c, width=3)
            self.prec_entry.insert(0, "6")
            self.prec_entry.pack(side="left", padx=2, pady=(3, 3))
        elif self.kind == "array":
            c = self._cell()
            ttk.Label(c, text="元素:").pack(side="left", padx=(4, 0), pady=(3, 3))
            self.elem_type = ttk.Combobox(c, values=["整数", "浮点数"],
                                          width=5, state="readonly")
            self.elem_type.current(0)
            self.elem_type.pack(side="left", padx=2, pady=(3, 3))
            self.elem_type.bind("<<ComboboxSelected>>", self._toggle_elem_type)
            ttk.Label(c, text="范围:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.el_min = ttk.Entry(c, width=5)
            self.el_min.insert(0, "1")
            self.el_min.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="~").pack(side="left", pady=(3, 3))
            self.el_max = ttk.Entry(c, width=5)
            self.el_max.insert(0, "100")
            self.el_max.pack(side="left", padx=2, pady=(3, 3))
            self.prec_label = ttk.Label(c, text="精度:")
            self.prec_entry = ttk.Entry(c, width=3)
            self.prec_entry.insert(0, "6")
            self.prec_label.pack(side="left", padx=(6, 0), pady=(3, 3))
            self.prec_entry.pack(side="left", padx=2, pady=(3, 3))
            self._toggle_elem_type()
            self._build_source("行数:", "rows_source",
                               "_rows_refs", ["rows_min", "rows_max"],
                               ["1", "1"])
            self._build_source("每行长度:", "len_source",
                               "_len_refs", ["len_min", "len_max"],
                               ["1", "10"])
        elif self.kind == "perm":
            self._build_source("长度n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["1", "10"])
        elif self.kind == "string":
            self._build_source("长度:", "len_source",
                               "_len_refs", ["len_min", "len_max"],
                               ["1", "10"])
            self._build_source("行数:", "rows_source",
                               "_rows_refs", ["rows_min", "rows_max"],
                               ["1", "1"])
            c = self._cell()
            ttk.Label(c, text="字符集:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.charset_entry = ttk.Entry(c, width=12)
            self.charset_entry.insert(0, "abcdefghijklmnopqrstuvwxyz")
            self.charset_entry.pack(side="left", padx=2, pady=(3, 3))
        elif self.kind == "binseq":
            self._build_source("长度n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["1", "10"])
            self._build_source("1的个数k:", "k_source",
                               "_k_refs", ["k_min", "k_max"],
                               ["1", "5"])
        elif self.kind == "intervals":
            self._build_source("个数n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["1", "10"])
            c = self._cell()
            ttk.Label(c, text="范围:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.iv_lo = ttk.Entry(c, width=5)
            self.iv_lo.insert(0, "1")
            self.iv_lo.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="~").pack(side="left", pady=(3, 3))
            self.iv_hi = ttk.Entry(c, width=5)
            self.iv_hi.insert(0, "100")
            self.iv_hi.pack(side="left", padx=2, pady=(3, 3))
        elif self.kind == "points":
            self._build_source("个数n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["1", "10"])
            c = self._cell()
            ttk.Label(c, text="x:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.pt_xlo = ttk.Entry(c, width=5)
            self.pt_xlo.insert(0, "1")
            self.pt_xlo.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="~").pack(side="left", pady=(3, 3))
            self.pt_xhi = ttk.Entry(c, width=5)
            self.pt_xhi.insert(0, "100")
            self.pt_xhi.pack(side="left", padx=2, pady=(3, 3))
            c = self._cell()
            ttk.Label(c, text="y:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.pt_ylo = ttk.Entry(c, width=5)
            self.pt_ylo.insert(0, "1")
            self.pt_ylo.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="~").pack(side="left", pady=(3, 3))
            self.pt_yhi = ttk.Entry(c, width=5)
            self.pt_yhi.insert(0, "100")
            self.pt_yhi.pack(side="left", padx=2, pady=(3, 3))
        elif self.kind == "tree":
            self._build_source("顶点数n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["2", "8"])
            self._build_weight_cell("边权:", "w_")
            self._build_weight_cell("节点权值:", "v_")
        elif self.kind == "graph":
            self._build_source("顶点数n:", "n_source",
                               "_n_refs", ["n_min", "n_max"],
                               ["2", "6"])
            self._m_cell = self._build_source("边数m:", "m_source",
                                              "_m_refs", ["m_min", "m_max"],
                                              ["2", "6"])
            c = self._cell()
            ttk.Label(c, text="类型:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.g_dir_var = tk.StringVar(value="无向")
            g_dir = ttk.Combobox(c, textvariable=self.g_dir_var,
                                 values=["无向", "有向"], width=4,
                                 state="readonly")
            g_dir.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="连通:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.g_conn_var = tk.StringVar(value="任意")
            g_conn = ttk.Combobox(c, textvariable=self.g_conn_var,
                                  values=["任意", "连通"], width=4,
                                  state="readonly")
            g_conn.pack(side="left", padx=2, pady=(3, 3))
            ttk.Label(c, text="结构:").pack(side="left", padx=(6, 0), pady=(3, 3))
            self.g_type_var = tk.StringVar(value="一般")
            g_type = ttk.Combobox(c, textvariable=self.g_type_var,
                                  values=["一般", "二分图", "DAG", "环", "基环树"],
                                  width=5, state="readonly")
            g_type.pack(side="left", padx=2, pady=(3, 3))
            g_type.bind("<<ComboboxSelected>>", self._toggle_g_type)
            self._build_weight_cell("边权:", "w_")
            self._build_weight_cell("节点权值:", "v_")

        # 右侧按钮：上移 / 下移 / 删除
        c = self._cell()
        ttk.Button(c, text="▲", width=3,
                   command=lambda: self.app.move_row(self, -1)).pack(
            side="left", padx=(8, 2), pady=(3, 3))
        ttk.Button(c, text="▼", width=3,
                   command=lambda: self.app.move_row(self, 1)).pack(
            side="left", padx=2, pady=(3, 3))
        ttk.Button(c, text="✕", width=3,
                   command=lambda: self.app.delete_row(self)).pack(
            side="left", padx=2, pady=(3, 3))

        if self.kind == "graph":
            self._toggle_g_type()
        self.app._flow_pack(self.body)

    def _build_weight_cell(self, label, prefix):
        """创建边权/节点权值单元格（模式下拉 + 范围 + 精度）。prefix: 'w_'/'v_'。"""
        c = self._cell()
        ttk.Label(c, text=label).pack(side="left", padx=(6, 0), pady=(3, 3))
        setattr(self, prefix + "mode_var", tk.StringVar(value="无"))
        mode = ttk.Combobox(c, textvariable=getattr(self, prefix + "mode_var"),
                            values=["无", "整数", "浮点"], width=4,
                            state="readonly")
        setattr(self, prefix + "mode", mode)
        mode.pack(side="left", padx=2, pady=(3, 3))
        mode.bind("<<ComboboxSelected>>",
                  lambda e, p=prefix: self._toggle_weight_mode(p))
        rng_label = ttk.Label(c, text="范围:")
        r_min = ttk.Entry(c, width=4)
        r_min.insert(0, "1")
        tilde = ttk.Label(c, text="~")
        r_max = ttk.Entry(c, width=4)
        r_max.insert(0, "10")
        prec_label = ttk.Label(c, text="精度:")
        prec = ttk.Entry(c, width=3)
        prec.insert(0, "6")
        setattr(self, prefix + "range_label", rng_label)
        setattr(self, prefix + "min", r_min)
        setattr(self, prefix + "tilde", tilde)
        setattr(self, prefix + "max", r_max)
        setattr(self, prefix + "prec_label", prec_label)
        setattr(self, prefix + "prec", prec)
        self._toggle_weight_mode(prefix)
        return c

    def _on_body_configure(self, event=None):
        """主体流容器尺寸变化：重新流式排布单元格。"""
        self.app._flow_pack(self.body)

    def _toggle_elem_type(self, event=None):
        """根据数组元素类型，显示或隐藏精度输入框。"""
        is_float = self.elem_type.get() == "浮点数"
        if is_float:
            self.prec_label.pack(side="left", padx=(6, 0), pady=(3, 3))
            self.prec_entry.pack(side="left", padx=2, pady=(3, 3))
        else:
            self.prec_label.pack_forget()
            self.prec_entry.pack_forget()
        self.app._flow_pack(self.body)
        self.app._fit_var_inner_size()

    def _toggle_w_mode(self, event=None):
        """根据边权模式显示/隐藏权重范围与精度输入框。"""
        self._toggle_weight_mode("w_")

    def _toggle_v_mode(self, event=None):
        """根据节点权值模式显示/隐藏范围与精度输入框。"""
        self._toggle_weight_mode("v_")

    def _toggle_weight_mode(self, prefix, event=None):
        """按 prefix（'w_'/'v_'）显示/隐藏范围与精度输入框。"""
        mode = getattr(self, prefix + "mode_var").get()
        for name in ("range_label", "min", "tilde", "max",
                     "prec_label", "prec"):
            getattr(self, prefix + name).pack_forget()
        if mode != "无":
            getattr(self, prefix + "range_label").pack(
                side="left", padx=(6, 0), pady=(3, 3))
            getattr(self, prefix + "min").pack(side="left", padx=2, pady=(3, 3))
            getattr(self, prefix + "tilde").pack(side="left", pady=(3, 3))
            getattr(self, prefix + "max").pack(side="left", padx=2, pady=(3, 3))
            if mode == "浮点":
                getattr(self, prefix + "prec_label").pack(
                    side="left", padx=(6, 0), pady=(3, 3))
                getattr(self, prefix + "prec").pack(side="left", padx=2, pady=(3, 3))
        self.app._flow_pack(self.body)
        self.app._fit_var_inner_size()

    def _toggle_g_type(self, event=None):
        """根据图结构类型显示/隐藏边数 m 与环大小输入。"""
        t = self.g_type_var.get()
        hidden = self.body._hidden_cells
        if t == "环":
            hidden.add(self._m_cell)
            if hasattr(self, "_k_cell"):
                hidden.add(self._k_cell)
        elif t == "基环树":
            hidden.add(self._m_cell)
            if not hasattr(self, "_k_cell"):
                kc = self._cell()
                self._k_cell = kc
                self.k_label = ttk.Label(kc, text="环大小k:")
                self.k_label.pack(side="left", padx=(6, 0), pady=(3, 3))
                self.k_min = ttk.Entry(kc, width=4)
                self.k_min.insert(0, "3")
                self.k_min.pack(side="left", padx=2, pady=(3, 3))
                self.k_tilde = ttk.Label(kc, text="~")
                self.k_tilde.pack(side="left", pady=(3, 3))
                self.k_max = ttk.Entry(kc, width=4)
                self.k_max.insert(0, "6")
                self.k_max.pack(side="left", padx=2, pady=(3, 3))
            hidden.discard(self._k_cell)
        else:
            hidden.discard(self._m_cell)
            if hasattr(self, "_k_cell"):
                hidden.add(self._k_cell)
        self.app._flow_pack(self.body)
        self.app._fit_var_inner_size()


class Application(tk.Tk):
    """主窗口：负责 UI 构建与对拍核心逻辑。"""

    def __init__(self):
        super().__init__()
        self.title("竞赛对拍机")
        self.geometry("1180x800")
        self.minsize(900, 620)

        self.rows = []                 # 内置生成器的变量条目列表
        self._scroll_regions = []      # 自滚动区域（滚轮滚动链），构建时填充
        self.worker = None             # 对拍后台线程
        self.running = False           # 是否正在对拍
        self._trying = False           # 是否有试运行在进行
        self.stop_event = threading.Event()
        self.msg_queue = queue.Queue() # 后台线程 → 主线程的消息队列
        self.stats = None
        self.tested = 0
        self.finish_reason = ""
        self._sections = {}            # 各可折叠区块：key -> 状态字典

        self._setup_vars()
        self._setup_style()
        self._initial_collapsed = self._load_state()
        self._build_ui()
        self._apply_collapsed_state()
        self._build_menu()
        try:
            self.iconphoto(True, self._make_icon())
        except tk.TclError:
            pass
        self.protocol("WM_DELETE_WINDOW", self._on_close)

        # 随窗口大小的控件缩放：根窗口宽度变化时按比例调整全局字号与控件尺寸
        self._scale = 1.0
        self._scale_pending = False
        self.bind("<Configure>", self._on_window_configure)

        # 整窗滚轮（在自滚动区域之外滚动整个界面）
        self.bind_all("<MouseWheel>", self._on_window_wheel)
        self.bind_all("<Button-4>", self._on_window_wheel)
        self.bind_all("<Button-5>", self._on_window_wheel)

        # 启动主线程的队列轮询，处理后台线程投递的 UI 更新
        self._poll_id = None
        self._poll_id = self.after(100, self._poll_queue)

    # ------------------------------------------------------------------ #
    # UI 构建
    # ------------------------------------------------------------------ #
    def _setup_vars(self):
        """初始化所有 tk 变量。"""
        self.sol_cmd = tk.StringVar()
        self.brute_cmd = tk.StringVar()
        self.sol_mode = tk.StringVar(value="运行命令")   # 运行命令 / C++ 源码
        self.brute_mode = tk.StringVar(value="运行命令")
        self.compiler = tk.StringVar(value="g++")
        self.compile_flags = tk.StringVar(value="-O2 -std=c++17")
        self.gen_mode = tk.StringVar(value="builtin")   # builtin / external
        self.ext_gen_cmd = tk.StringVar()
        self.ext_gen_mode = tk.StringVar(value="运行命令")
        self.rounds = tk.StringVar(value="10000")
        self.timeout = tk.StringVar(value="5")
        self.seed = tk.StringVar()
        self.ignore_ws = tk.BooleanVar(value=False)
        self.status_var = tk.StringVar(value="已测试：0")
        self.multi_test = tk.BooleanVar(value=False)
        self.repeat_times = tk.StringVar(value="1")

        # 各程序所在目录（浏览文件时自动记录，用于解析相对路径）
        cwd = os.getcwd()
        self.sol_dir = cwd
        self.brute_dir = cwd
        self.extgen_dir = cwd

    @staticmethod
    def _base_font():
        """根据平台返回基础中文字体。"""
        if sys.platform == "darwin":
            return ("PingFang SC", 13)
        if os.name == "nt":
            return ("Microsoft YaHei UI", 10)
        return ("Noto Sans CJK SC", 10)

    @classmethod
    def _bold_font(cls):
        fam, size = cls._base_font()
        return (fam, size, "bold")

    def _setup_style(self):
        """朴素风格：自适应原生主题 + 浅色界面 + 默认控件外观。"""
        style = ttk.Style(self)
        avail = set(style.theme_names())
        if sys.platform == "darwin":
            theme = "aqua" if "aqua" in avail else ("clam" if "clam" in avail else "default")
        elif os.name == "nt":
            theme = "vista" if "vista" in avail else ("clam" if "clam" in avail else "default")
        else:
            theme = "clam" if "clam" in avail else "default"
        try:
            style.theme_use(theme)
        except tk.TclError:
            pass

        # 朴素调色板
        self.accent = "#2f6fed"      # 蓝色强调
        self.black = "#111111"
        self.panel_bg = "#f4f6fa"    # 浅灰蓝底
        self.status_bg = "#2b3440"
        self.status_fg = "#ffffff"
        self.text_bg = "#ffffff"
        self.text_fg = "#222222"

        try:
            style.configure(".", font=self._base_font())
        except tk.TclError:
            pass
        style.configure("TLabelframe.Label", foreground=self.accent,
                        font=self._bold_font())
        style.configure("TButton", padding=(10, 4))
        style.configure("Tag.TLabel", foreground=self.accent,
                        font=self._bold_font())
        style.configure("Hint.TLabel", foreground="#6b6b6b")
        try:
            self.configure(bg=self.panel_bg)
            style.configure("TFrame", background=self.panel_bg)
            style.configure("TLabelframe", background=self.panel_bg)
        except tk.TclError:
            pass

    def _build_menu(self):
        """顶部菜单栏（文件 / 帮助）。"""
        menubar = tk.Menu(self)
        fm = tk.Menu(menubar, tearoff=0)
        fm.add_command(label="退出", command=self._on_close)
        menubar.add_cascade(label="文件", menu=fm)
        hm = tk.Menu(menubar, tearoff=0)
        hm.add_command(label="使用说明", command=self._show_help)
        hm.add_command(label="关于", command=self._show_about)
        menubar.add_cascade(label="帮助", menu=hm)
        self.config(menu=menubar)

    def _show_help(self):
        """显示使用说明。"""
        messagebox.showinfo(
            "使用说明",
            "1. 正解/暴力：可填“运行命令”（如 python3 ./sol.py、./sol）\n"
            "   或选“C++ 源码”填 .cpp 路径——开始对拍时会自动用 g++ 编译。\n"
            "2. 点“试运行”可按当前生成器生成一份样例并运行对应程序，\n"
            "   结果显示在“试运行输出”区，便于正式对拍前验证。\n"
            "3. 生成器：内置可添加 整数/浮点/数组/排列/树/图 变量；\n"
            "   数组行数、树/图 n、m 等都可引用前面变量的值（取整）。\n"
            "4. 设置组数（-1 无限）、超时、随机种子后点击“开始对拍”。\n"
            "5. 发现不一致(WA)时，测试数据与双方输出会保存到 ./fail/ 目录。")

    def _show_about(self):
        """显示关于信息。"""
        messagebox.showinfo("关于", "竞赛对拍机\n"
                                    "基于 tkinter 的图形化对拍工具\n"
                                    "纯标准库实现，跨平台（Linux / Windows / macOS）。")

    def _px_rounded_rect(self, img, x0, y0, x1, y1, r, color):
        """在 PhotoImage 上绘制一个填充的圆角矩形（像素级）。"""
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                dx = max(x0 + r - x, 0, x - (x1 - r))
                dy = max(y0 + r - y, 0, y - (y1 - r))
                if dx * dx + dy * dy <= r * r:
                    img.put(color, (x, y))

    def _make_icon(self):
        """程序化绘制一个 32x32 应用图标（对拍/对比含义），零外部文件。"""
        size = 32
        img = tk.PhotoImage(width=size, height=size)
        bg = "#dfe6f0"
        for y in range(size):
            for x in range(size):
                img.put(bg, (x, y))
        # 外圈圆角徽标
        self._px_rounded_rect(img, 2, 2, 29, 29, 7, self.accent)
        self._px_rounded_rect(img, 5, 5, 26, 26, 5, bg)
        # 左绿右红两个对比面板
        self._px_rounded_rect(img, 7, 9, 14, 23, 3, "#3eb489")
        self._px_rounded_rect(img, 17, 9, 24, 23, 3, "#e05561")
        # 中间白色分隔
        for y in range(10, 23):
            img.put("#ffffff", (15, y))
            img.put("#ffffff", (16, y))
        return img

    def _build_ui(self):
        outer = ttk.Frame(self)
        outer.pack(fill="both", expand=True)

        # 整窗滚动区：所有功能区块放进外层 Canvas，右侧带纵向滚动条
        wrap = ttk.Frame(outer)
        wrap.pack(fill="both", expand=True)
        wrap.rowconfigure(0, weight=1)
        wrap.columnconfigure(0, weight=1)

        self.body_canvas = tk.Canvas(wrap, highlightthickness=0,
                                     yscrollincrement=1)
        self.body_vbar = ttk.Scrollbar(wrap, orient="vertical",
                                       command=self.body_canvas.yview)
        self.body_canvas.configure(yscrollcommand=self.body_vbar.set)
        self.body_inner = ttk.Frame(self.body_canvas, padding=8)
        self.body_inner_window = self.body_canvas.create_window(
            (0, 0), window=self.body_inner, anchor="nw")
        self.body_canvas.grid(row=0, column=0, sticky="nsew")
        self.body_vbar.grid(row=0, column=1, sticky="ns")

        self.body_inner.bind("<Configure>", self._on_body_inner_configure)
        self.body_canvas.bind("<Configure>", self._on_body_canvas_configure)

        self._build_program_section(self.body_inner)
        self._build_source_section(self.body_inner)
        self._build_generator_section(self.body_inner)
        self._build_param_section(self.body_inner)
        self._build_control_section(self.body_inner)

        # 底部状态栏固定在窗口底部
        self.status_label = tk.Label(outer, textvariable=self.status_var,
                                     bg=self.status_bg, fg=self.status_fg,
                                     anchor="w", padx=10, pady=5)
        self.status_label.pack(fill="x", pady=(6, 0))

        # “自滚动区域”：整页滚轮在这些区域内不接管（交给各自滚动）
        for region in (self.var_canvas, self.log_text, self.preview_text,
                       self.tryout_text, self.src_text):
            self._scroll_regions.append(region)
    def _on_body_inner_configure(self, event):
        """内容尺寸变化时更新滚动区域并自适应高度。"""
        self.body_canvas.configure(scrollregion=self.body_canvas.bbox("all"))
        self._fit_body_height()

    def _on_body_canvas_configure(self, event):
        """窗口变化时让内容宽度贴合画布宽度并自适应高度。"""
        self.body_canvas.itemconfigure(self.body_inner_window,
                                       width=event.width)
        self._fit_body_height()

    def _fit_body_height(self):
        """内容高度超过窗口时显示滚动条并滚动；否则撑满窗口并隐藏滚动条。"""
        canvas_h = self.body_canvas.winfo_height()
        req_h = self.body_inner.winfo_reqheight()
        h = max(canvas_h, req_h)
        try:
            cur = int(self.body_canvas.itemcget(self.body_inner_window, "height"))
        except (TypeError, ValueError):
            cur = 0
        if cur != h:
            self.body_canvas.itemconfigure(self.body_inner_window, height=h)
        needs = req_h > canvas_h
        if needs and self.body_vbar.winfo_manager() == "":
            self.body_vbar.grid(row=0, column=1, sticky="ns")
        elif not needs and self.body_vbar.winfo_manager() == "grid":
            self.body_vbar.grid_remove()

    def _section(self, parent, title, accent="red", expand=False):
        """朴素区块：浅色标题栏 + 白底细边框 body；点击标题可折叠/展开。"""
        outer = ttk.Frame(parent)
        outer.pack(fill="both" if expand else "x", expand=expand, pady=(0, 6))
        head = tk.Frame(outer, bg=self.panel_bg, height=30, cursor="hand2",
                        highlightthickness=1, highlightbackground="#c7ccd4")
        head.pack(fill="x")
        head.pack_propagate(False)
        fam = self._base_font()[0]
        tk.Label(head, bg=self.panel_bg, fg="#222222", font=(fam, 12, "bold"),
                 text=title, cursor="hand2").pack(side="left", padx=10)
        arrow = tk.Label(head, bg=self.panel_bg, fg="#555555",
                         font=(fam, 11, "bold"), text="−", width=3,
                         cursor="hand2")
        arrow.pack(side="right", padx=(0, 8))
        body = tk.Frame(outer, bg=self.text_bg, bd=1, relief="solid")
        body.pack(fill="both" if expand else "x", expand=expand)
        inner = tk.Frame(body, bg=self.text_bg)
        inner.pack(fill="both", expand=True, padx=8, pady=8)
        # 记录区块并支持点击标题栏折叠/展开
        self._sections[title] = {"head": head, "body": body, "inner": inner,
                                 "expand": expand, "arrow": arrow,
                                 "collapsed": False}
        for w in [head] + list(head.winfo_children()):
            w.bind("<Button-1>", lambda e, k=title: self._toggle_section(k))
        return inner

    def _toggle_section(self, key):
        """折叠/展开一个区块，并更新箭头。"""
        sec = self._sections.get(key)
        if sec is None:
            return
        sec["collapsed"] = not sec["collapsed"]
        if sec["collapsed"]:
            sec["body"].pack_forget()
            sec["arrow"].configure(text="+")
        else:
            sec["body"].pack(fill="both" if sec["expand"] else "x",
                             expand=sec["expand"])
            sec["arrow"].configure(text="−")

    def _apply_collapsed_state(self):
        """应用上次会话保存的折叠状态。"""
        for key, collapsed in self._initial_collapsed.items():
            sec = self._sections.get(key)
            if sec is None or not collapsed:
                continue
            sec["collapsed"] = True
            sec["body"].pack_forget()
            sec["arrow"].configure(text="+")

    @staticmethod
    def _state_path():
        """状态文件路径：程序（或 exe）所在目录下。"""
        base = os.path.dirname(os.path.abspath(sys.argv[0]))
        return os.path.join(base, "duipai_state.json")

    def _load_state(self):
        """读取状态文件，返回折叠的区块集合与变量行折叠状态。"""
        collapsed = {}
        self._rows_state = {}
        try:
            with open(self._state_path(), "r", encoding="utf-8") as fh:
                data = json.load(fh)
            sections = data.get("sections", {})
            for key, val in sections.items():
                if isinstance(val, bool) and val:
                    collapsed[key] = True
            rows = data.get("rows", {})
            for path, val in rows.items():
                if isinstance(val, bool) and val:
                    self._rows_state[str(path)] = True
        except Exception:
            pass
        return collapsed

    def _apply_row_collapsed_state(self):
        """把保存的变量行折叠状态应用到当前行。"""
        state = getattr(self, "_rows_state", {})
        if not state:
            return
        for path in state:
            row = self._row_by_path(path)
            if row is None:
                continue
            if not getattr(row, "collapsed", False):
                row._toggle_collapse()

    def _row_path(self, row):
        """返回变量行的索引（如 '0'、'2'）。"""
        return str(self.rows.index(row))

    def _row_by_path(self, path):
        """根据索引找到变量行（找不到返回 None）。"""
        try:
            idx = int(path)
        except (TypeError, ValueError):
            return None
        if 0 <= idx < len(self.rows):
            return self.rows[idx]
        return None

    def _save_state(self):
        """把各区块折叠状态与变量行折叠状态写入状态文件。"""
        try:
            sections = {k: bool(v["collapsed"])
                        for k, v in self._sections.items()}
            rows_state = {}
            for row in self._ordered_rows():
                if getattr(row, "collapsed", False):
                    rows_state[self._row_path(row)] = True
            with open(self._state_path(), "w", encoding="utf-8") as fh:
                json.dump({"sections": sections, "rows": rows_state}, fh,
                          ensure_ascii=False, indent=2)
        except Exception:
            pass

    def _build_program_section(self, parent):
        box = self._section(parent, "程序路径", "red")
        box.columnconfigure(2, weight=1)

        def prog_row(row, label_var, cmd_var, mode_var, tag, try_label):
            ttk.Label(box, text=label_var).grid(row=row, column=0,
                                                sticky="w", padx=(0, 4))
            ttk.Combobox(box, textvariable=mode_var,
                         values=["运行命令", "C++ 源码"], width=8,
                         state="readonly").grid(row=row, column=1,
                                                sticky="w", padx=(0, 4),
                                                pady=(4 if row else 0))
            ttk.Entry(box, textvariable=cmd_var).grid(
                row=row, column=2, sticky="ew", padx=2, pady=(4 if row else 0))
            ttk.Button(box, text="浏览", style="Blue.TButton",
                       command=lambda: self._browse(cmd_var, tag)).grid(
                row=row, column=3, padx=(4, 2), pady=(4 if row else 0))
            ttk.Button(box, text=try_label, width=6, style="Blue.TButton",
                       command=lambda t=tag: self._tryout(t)).grid(
                row=row, column=4, padx=(2, 0), pady=(4 if row else 0))
            ttk.Button(box, text="源码", width=5, style="Blue.TButton",
                       command=lambda t=tag: self._view_source(t)).grid(
                row=row, column=5, padx=(2, 0), pady=(4 if row else 0))

        prog_row(0, "正解代码：", self.sol_cmd, self.sol_mode, "sol", "试运行")
        prog_row(1, "暴力代码：", self.brute_cmd, self.brute_mode, "brute", "试运行")

        # 编译设置（C++ 源码模式使用）
        comp = ttk.Frame(box)
        comp.grid(row=2, column=0, columnspan=6, sticky="w", pady=(6, 0))
        ttk.Label(comp, text="编译设置:").pack(side="left")
        ttk.Entry(comp, textvariable=self.compiler, width=10).pack(
            side="left", padx=(4, 8))
        ttk.Label(comp, text="编译参数:").pack(side="left")
        ttk.Entry(comp, textvariable=self.compile_flags, width=22).pack(
            side="left", padx=(4, 8))
        ttk.Label(comp, text="（仅 C++ 源码模式使用）", style="Hint.TLabel").pack(
            side="left")

        ttk.Label(box, text="示例：运行命令填 python3 ./sol.py 或 ./sol；"
                            "C++ 源码模式填 .cpp 路径，开始对拍时自动编译后运行。"
                 ).grid(row=3, column=0, columnspan=6, sticky="w",
                        padx=(0, 4), pady=(4, 0))

    def _build_source_section(self, parent):
        """内嵌源码查看区：查看传入的正解/暴力/外置生成器源码。"""
        box = self._section(parent, "源码查看", "blue")
        head = ttk.Frame(box)
        head.pack(fill="x")
        ttk.Label(head, text="查看程序：").pack(side="left")
        self.src_pick = ttk.Combobox(head, values=["正解", "暴力", "外置生成器"],
                                     width=10, state="readonly")
        self.src_pick.current(0)
        self.src_pick.pack(side="left", padx=(4, 0))
        self.src_pick.bind("<<ComboboxSelected>>", self._on_src_pick)
        ttk.Label(head, text="（也可点击程序行旁的“源码”按钮直接载入）",
                  style="Hint.TLabel").pack(side="left", padx=(8, 0))

        body = ttk.Frame(box)
        body.pack(fill="x", pady=(4, 0))
        mono = ("Menlo", 10) if sys.platform == "darwin" else ("Consolas", 9)
        self.src_gutter = tk.Text(body, width=4, bg="#ececec", fg="#888888",
                                  font=mono, state="disabled", wrap="none",
                                  padx=3, insertofftime=0, relief="flat",
                                  takefocus=0)
        self.src_text = tk.Text(body, height=9, wrap="none", font=mono,
                                bg=self.text_bg, fg=self.text_fg,
                                selectbackground=self.accent,
                                relief="solid", bd=1, state="disabled",
                                insertofftime=0, padx=6, pady=4)
        vbar = ttk.Scrollbar(body, orient="vertical",
                             command=self.src_text.yview)
        self.src_gutter.pack(side="left", fill="y")
        self.src_text.pack(side="left", fill="both", expand=True)
        vbar.pack(side="left", fill="y")

        def _sync_scroll(*args):
            vbar.set(*args)
            self.src_text.yview_moveto(args[0])
            self.src_gutter.yview_moveto(args[0])

        self.src_text.configure(yscrollcommand=_sync_scroll)
        self.src_gutter.configure(yscrollcommand=_sync_scroll)

    def _on_src_pick(self, event=None):
        """下拉切换程序时载入对应源码。"""
        tag_map = {"正解": "sol", "暴力": "brute", "外置生成器": "ext"}
        self._view_source(tag_map.get(self.src_pick.get(), "sol"))

    def _view_source(self, tag):
        """载入对应程序的源码到源码查看区。"""
        tag_map = {"sol": "正解", "brute": "暴力", "ext": "外置生成器"}
        self.src_pick.set(tag_map.get(tag, "正解"))
        if tag == "sol":
            var, mode_var, d = self.sol_cmd, self.sol_mode, self.sol_dir
        elif tag == "brute":
            var, mode_var, d = self.brute_cmd, self.brute_mode, self.brute_dir
        elif tag == "ext":
            var, mode_var, d = self.ext_gen_cmd, self.ext_gen_mode, self.extgen_dir
        else:
            return
        raw = var.get().strip()
        path = ""
        if mode_var.get() == "C++ 源码":
            tokens = self._parse_command(raw)
            path = tokens[0] if tokens else ""
        else:
            # 运行命令：尝试把某个 token 当作文件
            tokens = self._parse_command(raw)
            for tok in tokens:
                p = tok if os.path.isabs(tok) else os.path.normpath(os.path.join(d, tok))
                if os.path.isfile(p):
                    path = p
                    break
        if path and not os.path.isabs(path):
            path = os.path.normpath(os.path.join(d, path))
        if not path:
            self._set_source("未解析到源码文件：请切换到“C++ 源码”模式，"
                             "或直接浏览源码文件。\n")
            return
        if not os.path.isfile(path):
            self._set_source(f"找不到文件：{path}\n")
            return
        try:
            with open(path, "rb") as fh:
                data = fh.read()
        except OSError as e:
            self._set_source(f"读取文件失败：{e}\n")
            return
        if b"\x00" in data[:4096]:
            self._set_source("该文件为二进制文件，无法作为源码显示。\n")
            return
        text = data.decode("utf-8", "replace")
        # 统一行尾，避免 CRLF 与 Tk 换行规则不一致导致行号错位
        text = text.replace("\r\n", "\n").replace("\r", "\n")
        if text.startswith("\ufeff"):
            text = text[1:]
        self._set_source(text, path)

    def _set_source(self, text, path=""):
        """把源码文本与行号写入源码查看区。"""
        lines = text.split("\n")
        if lines and lines[-1] == "":
            lines.pop()
        self.src_text.configure(state="normal")
        self.src_text.delete("1.0", "end")
        self.src_text.insert("1.0", text)
        self.src_text.configure(state="disabled")
        self.src_gutter.configure(state="normal")
        self.src_gutter.delete("1.0", "end")
        self.src_gutter.insert("1.0", "\n".join(map(str, range(1, len(lines) + 1))))
        self.src_gutter.configure(state="disabled")
        self.src_text.yview_moveto(0)
        self.src_gutter.yview_moveto(0)

    def _build_generator_section(self, parent):
        # expand=False：区块按内容自然高度展开，总高度超出窗口时由整窗滚动接管，
        # 让图形化与 DSL 区域都充分展示，不被相互挤压。
        box = self._section(parent, "数据生成器", "blue", expand=False)

        # 生成器模式选择
        mode = ttk.Frame(box)
        mode.pack(fill="x")
        ttk.Radiobutton(mode, text="内置生成器", value="builtin",
                        variable=self.gen_mode,
                        command=self._switch_gen_mode).pack(side="left")
        ttk.Radiobutton(mode, text="外置生成器", value="external",
                        variable=self.gen_mode,
                        command=self._switch_gen_mode).pack(side="left", padx=(14, 0))

        # 面板容器：两个面板重叠放置，用 grid_remove 切换
        holder = ttk.Frame(box)
        holder.pack(fill="both", expand=True, pady=(6, 0))
        holder.rowconfigure(0, weight=1)
        holder.columnconfigure(0, weight=1)
        self.ext_panel = ttk.Frame(holder)
        self.builtin_panel = ttk.Frame(holder)
        self.ext_panel.grid(row=0, column=0, sticky="nsew")
        self.builtin_panel.grid(row=0, column=0, sticky="nsew")

        self._build_external_panel()
        self._build_builtin_panel()
        self._switch_gen_mode()

    def _build_external_panel(self):
        box = self.ext_panel
        ttk.Label(box, text="接入一个生成程序：每次对拍都会运行该程序，"
                            "其标准输出将作为测试数据写入 test.in。",
                  style="Hint.TLabel").pack(fill="x", pady=(0, 4))
        row = ttk.Frame(box)
        row.pack(fill="x")
        row.columnconfigure(2, weight=1)
        ttk.Label(row, text="生成程序：").grid(row=0, column=0,
                                               sticky="w", padx=(0, 4))
        ttk.Combobox(row, textvariable=self.ext_gen_mode,
                     values=["运行命令", "C++ 源码"], width=8,
                     state="readonly").grid(row=0, column=1, padx=(0, 4))
        ttk.Entry(row, textvariable=self.ext_gen_cmd).grid(
            row=0, column=2, sticky="ew")
        ttk.Button(row, text="浏览", style="Blue.TButton",
                   command=lambda: self._browse(self.ext_gen_cmd, "ext")).grid(
            row=0, column=3, padx=(4, 0))
        ttk.Button(row, text="源码", width=5, style="Blue.TButton",
                   command=lambda: self._view_source("ext")).grid(
            row=0, column=4, padx=(4, 0))
        ttk.Label(box,
                  text="说明：种子将作为 --seed 参数传递（仅当填写了随机种子时）；"
                       "C++ 源码模式会在开始对拍时自动编译；"
                       "可点击“预览生成示例”实际运行一次查看输出。",
                  style="Hint.TLabel").pack(fill="x", padx=(0, 4), pady=(4, 0))

    def _build_builtin_panel(self):
        box = self.builtin_panel

        # ---- 图形化面板（上半区）----
        self.gui_panel = ttk.Frame(box)
        self.gui_panel.pack(fill="both", expand=True)

        # 多测模式设置行（面板最开头，流式换行）
        multi_holder = ttk.Frame(self.gui_panel)
        multi_holder.pack(fill="x", pady=(6, 0))
        self.multi_row = tk.Frame(multi_holder, bg=self.panel_bg)
        self.multi_row.pack(fill="x")
        self.multi_row.bind(
            "<Configure>",
            lambda e, c=self.multi_row: self._flow_pack(c))
        self._multi_check = ttk.Checkbutton(self.multi_row, text="多测模式",
                                            variable=self.multi_test,
                                            command=self._on_multi_toggle)
        ttk.Label(self.multi_row, text="重复次数:")
        self.repeat_entry = ttk.Entry(self.multi_row,
                                      textvariable=self.repeat_times,
                                      width=6, state="disabled")
        self.repeat_entry.bind("<KeyRelease>", self._sync_dsl_from_gui)
        ttk.Label(self.multi_row,
                  text="勾选后：首行输出组数 N，随后整块变量独立随机重复 N 次（默认为 1）",
                  style="Hint.TLabel")

        # 顶部按钮行：流式换行，放不下自动折到下一行，不再横向滚动
        top_holder = ttk.Frame(self.gui_panel)
        top_holder.pack(fill="x", pady=(6, 0))
        self.top_flow = tk.Frame(top_holder, bg=self.panel_bg)
        self.top_flow.pack(fill="x")
        self.top_flow.bind(
            "<Configure>",
            lambda e, c=self.top_flow: self._flow_pack(c))
        top = self.top_flow
        ttk.Button(top, text="+ 整数变量", style="Blue.TButton",
                   command=lambda: self._add_var("int"))
        ttk.Button(top, text="+ 浮点数变量", style="Blue.TButton",
                   command=lambda: self._add_var("float"))
        ttk.Button(top, text="+ 数组变量", style="Blue.TButton",
                   command=lambda: self._add_var("array"))
        ttk.Button(top, text="+ 排列变量", style="Blue.TButton",
                   command=lambda: self._add_var("perm"))
        ttk.Button(top, text="+ 树变量", style="Blue.TButton",
                   command=lambda: self._add_var("tree"))
        ttk.Button(top, text="+ 图变量", style="Blue.TButton",
                   command=lambda: self._add_var("graph"))
        ttk.Button(top, text="+ 字符串变量", style="Blue.TButton",
                   command=lambda: self._add_var("string"))
        ttk.Button(top, text="+ 0/1序列", style="Blue.TButton",
                   command=lambda: self._add_var("binseq"))
        ttk.Button(top, text="+ 区间", style="Blue.TButton",
                   command=lambda: self._add_var("intervals"))
        ttk.Button(top, text="+ 点集", style="Blue.TButton",
                   command=lambda: self._add_var("points"))
        ttk.Label(top, text="↑ 图形化配置（可编辑）", style="Hint.TLabel")
        self._flow_pack(self.multi_row)
        self._flow_pack(self.top_flow)

        # 可滚动的变量列表（Canvas + Scrollbar）：高度自适应内容，展开显示不截断
        scroll_holder = ttk.Frame(self.gui_panel)
        scroll_holder.pack(fill="both", expand=True, pady=(6, 0))
        scroll_holder.rowconfigure(0, weight=1)
        scroll_holder.columnconfigure(0, weight=1)

        self.var_canvas = tk.Canvas(scroll_holder, highlightthickness=0,
                                    height=60, yscrollincrement=1)
        vbar = ttk.Scrollbar(scroll_holder, orient="vertical",
                             command=self.var_canvas.yview)
        self.var_canvas.configure(yscrollcommand=vbar.set)

        self.var_inner = ttk.Frame(self.var_canvas)
        self.var_inner_window = self.var_canvas.create_window(
            (0, 0), window=self.var_inner, anchor="nw")

        # 滚轮绑定到画布及其内容，保证列表内任意位置都可滚动
        self._bind_scroll_recursive(self.var_canvas)

        self.var_canvas.grid(row=0, column=0, sticky="nsew")
        vbar.grid(row=0, column=1, sticky="ns")

        # 内容尺寸变化时刷新滚动区域并自适应高度：
        # 宽度 —— 始终贴合画布宽度（行内控件流式换行，不再横向滚动）；
        # 高度 —— 完全贴合内容高度（不设上限），由整窗滚动接管。
        self.var_inner.bind(
            "<Configure>", self._on_var_inner_configure)
        self.var_canvas.bind(
            "<Configure>", self._fit_var_inner_size)

        ttk.Label(self.gui_panel,
                  text="提示：变量从上到下每行各生成一行数据；数组的“长度来源”可引用前面变量的值作为长度。").pack(
            fill="x", padx=(0, 4), pady=(4, 0))

        # ---- DSL 文本面板（下半区，与图形化对照显示）----
        sep = ttk.Separator(box, orient="horizontal")
        sep.pack(fill="x", pady=(8, 0))
        self.dsl_panel = ttk.Frame(box)
        self.dsl_panel.pack(fill="both", expand=True)
        dsl_head = ttk.Frame(self.dsl_panel)
        dsl_head.pack(fill="x", pady=(6, 0))
        ttk.Label(dsl_head, text="DSL 脚本").pack(side="left")
        ttk.Label(dsl_head,
                  text="↓ 与上方图形化同步显示，可直接编辑；点“应用”把脚本转回图形化",
                  style="Hint.TLabel").pack(side="left", padx=(8, 0))
        ttk.Button(dsl_head, text="应用（转为图形化）", style="Blue.TButton",
                   command=self._apply_dsl).pack(side="right")
        ttk.Button(dsl_head, text="查看语法示例", style="TButton",
                   command=self._dsl_help).pack(side="right", padx=(0, 6))
        mono = ("Menlo", 10) if sys.platform == "darwin" else ("Consolas", 9)
        self.dsl_text = scrolledtext.ScrolledText(
            self.dsl_panel, height=2, wrap="char", font=mono,
            bg=self.text_bg, fg=self.text_fg, undo=True,
            selectbackground=self.accent, insertofftime=0)
        self.dsl_text.pack(fill="both", expand=True, pady=(4, 0))
        self._scroll_regions.append(self.dsl_text)

        # 生成样例预览区（只读，点击“预览生成示例”生成，不再弹窗）
        prev_head = ttk.Frame(box)
        prev_head.pack(fill="x", pady=(6, 0))
        ttk.Label(prev_head, text="生成样例预览").pack(side="left")
        ttk.Button(prev_head, text="预览生成示例", style="Blue.TButton",
                   command=self._preview).pack(side="right")
        mono = ("Menlo", 10) if sys.platform == "darwin" else ("Consolas", 9)
        self.preview_text = tk.Text(box, height=2, wrap="char", font=mono,
                                    bg=self.text_bg, fg=self.text_fg,
                                    relief="solid", bd=1, padx=6, pady=4,
                                    state="disabled", insertofftime=0)
        self.preview_text.pack(fill="x", pady=(4, 0))
        self._sync_dsl_from_gui()

    def _on_multi_toggle(self, event=None):
        """多测模式勾选框变化：切换重复次数输入框可用性并同步 DSL。"""
        if getattr(self, "_loading_config", False):
            return
        try:
            state = "normal" if self.multi_test.get() else "disabled"
            self.repeat_entry.configure(state=state)
        except tk.TclError:
            pass
        self._sync_dsl_from_gui()

    def _sync_dsl_from_gui(self, event=None):
        """把当前图形化配置序列化为 DSL 文本，实时同步到 DSL 编辑器。

        仅在用户未聚焦 DSL 编辑器时自动同步，避免覆盖正在编辑的内容。"""
        if getattr(self, "_syncing_dsl", False):
            return
        if self.dsl_text.focus_get() is self.dsl_text:
            return
        import dsl
        if not self.rows:
            text = _DSL_EMPTY_HINT
        else:
            try:
                text = dsl.serialize(self._snapshot_vars())
            except Exception:
                return
        cur = self.dsl_text.get("1.0", "end-1c")
        if cur.strip() == text.strip():
            return
        self._syncing_dsl = True
        try:
            self.dsl_text.configure(state="normal")
            self.dsl_text.delete("1.0", "end")
            self.dsl_text.insert("1.0", text)
            self.dsl_text.edit_reset()
            nlines = max(2, min(len(text.splitlines()), 30))
            self.dsl_text.configure(height=nlines)
        finally:
            self._syncing_dsl = False

    def _dsl_help(self):
        """显示 DSL 语法帮助（弹窗）。"""
        import dsl
        messagebox.showinfo(
            "DSL 语法示例",
            _DSL_HELP,
            parent=self)

    def _apply_dsl(self):
        """把 DSL 文本解析后转为图形化变量列表；失败则提示并保留 DSL 内容。"""
        import dsl
        text = self.dsl_text.get("1.0", "end-1c")
        config, err = dsl.parse(text)
        if err:
            messagebox.showerror("DSL 解析失败", err, parent=self)
            return
        if not (config or {}).get("items"):
            messagebox.showwarning("DSL 为空", "请先填写至少一条语句。", parent=self)
            return
        self._load_config_to_rows(config)
        self._sync_dsl_from_gui()

    def _load_config_to_rows(self, config):
        """用统一配置（dict：{"repeat": ..., "items": [...]}）重建图形化变量列表，
        并把多测模式/重复次数写回勾选框与输入框。"""
        items = (config or {}).get("items", [])
        repeat = (config or {}).get("repeat") or {}
        # 先移除现有所有行
        for row in list(self.rows):
            self.delete_row(row)
        for item in items:
            self._build_row_from_item(self.var_inner, item)
        self._loading_config = True
        try:
            if repeat.get("enabled"):
                self.multi_test.set(True)
                self.repeat_times.set(str(repeat.get("count", "1")))
            else:
                self.multi_test.set(False)
                self.repeat_times.set("1")
        finally:
            self._loading_config = False
        self._on_multi_toggle()
        self._refresh_sources()
        self._update_scrollregion()
        self._sync_dsl_from_gui()
        self._fit_var_inner_size()
        self._apply_row_collapsed_state()

    def _build_row_from_item(self, parent, item):
        """构建单个变量行。"""
        row = VariableRow(parent, item["kind"], self)
        self.rows.append(row)
        row.name = item["name"]
        row.frame.pack(fill="x", padx=2, pady=6)
        self._refresh_sources()
        self._fill_row_from_item(row, item)
        self._bind_scroll_recursive(row.frame)
        self._bind_dsl_sync(row.frame)

    def _fill_row_from_item(self, row, item):
        """把统一配置填回变量行控件。"""
        def set_entry(entry, val):
            entry.delete(0, "end")
            entry.insert(0, str(val))

        def set_source(attr, refs_attr, entries, expr):
            """根据表达式设置来源下拉。expr 形如 int(a,b) / 名字 / 其它表达式。"""
            import re as _re
            m = _re.fullmatch(r"\s*int\(\s*(.*?)\s*,\s*(.*?)\s*\)\s*", expr)
            if m:
                getattr(row, attr + "_var").set("随机范围")
                set_entry(getattr(row, entries[0]), m.group(1))
                set_entry(getattr(row, entries[1]), m.group(2))
                self._apply_source_state_by_row(row, attr, entries)
                return
            # 名字：引用前面变量（下拉会自动列出），先找对应标签
            getattr(row, attr + "_var").set("随机范围")
            for label, prev in getattr(row, refs_attr):
                if (prev.name or "") == expr:
                    getattr(row, attr + "_var").set(label)
                    self._apply_source_state_by_row(row, attr, entries)
                    return
            # 其它表达式：作为只读表达式源加入下拉
            label = f"表达式(DSL)：{expr}"
            self._extra_sources_row(row, attr, label, expr)
            getattr(row, attr + "_var").set(label)
            self._apply_source_state_by_row(row, attr, entries)

        if item["kind"] in ("int", "float"):
            set_entry(row.min_entry, item["min"])
            set_entry(row.max_entry, item["max"])
            if item["kind"] == "float":
                set_entry(row.prec_entry, item.get("prec", "6"))
        elif item["kind"] == "array":
            row.elem_type.set("浮点数" if item["elem_type"] == "浮点数" else "整数")
            row._toggle_elem_type()
            set_entry(row.el_min, item["el_min"])
            set_entry(row.el_max, item["el_max"])
            set_entry(row.prec_entry, item["prec"])
            set_source("rows_source", "_rows_refs",
                       ["rows_min", "rows_max"], item["rows"])
            set_source("len_source", "_len_refs",
                       ["len_min", "len_max"], item["cols"])
        elif item["kind"] == "perm":
            set_source("n_source", "_n_refs", ["n_min", "n_max"], item["n"])
        elif item["kind"] == "string":
            set_source("len_source", "_len_refs",
                       ["len_min", "len_max"], item["cols"])
            set_source("rows_source", "_rows_refs",
                       ["rows_min", "rows_max"], item.get("rows", "1"))
            set_entry(row.charset_entry,
                      item.get("charset") or "abcdefghijklmnopqrstuvwxyz")
        elif item["kind"] == "binseq":
            set_source("n_source", "_n_refs", ["n_min", "n_max"], item["n"])
            set_source("k_source", "_k_refs", ["k_min", "k_max"], item["k"])
        elif item["kind"] == "intervals":
            set_source("n_source", "_n_refs", ["n_min", "n_max"], item["n"])
            set_entry(row.iv_lo, item["lo"])
            set_entry(row.iv_hi, item["hi"])
        elif item["kind"] == "points":
            set_source("n_source", "_n_refs", ["n_min", "n_max"], item["n"])
            set_entry(row.pt_xlo, item["xlo"])
            set_entry(row.pt_xhi, item["xhi"])
            set_entry(row.pt_ylo, item["ylo"])
            set_entry(row.pt_yhi, item["yhi"])
        elif item["kind"] in ("tree", "graph"):
            set_source("n_source", "_n_refs", ["n_min", "n_max"], item["n"])
            w = item.get("w")
            if w is None:
                row.w_mode_var.set("无")
            else:
                row.w_mode_var.set("整数" if w["kind"] == "int" else "浮点")
                set_entry(row.w_min, w["min"])
                set_entry(row.w_max, w["max"])
                set_entry(row.w_prec, w.get("prec", "6"))
            row._toggle_w_mode()
            val = item.get("val")
            if val is None:
                row.v_mode_var.set("无")
            else:
                row.v_mode_var.set("整数" if val["kind"] == "int" else "浮点")
                set_entry(row.v_min, val["min"])
                set_entry(row.v_max, val["max"])
                set_entry(row.v_prec, val.get("prec", "6"))
            row._toggle_v_mode()
            if item["kind"] == "graph":
                gtype = item.get("gtype", "general")
                row.g_type_var.set(
                    {"general": "一般", "bipartite": "二分图", "dag": "DAG",
                     "ring": "环", "base_ring": "基环树"}.get(gtype, "一般"))
                row._toggle_g_type()
                if gtype in ("ring", "base_ring"):
                    if gtype == "base_ring":
                        import re as _re2
                        m2 = _re2.fullmatch(
                            r"\s*int\(\s*(.*?)\s*,\s*(.*?)\s*\)\s*",
                            str(item.get("k", "3")))
                        if m2:
                            set_entry(row.k_min, m2.group(1))
                            set_entry(row.k_max, m2.group(2))
                        else:
                            set_entry(row.k_min, item.get("k", "3"))
                            set_entry(row.k_max, item.get("k", "3"))
                else:
                    row.g_dir_var.set("有向" if item.get("directed") else "无向")
                    row.g_conn_var.set("连通" if item.get("connected") else "任意")
                    set_source("m_source", "_m_refs",
                               ["m_min", "m_max"], item["m"])

    @staticmethod
    def _apply_source_state_by_row(row, attr, entries):
        """按来源下拉当前值启用/禁用对应输入框。"""
        var = getattr(row, attr + "_var")
        state = "normal" if var.get() == "随机范围" else "disabled"
        for name in entries:
            getattr(row, name).configure(state=state)

    def _extra_sources_row(self, row, attr, label, expr):
        """把一个 DSL 表达式注册为变量行的只读来源选项。"""
        extras = row._extra_sources.get(attr, [])
        extras.append((label, expr))
        row._extra_sources[attr] = extras

    def _build_param_section(self, parent):
        box = self._section(parent, "对拍参数", "red")
        box.columnconfigure(1, weight=1)

        ttk.Label(box, text="对拍组数：").grid(row=0, column=0,
                                              sticky="w", padx=(0, 4), pady=2)
        ttk.Entry(box, textvariable=self.rounds, width=10).grid(
            row=0, column=1, sticky="w", pady=2)
        ttk.Label(box, text="-1 表示无限").grid(row=0, column=2,
                                               sticky="w", padx=(6, 0))

        ttk.Label(box, text="超时时间（秒）：").grid(row=1, column=0,
                                                  sticky="w", padx=(0, 4), pady=2)
        ttk.Entry(box, textvariable=self.timeout, width=10).grid(
            row=1, column=1, sticky="w", pady=2)

        ttk.Label(box, text="随机种子：").grid(row=2, column=0,
                                              sticky="w", padx=(0, 4), pady=2)
        ttk.Entry(box, textvariable=self.seed, width=10).grid(
            row=2, column=1, sticky="w", pady=2)
        ttk.Label(box, text="留空则使用系统时间随机").grid(row=2, column=2,
                                                       sticky="w", padx=(6, 0))

        ttk.Checkbutton(box, text="忽略行末空格（同时忽略末尾连续空行）",
                        variable=self.ignore_ws).grid(
            row=3, column=0, columnspan=3, sticky="w", padx=(0, 4), pady=2)

    def _build_control_section(self, parent):
        box = self._section(parent, "控制与日志", "red", expand=True)

        btns = ttk.Frame(box)
        btns.pack(fill="x")
        self.btn_start = ttk.Button(btns, text="开始对拍", style="TButton",
                                    command=self._start)
        self.btn_start.pack(side="left", padx=(0, 6))
        self.btn_stop = ttk.Button(btns, text="停止", style="Red.TButton",
                                   command=self._stop, state="disabled")
        self.btn_stop.pack(side="left")

        font = ("Menlo", 10) if sys.platform == "darwin" else ("Consolas", 9)
        # 试运行输出区（只读，独立于主日志）
        self.tryout_head = ttk.Frame(box)
        self.tryout_head.pack(fill="x", pady=(6, 0))
        ttk.Label(self.tryout_head, text="试运行输出").pack(side="left")
        self.tryout_text = tk.Text(box, height=6, wrap="word", font=font,
                                   bg=self.text_bg, fg=self.text_fg,
                                   selectbackground=self.accent,
                                   relief="solid", bd=1, padx=6, pady=4,
                                   state="disabled", insertofftime=0)
        self.tryout_text.pack(fill="x", pady=(4, 0))

        self.log_text = scrolledtext.ScrolledText(box, height=12, state="disabled",
                                                  wrap="word", font=font,
                                                  bg=self.text_bg, fg=self.text_fg,
                                                  selectbackground=self.accent,
                                                  insertofftime=0)
        self.log_text.pack(fill="both", expand=True, pady=(6, 0))

    # ------------------------------------------------------------------ #
    # 浏览与生成器面板操作
    # ------------------------------------------------------------------ #
    def _browse(self, var, tag):
        """打开文件选择框，把所选路径填入命令输入框并记录目录。"""
        path = filedialog.askopenfilename(title="选择可执行文件或脚本")
        if not path:
            return
        var.set('"' + path + '"' if " " in path else path)
        d = os.path.dirname(path)
        is_cpp = os.path.splitext(path)[1].lower() in (".cpp", ".cc", ".cxx", ".c")
        mode = "C++ 源码" if is_cpp else "运行命令"
        if tag == "sol":
            self.sol_dir = d
            self.sol_mode.set(mode)
        elif tag == "brute":
            self.brute_dir = d
            self.brute_mode.set(mode)
        elif tag == "ext":
            self.extgen_dir = d
            self.ext_gen_mode.set(mode)

    def _switch_gen_mode(self):
        """根据单选按钮切换外置 / 内置生成器面板。"""
        if self.gen_mode.get() == "external":
            self.builtin_panel.grid_remove()
            self.ext_panel.grid()
        else:
            self.ext_panel.grid_remove()
            self.builtin_panel.grid()

    def _add_var(self, kind):
        """添加一个变量条目。kind: int/float/array/perm/tree/graph 等。"""
        row = VariableRow(self.var_inner, kind, self)
        self.rows.append(row)
        row.frame.pack(fill="x", padx=2, pady=6)
        self._bind_scroll_recursive(row.frame)
        self._bind_dsl_sync(row.frame)
        self._refresh_sources()
        self._update_scrollregion()
        self._sync_dsl_from_gui()
        self._fit_var_inner_size()

    def _ordered_rows(self):
        """返回全部变量行（顶层顺序），供引用/序列化使用。"""
        return list(self.rows)

    def move_row(self, row, delta):
        """上下移动变量条目。"""
        lst = self.rows
        i = lst.index(row)
        j = i + delta
        if not (0 <= j < len(lst)):
            return
        lst[i], lst[j] = lst[j], lst[i]
        self._repack_rows()
        self._refresh_sources()
        self._sync_dsl_from_gui()

    def delete_row(self, row):
        """删除变量条目。"""
        lst = self.rows
        if row in lst:
            lst.remove(row)
            row.frame.destroy()
            self._refresh_sources()
            self._update_scrollregion()
            self._sync_dsl_from_gui()
            self._fit_var_inner_size()

    def _repack_rows(self):
        """按 rows 列表顺序重新 pack 全部条目（改变上下顺序）。"""
        for row in self.rows:
            row.frame.pack_forget()
        for row in self.rows:
            row.frame.pack(fill="x", padx=2, pady=6)
        self._update_scrollregion()

    def _on_var_inner_configure(self, event=None):
        """变量列表内容尺寸变化：刷新滚动区域，并延迟重算 canvas 高度（等布局稳定）。"""
        self.var_canvas.configure(scrollregion=self.var_canvas.bbox("all"))
        if getattr(self, "_var_resize_pending", False):
            return
        self._var_resize_pending = True
        self.after_idle(self._var_resize_done)

    def _var_resize_done(self):
        self._var_resize_pending = False
        self._fit_var_inner_size()

    def _flow_pack(self, container, event=None):
        """按容器宽度流式重排其中的单元格（子 Frame）：放不下自动换行。

        用 place 精确摆放（避免 grid 跨行列宽耦合），并在重排后把容器高度
        设为内容总高，保证父级（行 Frame / var_inner）的 reqheight 正确。"""
        if getattr(container, "_flow_busy", False):
            return
        container._flow_busy = True
        try:
            width = container.winfo_width()
            if width <= 1:
                return
            x = y = 0
            line_h = 0
            gap = 6
            for w in container.winfo_children():
                if getattr(w, "_flow_hidden", False):
                    w.place_forget()
                    continue
                rw = w.winfo_reqwidth()
                rh = w.winfo_reqheight()
                if x > 0 and x + rw > width:
                    x = 0
                    y += line_h + gap
                    line_h = 0
                w.place(x=x, y=y, anchor="nw")
                x += rw + gap
                line_h = max(line_h, rh)
            total = y + line_h + gap
            container.configure(height=total)
        except tk.TclError:
            pass
        finally:
            container._flow_busy = False

    def _on_window_configure(self, event=None):
        """根窗口尺寸变化：计算缩放比例并（去抖后）应用全局字号缩放。"""
        if event is not None and event.widget is not self:
            return
        scale = max(0.7, min(1.8, self.winfo_width() / 1180.0))
        if abs(scale - self._scale) < 0.03:
            return
        if self._scale_pending:
            return
        self._scale_pending = True
        self.after_idle(lambda: self._apply_scale(scale))

    def _apply_scale(self, scale):
        """按比例调整全局字号与控件尺寸，然后重新布局。"""
        self._scale_pending = False
        self._scale = scale
        style = ttk.Style(self)
        fam, size = self._base_font()
        new_size = max(6, int(round(size * scale)))
        bold = (fam, max(6, int(round(size * scale))), "bold")
        mono_fam = "Menlo" if sys.platform == "darwin" else "Consolas"
        mono = (mono_fam, max(6, int(round(9 * scale))))
        try:
            style.configure(".", font=(fam, new_size))
            style.configure("TLabelframe.Label", foreground=self.accent,
                            font=bold)
            style.configure("Tag.TLabel", foreground=self.accent, font=bold)
            style.configure("Hint.TLabel", foreground="#6b6b6b")
        except tk.TclError:
            pass
        for w in (self.dsl_text, self.preview_text, self.log_text,
                  self.tryout_text, self.src_text, self.src_gutter):
            try:
                w.configure(font=mono)
            except tk.TclError:
                pass
        try:
            self.status_label.configure(
                font=(fam, max(6, int(round(10 * scale)))))
        except (AttributeError, tk.TclError):
            pass
        # 重新流式排布所有行 + 刷新各画布高度
        self._fit_var_inner_size()
        self._fit_body_height()

    def _fit_var_inner_size(self, event=None):
        """让变量列表自适应内容尺寸：
        - 宽度：始终贴合画布宽度（行内控件流式换行，不再横向滚动）；
        - 高度：完全贴合内容高度（不设上限），由整窗滚动接管。"""
        try:
            req_w = self.var_inner.winfo_reqwidth()
            req_h = self.var_inner.winfo_reqheight()
            canvas_w = self.var_canvas.winfo_width()
            self.var_canvas.itemconfigure(self.var_inner_window,
                                          width=max(req_w, canvas_w))
            self.var_canvas.configure(height=max(req_h, 30))
            self._flow_pack(self.multi_row)
            self._flow_pack(self.top_flow)
            for row in self.rows:
                self._flow_pack(row.body)
        except tk.TclError:
            pass

    def _bind_dsl_sync(self, widget):
        """给变量行内的输入框/下拉绑定“变更后同步 DSL”事件。"""
        for child in widget.winfo_children():
            cls = child.winfo_class()
            if cls == "TEntry":
                child.bind("<KeyRelease>", self._sync_dsl_from_gui)
                child.bind("<<Paste>>", self._sync_dsl_from_gui)
            elif cls == "TCombobox":
                child.bind("<<ComboboxSelected>>", self._sync_dsl_from_gui)
            self._bind_dsl_sync(child)

    def _auto_name(self, row):
        """为变量行生成唯一的 DSL 变量名（v1, v2, ...）。"""
        used = {r.name for r in self._ordered_rows() if r is not row and r.name}
        i = 1
        while f"v{i}" in used:
            i += 1
        return f"v{i}"

    def _refresh_sources(self):
        """重建各行的“来源”下拉选项，只列出它之前的可引用变量。"""
        refable = ("int", "float", "tree", "graph", "perm")
        ordered = self._ordered_rows()
        for i, row in enumerate(ordered):
            if not row.name:
                row.name = self._auto_name(row)
            values = ["随机范围"]
            refs = []
            for prev in ordered[:i]:
                if prev.kind in refable:
                    label = prev.name or self._auto_name(prev)
                    prev.name = label
                    values.append(label)
                    refs.append((label, prev))
            if row.kind == "array":
                cur = row.len_source_var.get()
                extra = row._extra_sources.get("len_source", [])
                row._set_sources("len_source", "_len_refs",
                                 ["len_min", "len_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
                cur = row.rows_source_var.get()
                extra = row._extra_sources.get("rows_source", [])
                row._set_sources("rows_source", "_rows_refs",
                                 ["rows_min", "rows_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
            elif row.kind in ("perm", "tree"):
                cur = row.n_source_var.get()
                extra = row._extra_sources.get("n_source", [])
                row._set_sources("n_source", "_n_refs",
                                 ["n_min", "n_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
            elif row.kind == "string":
                cur = row.len_source_var.get()
                extra = row._extra_sources.get("len_source", [])
                row._set_sources("len_source", "_len_refs",
                                 ["len_min", "len_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
                cur = row.rows_source_var.get()
                extra = row._extra_sources.get("rows_source", [])
                row._set_sources("rows_source", "_rows_refs",
                                 ["rows_min", "rows_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
            elif row.kind == "binseq":
                cur = row.n_source_var.get()
                extra = row._extra_sources.get("n_source", [])
                row._set_sources("n_source", "_n_refs",
                                 ["n_min", "n_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
                cur = row.k_source_var.get()
                extra = row._extra_sources.get("k_source", [])
                row._set_sources("k_source", "_k_refs",
                                 ["k_min", "k_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
            elif row.kind in ("intervals", "points"):
                cur = row.n_source_var.get()
                extra = row._extra_sources.get("n_source", [])
                row._set_sources("n_source", "_n_refs",
                                 ["n_min", "n_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
            elif row.kind == "graph":
                cur = row.n_source_var.get()
                extra = row._extra_sources.get("n_source", [])
                row._set_sources("n_source", "_n_refs",
                                 ["n_min", "n_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)
                cur = row.m_source_var.get()
                extra = row._extra_sources.get("m_source", [])
                row._set_sources("m_source", "_m_refs",
                                 ["m_min", "m_max"], values, refs,
                                 cur if cur in values + [t for t, _ in extra]
                                 else "随机范围", extra)

    def _repack_rows(self):
        """按 rows 列表顺序重新 pack 全部条目（改变上下顺序）。"""
        for row in self.rows:
            row.frame.pack_forget()
        for row in self.rows:
            row.frame.pack(fill="x", padx=2, pady=6)
        self._update_scrollregion()

    def _update_scrollregion(self):
        self.var_canvas.configure(scrollregion=self.var_canvas.bbox("all"))

    @staticmethod
    def _wheel_units(event, px_per_notch):
        """把滚轮事件换算为像素滚动量（yscrollincrement=1，1 单位 = 1px）。
        Windows 一格 delta=±120 对应 px_per_notch 像素；触控板/平滑滚轮为小步长
        （如 delta=±1..±30），按比例换算后累积，实现像素级平滑滚动。"""
        delta = getattr(event, "delta", 0)
        num = getattr(event, "num", 0)
        if delta:
            return -delta / 120.0 * px_per_notch
        if num == 4:
            return -px_per_notch
        if num == 5:
            return px_per_notch
        return 0.0

    def _scroll_units(self, canvas, units):
        """yview_scroll 只接受整数，这里把小数像素累加后再按整像素滚动，并做边界钳制。"""
        acc = getattr(canvas, "_wheel_acc", 0.0) + units
        whole = int(acc)
        frac = acc - whole
        canvas._wheel_acc = frac
        if whole == 0:
            return
        top, bottom = canvas.yview()
        if whole < 0 and top <= 0.0:
            canvas._wheel_acc = 0.0
            return
        if whole > 0 and bottom >= 1.0:
            canvas._wheel_acc = 0.0
            return
        canvas.yview_scroll(whole, "units")

    @staticmethod
    def _region_at_boundary(region, units):
        """判断自滚动区域是否已到 units 方向的边界（滚动链放行依据）。"""
        try:
            top, bottom = region.yview()
        except Exception:
            return True
        if units < 0 and top <= 0.0:
            return True
        if units > 0 and bottom >= 1.0:
            return True
        return False

    def _on_canvas_wheel(self, event):
        """滚动内置生成器变量列表（每格约 120px）。
        到边界时放行给整窗（滚动链）。"""
        units = self._wheel_units(event, 120.0)
        if units == 0:
            return "break"
        canvas = self._wheel_target(event.widget)
        if canvas is None:
            return None
        top, bottom = canvas.yview()
        if (units < 0 and top <= 0.0) or (units > 0 and bottom >= 1.0):
            canvas._wheel_acc = 0.0
            return None   # 已到边界：不 break，让整窗滚轮接管
        self._scroll_units(canvas, units)
        return "break"   # 阻止 bind_all 整页滚轮再次触发

    def _wheel_target(self, widget):
        """滚轮事件所在的自滚动 canvas（变量列表）。"""
        if self._is_descendant(widget, self.var_canvas):
            return self.var_canvas
        return None

    def _bind_scroll_recursive(self, widget):
        """把滚轮事件递归绑定到 widget 及其所有子控件，保证列表内任意位置可滚动。"""
        widget.bind("<MouseWheel>", self._on_canvas_wheel)
        widget.bind("<Button-4>", self._on_canvas_wheel)
        widget.bind("<Button-5>", self._on_canvas_wheel)
        for child in widget.winfo_children():
            self._bind_scroll_recursive(child)

    def _all_scroll_regions(self):
        """全部自滚动区域（基础区域）。"""
        return list(getattr(self, "_scroll_regions", ()))

    def _on_window_wheel(self, event):
        """整窗滚轮：自滚动区域到边界时接管（滚动链），否则交给内层。"""
        units = self._wheel_units(event, 160.0)
        if units == 0:
            return
        for region in self._all_scroll_regions():
            if self._is_descendant(event.widget, region) and \
                    not self._region_at_boundary(region, units):
                return   # 内层还有空间，交给内层
        self._scroll_units(self.body_canvas, units)

    @staticmethod
    def _is_descendant(widget, ancestor):
        """判断 widget 是否为 ancestor 的子孙（含自身）。"""
        while widget is not None:
            if widget is ancestor:
                return True
            try:
                widget = widget.master
            except Exception:
                return False
        return False

    def _current_var_config(self):
        """返回图形化配置（统一表达式配置）。返回 (config, 错误信息或 None)。

        图形化是数据源，DSL 文本是对照镜像；用户编辑 DSL 后需点“应用”转回图形化。"""
        return self._snapshot_vars(), None

    def _preview(self):
        """按当前设置生成一次数据并显示到预览区（不弹窗）。"""
        if self.gen_mode.get() == "external":
            self._preview_external()
            return
        config, cerr = self._current_var_config()
        if cerr:
            self._set_preview("生成失败：" + cerr + "\n")
            return
        if not (config or {}).get("items"):
            self._set_preview("请先添加至少一个变量或填写 DSL。\n")
            return
        seed_str = self.seed.get().strip()
        try:
            random.seed(int(seed_str) if seed_str else None)
        except ValueError:
            self._set_preview("错误：随机种子必须是整数或留空。\n")
            return
        lines, err = self._generate_builtin(config)
        if err:
            self._set_preview("生成失败：" + err + "\n")
            return
        total = len(lines)
        shown = lines[:200]
        text = "".join(f"{i + 1:>4}  {ln}\n" for i, ln in enumerate(shown))
        if total > 200:
            text += f"......（共 {total} 行，此处仅显示前 200 行）\n"
        self._set_preview(text)

    def _preview_external(self):
        """外置生成器预览：实际运行一次生成程序，把 stdout 显示到预览区。"""
        cmd = self.ext_gen_cmd.get().strip()
        if not cmd:
            self._set_preview("外置生成程序命令为空。\n")
            return
        if self.ext_gen_mode.get() == "C++ 源码":
            src = self._parse_command(cmd)[0] if cmd else ""
            build_dir = os.path.join(tempfile.gettempdir(), "duipai_preview_build")
            os.makedirs(build_dir, exist_ok=True)
            compiler = self.compiler.get().strip() or "g++"
            flags = self.compile_flags.get().strip() or "-O2 -std=c++17"
            exe, cerr = self._compile_cpp(src, build_dir, "gen", compiler, flags)
            if cerr:
                self._set_preview("外置生成程序编译失败：\n" + cerr + "\n")
                return
            cmd, base_dir = exe, build_dir
        else:
            base_dir = self.extgen_dir
        seed_str = self.seed.get().strip()
        try:
            seed = int(seed_str) if seed_str else None
        except ValueError:
            self._set_preview("错误：随机种子必须是整数或留空。\n")
            return
        try:
            timeout = float(self.timeout.get().strip())
        except ValueError:
            timeout = 5.0
        if timeout <= 0:
            timeout = 5.0
        out, err = self._generate_external(timeout, cmd, base_dir, seed)
        if err:
            self._set_preview("外置生成程序出错：" + err + "\n")
            return
        lines = out.decode("utf-8", "replace").splitlines()
        shown = lines[:200]
        text = "".join(f"{i + 1:>4}  {ln}\n" for i, ln in enumerate(shown))
        if len(lines) > 200:
            text += f"......（共 {len(lines)} 行，此处仅显示前 200 行）\n"
        self._set_preview(text)

    def _set_preview(self, text):
        """把文本写入只读预览区，并随内容自适应高度（不设上限）。"""
        self.preview_text.configure(state="normal")
        self.preview_text.delete("1.0", "end")
        self.preview_text.insert("1.0", text)
        self.preview_text.configure(state="disabled")
        nlines = max(2, min(len(text.splitlines()), 40))
        self.preview_text.configure(height=nlines)

    # ------------------------------------------------------------------ #
    # 试运行（正解/暴力各按钮）
    # ------------------------------------------------------------------ #
    def _tryout_prog_spec(self, tag):
        """构造单个程序的配置（用于试运行）。"""
        if tag == "sol":
            var, mode_var, d, label = self.sol_cmd, self.sol_mode, self.sol_dir, "正解"
        elif tag == "brute":
            var, mode_var, d, label = self.brute_cmd, self.brute_mode, self.brute_dir, "暴力"
        elif tag == "ext":
            var, mode_var, d, label = self.ext_gen_cmd, self.ext_gen_mode, self.extgen_dir, "外置生成器"
        else:
            return None
        raw = var.get().strip()
        if not raw:
            return None
        if mode_var.get() == "C++ 源码":
            tokens = self._parse_command(raw)
            src = tokens[0] if tokens else ""
            if src and not os.path.isabs(src):
                src = os.path.normpath(os.path.join(d, src))
            return {"mode": "C++ 源码", "cmd": src, "dir": d, "label": label}
        return {"mode": "运行命令", "cmd": raw, "dir": d, "label": label}

    def _tryout(self, tag):
        """试运行：生成一份样例并运行对应程序，结果显示到“试运行输出”区。"""
        if self.running:
            self._set_tryout("对拍进行中，请先停止再试运行。\n")
            return
        if getattr(self, "_trying", False):
            self._set_tryout("已有试运行正在进行，请稍候。\n")
            return
        prog = self._tryout_prog_spec(tag)
        if prog is None:
            self._set_tryout("请先填写该程序的命令或 C++ 源码路径。\n")
            return
        gen_mode = self.gen_mode.get()
        var_config = []
        ext_prog = None
        if gen_mode == "external":
            ext_prog = self._tryout_prog_spec("ext")
            if ext_prog is None:
                self._set_tryout("外置生成程序命令为空。\n")
                return
        else:
            var_config, cerr = self._current_var_config()
            if cerr:
                self._set_tryout("生成配置错误：" + cerr + "\n")
                return
            if not (var_config or {}).get("items"):
                self._set_tryout("内置生成器至少需要添加一个变量或填写 DSL。\n")
                return
        try:
            timeout = float(self.timeout.get().strip())
        except ValueError:
            timeout = 5.0
        if timeout <= 0:
            timeout = 5.0
        seed_str = self.seed.get().strip()
        try:
            seed = int(seed_str) if seed_str else None
        except ValueError:
            self._set_tryout("错误：随机种子必须是整数或留空。\n")
            return
        compiler = self.compiler.get().strip() or "g++"
        flags = self.compile_flags.get().strip() or "-O2 -std=c++17"
        self._trying = True
        self._set_tryout(f"正在试运行“{prog['label']}”……\n")
        threading.Thread(
            target=self._tryout_worker,
            args=(prog, ext_prog, gen_mode, var_config, seed, timeout,
                  compiler, flags), daemon=True).start()

    def _tryout_worker(self, prog, ext_prog, gen_mode, var_config, seed,
                       timeout, compiler, flags):
        """试运行后台线程：生成样例 -> 编译/运行目标程序 -> 回传结果。"""
        workdir = tempfile.mkdtemp(prefix="duipai_try_")
        try:
            ext_ready = None
            if ext_prog is not None:
                self._set_tryout("正在准备外置生成程序（编译）……\n")
                ext_run, ext_run_dir, err = self._prepare_program(
                    ext_prog, workdir, "gen", compiler, flags)
                if err:
                    self._set_tryout("外置生成程序编译失败：\n" + err + "\n")
                    return
                ext_ready = {"mode": ext_prog["mode"], "cmd": ext_run,
                             "dir": ext_run_dir}
            self._set_tryout("正在生成样例……\n")
            random.seed(seed if seed is not None else None)
            input_data, err = self._generate_input(
                timeout, gen_mode, ext_ready, seed, var_config)
            if err:
                self._set_tryout("样例生成失败：" + err + "\n")
                return
            sample_lines = input_data.decode("utf-8", "replace").splitlines()
            sample_show = "\n".join(sample_lines[:5])
            if len(sample_lines) > 5:
                sample_show += "\n..."
            if prog["mode"] == "C++ 源码":
                self._set_tryout("正在编译 C++ 源码……\n")
            run_cmd, run_dir, cerr = self._prepare_program(
                prog, workdir, "try", compiler, flags)
            if cerr:
                self._set_tryout(f"“{prog['label']}”编译失败：\n{cerr}\n")
                return
            self._set_tryout("正在运行……\n")
            t0 = time.perf_counter()
            r = self._run_program(run_cmd, run_dir, input_data, timeout)
            dt = time.perf_counter() - t0
            if r["status"] == "tle":
                self._set_tryout(
                    f"“{prog['label']}” 超时（>{timeout}s）\n")
                return
            if r["status"] != "ok":
                self._set_tryout(
                    f"“{prog['label']}” 运行出错：{r['error']}\n")
                return
            out = r["stdout"].decode("utf-8", "replace")
            errs = r["stderr"].decode("utf-8", "replace").strip()
            txt = (f"【{prog['label']} 试运行】耗时 {dt:.3f}s，返回码 {r['returncode']}\n"
                   f"样例输入（前5行）：\n{sample_show}\n"
                   f"标准输出：\n{out.rstrip()}\n")
            if errs:
                txt += f"错误输出：\n{errs[:500]}\n"
            self._set_tryout(txt)
        except Exception as e:
            import traceback
            self._set_tryout(f"试运行发生内部错误：{e!r}\n"
                             + traceback.format_exc()[-1200:] + "\n")
        finally:
            shutil.rmtree(workdir, ignore_errors=True)
            self._trying = False

    def _set_tryout(self, text):
        """把文本投递到“试运行输出”区（线程安全）。"""
        self.msg_queue.put(("tryout", text))

    # ------------------------------------------------------------------ #
    # 配置快照（在主线程读取所有 Tcl 变量，供后台线程使用）
    # ------------------------------------------------------------------ #
    def _snapshot_vars(self):
        """把当前内置生成器转成统一表达式配置（含名字与多测信息）。

        返回 dict：{"repeat": {...} | None, "items": [...]}。"""
        repeat = None
        if self.multi_test.get():
            repeat = {"enabled": True,
                      "count": self.repeat_times.get().strip() or "1"}
        return {"repeat": repeat, "items": self._snapshot_list(self.rows)}

    def _snapshot_list(self, rows):
        """快照一个变量行列表。"""
        config = []
        for row in rows:
            item = self._snapshot_row(row)
            if item:
                config.append(item)
        return config

    def _snapshot_row(self, row):
        """把单个变量行转成配置项。"""
        if not row.name:
            row.name = self._auto_name(row)
        name = row.name
        ordered = self._ordered_rows()
        idx = ordered.index(row)

        def src_expr(attr, refs_attr, entries):
            return self._src_expr_of(row, attr, refs_attr, entries, ordered, idx)

        def val_expr():
            """读取节点权值描述（v_mode 控件），无则 None。"""
            vm = row.v_mode_var.get()
            if vm == "整数":
                return {"kind": "int", "min": row.v_min.get(),
                        "max": row.v_max.get(), "prec": "6"}
            if vm == "浮点":
                return {"kind": "float", "min": row.v_min.get(),
                        "max": row.v_max.get(),
                        "prec": row.v_prec.get()}
            return None

        if row.kind == "int":
            return {"name": name, "kind": "int",
                    "min": row.min_entry.get(),
                    "max": row.max_entry.get()}
        if row.kind == "float":
            return {"name": name, "kind": "float",
                    "min": row.min_entry.get(),
                    "max": row.max_entry.get(),
                    "prec": row.prec_entry.get()}
        if row.kind == "array":
            return {"name": name, "kind": "array",
                    "elem_type": row.elem_type.get(),
                    "el_min": row.el_min.get(),
                    "el_max": row.el_max.get(),
                    "prec": row.prec_entry.get(),
                    "rows": src_expr("rows_source", "_rows_refs",
                                     ["rows_min", "rows_max"]),
                    "cols": src_expr("len_source", "_len_refs",
                                     ["len_min", "len_max"])}
        if row.kind == "string":
            return {"name": name, "kind": "string",
                    "rows": src_expr("rows_source", "_rows_refs",
                                     ["rows_min", "rows_max"]),
                    "cols": src_expr("len_source", "_len_refs",
                                     ["len_min", "len_max"]),
                    "charset": row.charset_entry.get()}
        if row.kind == "binseq":
            return {"name": name, "kind": "binseq",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"]),
                    "k": src_expr("k_source", "_k_refs",
                                  ["k_min", "k_max"])}
        if row.kind == "intervals":
            return {"name": name, "kind": "intervals",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"]),
                    "lo": row.iv_lo.get(),
                    "hi": row.iv_hi.get()}
        if row.kind == "points":
            return {"name": name, "kind": "points",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"]),
                    "xlo": row.pt_xlo.get(), "xhi": row.pt_xhi.get(),
                    "ylo": row.pt_ylo.get(), "yhi": row.pt_yhi.get()}
        if row.kind == "perm":
            return {"name": name, "kind": "perm",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"])}
        if row.kind == "tree":
            w = None
            wm = row.w_mode_var.get()
            if wm == "整数":
                w = {"kind": "int", "min": row.w_min.get(),
                     "max": row.w_max.get(), "prec": "6"}
            elif wm == "浮点":
                w = {"kind": "float", "min": row.w_min.get(),
                     "max": row.w_max.get(),
                     "prec": row.w_prec.get()}
            return {"name": name, "kind": "tree",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"]),
                    "w": w, "val": val_expr()}
        if row.kind == "graph":
            w = None
            wm = row.w_mode_var.get()
            if wm == "整数":
                w = {"kind": "int", "min": row.w_min.get(),
                     "max": row.w_max.get(), "prec": "6"}
            elif wm == "浮点":
                w = {"kind": "float", "min": row.w_min.get(),
                     "max": row.w_max.get(),
                     "prec": row.w_prec.get()}
            gtype = {"一般": "general", "二分图": "bipartite",
                     "DAG": "dag", "环": "ring",
                     "基环树": "base_ring"}.get(row.g_type_var.get(),
                                               "general")
            item = {"name": name, "kind": "graph",
                    "n": src_expr("n_source", "_n_refs",
                                  ["n_min", "n_max"]),
                    "directed": row.g_dir_var.get() == "有向",
                    "connected": row.g_conn_var.get() == "连通",
                    "gtype": gtype, "w": w, "val": val_expr()}
            if gtype in ("ring", "base_ring"):
                item["m"] = item["n"]
                if gtype == "base_ring":
                    kmn, kmx = row.k_min.get(), row.k_max.get()
                    item["k"] = kmn if kmn == kmx else f"int({kmn}, {kmx})"
            else:
                item["m"] = src_expr("m_source", "_m_refs",
                                     ["m_min", "m_max"])
            return item
        return None

    def _src_expr_of(self, row, attr, refs_attr, entries, ordered, idx):
        """返回某行某个“来源”的表达式字符串；引用检查用 ordered 列表。"""
        src = getattr(row, attr + "_var").get()
        if src == "随机范围":
            lo, hi = [getattr(row, e).get() for e in entries]
            return f"int({lo}, {hi})"
        for label, expr in row._extra_sources.get(attr, []):
            if src == label:
                return expr
        ref_row = row._ref_row(refs_attr, src)
        if ref_row is not None and ordered.index(ref_row) < idx:
            return ref_row.name or self._auto_name(ref_row)
        return src

    # ------------------------------------------------------------------ #
    # 数据生成（后台线程可安全调用）
    # ------------------------------------------------------------------ #
    def _generate_input(self, timeout, gen_mode, ext_prog, seed, var_config):
        """生成一组测试数据，返回 (bytes, 错误信息)。"""
        if gen_mode == "external":
            return self._generate_external(timeout, ext_prog["cmd"],
                                           ext_prog["dir"], seed)
        lines, err = self._generate_builtin(var_config)
        if err:
            return None, err
        return "\n".join(lines).encode("utf-8") + b"\n", None

    def _generate_external(self, timeout, cmd, base_dir, seed):
        """运行外置生成器，捕获其 stdout 作为测试数据。"""
        args = self._parse_command(cmd)
        if not args:
            return None, "外置生成器命令为空"
        if seed is not None:
            args = args + ["--seed", str(seed)]
        cwd = base_dir if os.path.isdir(base_dir) else os.getcwd()
        args = self._resolve_program_path(args, cwd)
        try:
            proc = subprocess.Popen(args, cwd=cwd,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE,
                                    **self._popen_extra())
        except FileNotFoundError:
            return None, f"找不到外置生成器：{args[0]}"
        except Exception as e:
            return None, str(e)
        try:
            stdout, stderr = proc.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            self._kill(proc)
            return None, f"外置生成器超时（>{timeout}s）"
        if proc.returncode != 0:
            msg = stderr.decode("utf-8", "replace").strip()
            return None, f"外置生成器返回码 {proc.returncode}：{msg[:200]}"
        return stdout, None

    @staticmethod
    def _all_pairs(n, directed):
        """列出无自环的全部顶点对（有向含顺序，无向去重）。"""
        pairs = []
        for u in range(1, n + 1):
            for v in range(1, n + 1):
                if u == v:
                    continue
                if not directed and u > v:
                    continue
                pairs.append((u, v))
        return pairs

    def _generate_builtin(self, config):
        """按统一表达式配置生成数据，返回 (行列表, 错误信息)。

        config 为 dict：{"repeat": {...} | None, "items": [...]}。所有数值字段
        为表达式字符串（用 dsl.eval_expr 求值，环境为前面已生成变量的值）。
        勾选多测模式时：首行输出组数 N，随后整块变量独立随机重复 N 次。"""
        import dsl
        items = (config or {}).get("items", [])
        repeat = (config or {}).get("repeat") or {}
        lines = []
        if repeat.get("enabled"):
            count_s = str(repeat.get("count", "1")).strip()
            try:
                count = int(count_s)
            except ValueError:
                return None, f"多测模式重复次数必须是整数：{count_s!r}"
            if count < 1:
                return None, "多测模式重复次数应 >= 1"
            lines.append(str(count))
            for _ in range(count):
                sub_lines = []
                err = self._gen_items(items, sub_lines, {})
                if err:
                    return None, err
                lines.extend(sub_lines)
            return lines, None
        err = self._gen_items(items, lines, {})
        return (None, err) if err else (lines, None)

    def _gen_items(self, config, lines, outer_env):
        """生成一组配置项，写入 lines；返回错误信息或 None。

        env：可引用的变量环境，随生成顺序累积。"""
        import dsl
        env = dict(outer_env)
        for idx, item in enumerate(config, start=1):
            name = item.get("name") or f"v{idx}"
            try:
                def ev(expr, label):
                    try:
                        return dsl.eval_expr(str(expr).strip(), env)
                    except dsl.DslError as e:
                        raise ValueError(f"{label}表达式错误：{e}")

                kind = item["kind"]
                if kind == "int":
                    lo, hi = ev(item["min"], "整数变量范围"), \
                             ev(item["max"], "整数变量范围")
                    lo, hi = int(lo), int(hi)
                    if lo > hi:
                        raise ValueError("整数变量范围最小值不能大于最大值")
                    value = random.randint(lo, hi)
                    lines.append(str(value))
                    env[name] = value
                elif kind == "float":
                    lo = float(ev(item["min"], "浮点数变量范围"))
                    hi = float(ev(item["max"], "浮点数变量范围"))
                    if lo > hi:
                        raise ValueError("浮点数变量范围最小值不能大于最大值")
                    prec = int(ev(item.get("prec", "6"), "浮点精度"))
                    if not (0 <= prec <= 15):
                        raise ValueError("浮点数变量精度应在 0~15 之间")
                    value = random.uniform(lo, hi)
                    lines.append(format_float(value, prec))
                    env[name] = value
                elif kind == "array":
                    if item["elem_type"] == "浮点数":
                        elo = float(ev(item["el_min"], "数组元素范围"))
                        ehi = float(ev(item["el_max"], "数组元素范围"))
                        prec = int(ev(item.get("prec", "6"), "数组元素精度"))
                        if not (0 <= prec <= 15):
                            raise ValueError("数组元素精度应在 0~15 之间")
                        if elo > ehi:
                            raise ValueError("数组元素范围最小值不能大于最大值")

                        def make_elems(n):
                            return [format_float(random.uniform(elo, ehi), prec)
                                    for _ in range(n)]
                    else:
                        elo = int(ev(item["el_min"], "数组元素范围"))
                        ehi = int(ev(item["el_max"], "数组元素范围"))
                        if elo > ehi:
                            raise ValueError("数组元素范围最小值不能大于最大值")

                        def make_elems(n):
                            return [str(random.randint(elo, ehi))
                                    for _ in range(n)]

                    rows = int(ev(item["rows"], "数组行数"))
                    length = int(ev(item["cols"], "数组每行长度"))
                    if rows < 1:
                        raise ValueError("数组行数不能小于 1")
                    if length < 0:
                        raise ValueError("数组每行长度不能为负")
                    lines.extend([" ".join(make_elems(length))
                                  for _ in range(rows)])
                elif kind == "string":
                    rows = int(ev(item.get("rows", "1"), "字符串行数"))
                    length = int(ev(item["cols"], "字符串长度"))
                    charset = item.get("charset")
                    if charset is None or charset == "":
                        raise ValueError("字符串字符集不能为空")
                    if rows < 1:
                        raise ValueError("字符串行数不能小于 1")
                    if length < 0:
                        raise ValueError("字符串长度不能为负")
                    lines.extend(["".join(random.choice(charset)
                                          for _ in range(length))
                                  for _ in range(rows)])
                elif kind == "binseq":
                    n = int(ev(item["n"], "0/1序列长度"))
                    k = int(ev(item["k"], "0/1序列中1的个数"))
                    if n < 0:
                        raise ValueError("0/1序列长度不能为负")
                    if not (0 <= k <= n):
                        raise ValueError("1 的个数 k 应在 0~n 之间")
                    seq = [1] * k + [0] * (n - k)
                    random.shuffle(seq)
                    lines.append(" ".join(map(str, seq)))
                elif kind == "intervals":
                    n = int(ev(item["n"], "区间个数"))
                    lo = int(ev(item["lo"], "区间下界"))
                    hi = int(ev(item["hi"], "区间上界"))
                    if n < 0:
                        raise ValueError("区间个数不能为负")
                    if lo > hi:
                        raise ValueError("区间下界不能大于上界")
                    for _ in range(n):
                        l = random.randint(lo, hi)
                        r = random.randint(l, hi)
                        lines.append(f"{l} {r}")
                elif kind == "points":
                    n = int(ev(item["n"], "点个数"))
                    xlo = int(ev(item["xlo"], "点 x 下界"))
                    xhi = int(ev(item["xhi"], "点 x 上界"))
                    ylo = int(ev(item["ylo"], "点 y 下界"))
                    yhi = int(ev(item["yhi"], "点 y 上界"))
                    if n < 0:
                        raise ValueError("点个数不能为负")
                    if xlo > xhi or ylo > yhi:
                        raise ValueError("点坐标范围无效")
                    for _ in range(n):
                        x = random.randint(xlo, xhi)
                        y = random.randint(ylo, yhi)
                        lines.append(f"{x} {y}")
                elif kind == "perm":
                    n = int(ev(item["n"], "排列长度"))
                    if n < 1:
                        raise ValueError("排列长度 n 应 >= 1")
                    perm = list(range(1, n + 1))
                    random.shuffle(perm)
                    lines.append(" ".join(map(str, perm)))
                    env[name] = n
                elif kind == "tree":
                    n = int(ev(item["n"], "树顶点数"))
                    if n < 1:
                        raise ValueError("树顶点数 n 应 >= 1")
                    w = item.get("w")
                    val = item.get("val")
                    tree_lines = [str(n)]
                    if val:
                        tree_lines.append(self._val_line(n, val, ev))
                    edge_lines = []
                    for i in range(2, n + 1):
                        p = random.randint(1, i - 1)
                        u, v = (i, p) if random.random() < 0.5 else (p, i)
                        edge_lines.append(self._edge_line(u, v, w, ev))
                    random.shuffle(edge_lines)
                    lines.extend(tree_lines + edge_lines)
                    env[name] = n
                elif kind == "graph":
                    gtype = item.get("gtype", "general")
                    w = item.get("w")
                    val = item.get("val")
                    if gtype == "ring":
                        n = int(ev(item["n"], "环顶点数"))
                        if n < 3:
                            raise ValueError("环顶点数 n 应 >= 3")
                        self._graph_ring(n, w, ev, lines, val)
                        env[name] = n
                        continue
                    if gtype == "base_ring":
                        n = int(ev(item["n"], "基环树顶点数"))
                        k = int(ev(item.get("k", "3"), "基环树环大小"))
                        if n < 3:
                            raise ValueError("基环树顶点数 n 应 >= 3")
                        if not (3 <= k <= n):
                            raise ValueError("环大小 k 应在 3~n 之间")
                        self._graph_base_ring(n, k, w, ev, lines, val)
                        env[name] = n
                        continue
                    directed = item["directed"]
                    connected = item["connected"]
                    n = int(ev(item["n"], "图顶点数"))
                    m = int(ev(item["m"], "图边数"))
                    if n < 1:
                        raise ValueError("图顶点数 n 应 >= 1")
                    if m < 0:
                        raise ValueError("图边数 m 不能为负")
                    if gtype == "dag":
                        self._graph_dag(n, m, w, ev, lines, val)
                        env[name] = n
                        continue
                    if gtype == "bipartite":
                        self._graph_bipartite(n, m, w, ev, lines, val)
                        env[name] = n
                        continue
                    possible = n * (n - 1) if directed else n * (n - 1) // 2
                    if m > possible:
                        raise ValueError(
                            f"图边数 m={m} 超过上限 {possible}"
                            f"（{'有向' if directed else '无向'}，n={n}）")
                    if connected and m < n - 1:
                        raise ValueError("连通图要求 m >= n-1")
                    edge_set = set()
                    if connected:
                        for i in range(2, n + 1):
                            p = random.randint(1, i - 1)
                            u, v = (i, p) if random.random() < 0.5 else (p, i)
                            if not directed and u > v:
                                u, v = v, u
                            edge_set.add((u, v))
                    if len(edge_set) < m:
                        if possible <= 100000:
                            all_pairs = self._all_pairs(n, directed)
                            candidates = [e for e in all_pairs if e not in edge_set]
                            edge_set.update(random.sample(candidates, m - len(edge_set)))
                        else:
                            attempts = 0
                            while len(edge_set) < m and attempts < m * 50 + 2000:
                                u = random.randint(1, n)
                                v = random.randint(1, n)
                                if u == v:
                                    attempts += 1
                                    continue
                                if not directed and u > v:
                                    u, v = v, u
                                edge_set.add((u, v))
                                attempts += 1
                            if len(edge_set) < m:
                                raise ValueError("随机补边失败，请检查参数")
                    edges = list(edge_set)
                    random.shuffle(edges)
                    graph_lines = [f"{n} {m}"]
                    if val:
                        graph_lines.append(self._val_line(n, val, ev))
                    graph_lines += [self._edge_line(u, v, w, ev)
                                    for u, v in edges]
                    lines.extend(graph_lines)
                    env[name] = n
            except ValueError as e:
                return f"第 {idx} 个变量设置错误：{e}"
        return None

    def _val_line(self, n, val, ev):
        """生成一行 n 个节点权值。"""
        if val["kind"] in ("整数", "int"):
            lo = int(ev(val["min"], "节点权值范围"))
            hi = int(ev(val["max"], "节点权值范围"))
            if lo > hi:
                raise ValueError("节点权值范围最小值不能大于最大值")
            return " ".join(str(random.randint(lo, hi)) for _ in range(n))
        lo = float(ev(val["min"], "节点权值范围"))
        hi = float(ev(val["max"], "节点权值范围"))
        if lo > hi:
            raise ValueError("节点权值范围最小值不能大于最大值")
        prec = int(ev(val.get("prec", "6"), "节点权值精度"))
        if not (0 <= prec <= 15):
            raise ValueError("节点权值精度应在 0~15 之间")
        return " ".join(format_float(random.uniform(lo, hi), prec)
                        for _ in range(n))

    def _graph_ring(self, n, w, ev, lines, val):
        """生成一个 n 顶点环（n 条边首尾相连）。"""
        if val:
            lines.append(str(n))
            lines.append(self._val_line(n, val, ev))
        else:
            lines.append(str(n))
        edge_lines = []
        for i in range(1, n + 1):
            u, v = i, (i % n) + 1
            if random.random() < 0.5:
                u, v = v, u
            edge_lines.append(self._edge_line(u, v, w, ev))
        lines.extend(edge_lines)

    def _graph_base_ring(self, n, k, w, ev, lines, val):
        """生成一个 n 顶点基环树：k 顶点环 + 其余挂到环上。"""
        if val:
            lines.append(str(n))
            lines.append(self._val_line(n, val, ev))
        else:
            lines.append(str(n))
        edge_set = set()
        for i in range(1, k + 1):
            u, v = i, (i % k) + 1
            if u > v:
                u, v = v, u
            edge_set.add((u, v))
        for i in range(k + 1, n + 1):
            p = random.randint(1, i - 1)
            u, v = i, p
            if u > v:
                u, v = v, u
            edge_set.add((u, v))
        edges = list(edge_set)
        random.shuffle(edges)
        lines.extend(self._edge_line(u, v, w, ev) for u, v in edges)

    def _graph_dag(self, n, m, w, ev, lines, val):
        """生成一个 n 顶点 m 条边的有向无环图（边从小 id 指向大 id）。"""
        if m < 0:
            raise ValueError("图边数 m 不能为负")
        possible = n * (n - 1) // 2
        if m > possible:
            raise ValueError(f"图边数 m={m} 超过上限 {possible}（DAG，n={n}）")
        edge_set = set()
        attempts = 0
        while len(edge_set) < m and attempts < m * 50 + 2000:
            u = random.randint(1, n - 1)
            v = random.randint(u + 1, n)
            edge_set.add((u, v))
            attempts += 1
        if len(edge_set) < m:
            raise ValueError("随机补边失败，请检查参数")
        edges = list(edge_set)
        random.shuffle(edges)
        graph_lines = [f"{n} {m}"]
        if val:
            graph_lines.append(self._val_line(n, val, ev))
        graph_lines += [self._edge_line(u, v, w, ev) for u, v in edges]
        lines.extend(graph_lines)

    def _graph_bipartite(self, n, m, w, ev, lines, val):
        """生成一个 n 顶点 m 条边的二分图（无向，边仅跨两部）。"""
        if m < 0:
            raise ValueError("图边数 m 不能为负")
        left = n // 2
        right = n - left
        if left < 1 or right < 1:
            raise ValueError("二分图 n 过小，无法分两部")
        possible = left * right
        if m > possible:
            raise ValueError(f"图边数 m={m} 超过上限 {possible}（二分图，n={n}）")
        pairs = [(u, left + v) for u in range(1, left + 1)
                 for v in range(1, right + 1)]
        edge_set = set(random.sample(pairs, m))
        edges = list(edge_set)
        random.shuffle(edges)
        graph_lines = [f"{n} {m}"]
        if val:
            graph_lines.append(self._val_line(n, val, ev))
        graph_lines += [self._edge_line(u, v, w, ev) for u, v in edges]
        lines.extend(graph_lines)

    def _edge_line(self, u, v, w, ev):
        """按边权描述生成一条边文本。w: None 或 {kind,min,max,prec}。"""
        if w is None:
            return f"{u} {v}"
        if w["kind"] in ("整数", "int"):
            wlo = int(ev(w["min"], "边权范围"))
            whi = int(ev(w["max"], "边权范围"))
            if wlo > whi:
                raise ValueError("边权范围最小值不能大于最大值")
            return f"{u} {v} {random.randint(wlo, whi)}"
        wlo = float(ev(w["min"], "边权范围"))
        whi = float(ev(w["max"], "边权范围"))
        if wlo > whi:
            raise ValueError("边权范围最小值不能大于最大值")
        wprec = int(ev(w.get("prec", "6"), "边权精度"))
        if not (0 <= wprec <= 15):
            raise ValueError("边权精度应在 0~15 之间")
        return f"{u} {v} {format_float(random.uniform(wlo, whi), wprec)}"

    # ------------------------------------------------------------------ #
    # 运行程序与比较输出
    # ------------------------------------------------------------------ #
    @staticmethod
    def _parse_command(cmd):
        """把命令字符串切分为参数列表，兼容 Windows 路径。"""
        cmd = cmd.strip()
        if not cmd:
            return []
        try:
            if os.name == "nt":
                tokens = shlex.split(cmd, posix=False)
                # posix=False 时引号不会被处理，这里手动去掉包围引号
                return [t[1:-1] if len(t) >= 2 and t[0] == t[-1]
                        and t[0] in "\"'" else t for t in tokens]
            return shlex.split(cmd)
        except ValueError:
            return [cmd]

    @staticmethod
    def _resolve_program_path(args, cwd):
        """把带路径/相对路径的程序名解析为绝对路径；纯命令名走 PATH。"""
        if not args:
            return args
        prog = args[0]
        if not os.path.isabs(prog) and (
                os.sep in prog or "/" in prog or prog.startswith(".")):
            return [os.path.normpath(os.path.join(cwd, prog))] + args[1:]
        return args

    def _run_program(self, cmd_str, base_dir, input_bytes, timeout):
        """运行一个程序，返回状态字典。status: ok / tle / error。"""
        if isinstance(input_bytes, str):
            input_bytes = input_bytes.encode("utf-8")
        args = self._parse_command(cmd_str)
        if not args:
            return {"status": "error", "returncode": None, "stdout": b"",
                    "stderr": b"", "error": "命令为空"}
        cwd = base_dir if base_dir and os.path.isdir(base_dir) else os.getcwd()
        args = self._resolve_program_path(args, cwd)
        try:
            proc = subprocess.Popen(args, cwd=cwd,
                                    stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE,
                                    **self._popen_extra())
        except FileNotFoundError:
            return {"status": "error", "returncode": None, "stdout": b"",
                    "stderr": b"", "error": f"找不到程序或解释器：{args[0]}"}
        except Exception as e:
            return {"status": "error", "returncode": None, "stdout": b"",
                    "stderr": b"", "error": str(e)}
        try:
            stdout, stderr = proc.communicate(input=input_bytes, timeout=timeout)
            return {"status": "ok", "returncode": proc.returncode,
                    "stdout": stdout, "stderr": stderr, "error": ""}
        except subprocess.TimeoutExpired:
            self._kill(proc)
            return {"status": "tle", "returncode": None, "stdout": b"",
                    "stderr": b"", "error": ""}

    @staticmethod
    def _popen_extra():
        """Windows 下禁止子进程创建控制台窗口（防止终端闪现）。"""
        if os.name == "nt":
            return {"creationflags": subprocess.CREATE_NO_WINDOW}
        return {}

    def _compile_cpp(self, source, workdir, name, compiler, flags):
        """编译 C++ 源码，返回 (可执行文件路径, 错误信息)。超时 60 秒。"""
        if not os.path.isfile(source):
            return None, f"找不到源码文件：{source}"
        exe = os.path.join(workdir, name + (".exe" if os.name == "nt" else ""))
        args = [compiler] + self._parse_command(flags) + [source, "-o", exe]
        cwd = os.path.dirname(source) or os.getcwd()
        try:
            proc = subprocess.Popen(args, cwd=cwd,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE,
                                    **self._popen_extra())
        except FileNotFoundError:
            return None, f"找不到编译器：{compiler}"
        except Exception as e:
            return None, str(e)
        try:
            stdout, stderr = proc.communicate(timeout=60)
        except subprocess.TimeoutExpired:
            self._kill(proc)
            return None, "编译超时（>60s）"
        if proc.returncode != 0:
            msg = stderr.decode("utf-8", "replace")
            lines = [ln for ln in msg.splitlines() if ln.strip()]
            return None, (f"编译失败，返回码 {proc.returncode}：\n"
                          + "\n".join(lines[:40]))
        return exe, None

    def _prepare_program(self, prog, workdir, name, compiler, flags):
        """准备一个程序：C++ 源码模式先编译，返回 (运行命令, 运行目录, 错误)。"""
        if prog["mode"] == "C++ 源码":
            exe, err = self._compile_cpp(prog["cmd"], workdir, name,
                                         compiler, flags)
            if err:
                return None, None, err
            self._log(f"[编译] {prog['label']}：{os.path.basename(prog['cmd'])}"
                      f" -> {os.path.basename(exe)} 编译完成")
            return exe, workdir, None
        return prog["cmd"], prog["dir"], None

    @staticmethod
    def _kill(proc):
        """尽量终止超时子进程。"""
        try:
            proc.kill()
        except Exception:
            pass
        try:
            proc.communicate(timeout=2)
        except Exception:
            pass

    @staticmethod
    def _compare(out1, out2, ignore_ws):
        """比较两份输出，可忽略行末空格与末尾空行。"""
        if ignore_ws:
            return Application._normalize(out1) == Application._normalize(out2)
        return out1 == out2

    @staticmethod
    def _normalize(text):
        """每行 rstrip，并去除末尾连续空行。"""
        lines = text.splitlines()
        while lines and not lines[-1].strip():
            lines.pop()
        return [ln.rstrip() for ln in lines]

    # ------------------------------------------------------------------ #
    # 对拍控制（后台线程）
    # ------------------------------------------------------------------ #
    def _start(self):
        """开始对拍：在主线程快照配置并启动后台线程。"""
        if self.running:
            return

        def prog_spec(cmd_var, mode_var, base_dir, label):
            """构造程序配置：运行命令保持原样，C++ 源码解析出源码路径（相对路径转绝对）。"""
            raw = cmd_var.get().strip()
            if not raw:
                return None
            if mode_var.get() == "C++ 源码":
                tokens = self._parse_command(raw)
                src = tokens[0] if tokens else ""
                if src and not os.path.isabs(src):
                    src = os.path.normpath(os.path.join(base_dir, src))
                return {"mode": "C++ 源码", "cmd": src, "dir": base_dir,
                        "label": label, "display": raw}
            return {"mode": "运行命令", "cmd": raw, "dir": base_dir,
                    "label": label, "display": raw}

        sol_prog = prog_spec(self.sol_cmd, self.sol_mode, self.sol_dir, "正解")
        brute_prog = prog_spec(self.brute_cmd, self.brute_mode, self.brute_dir, "暴力")
        if sol_prog is None:
            self._log("错误：请填写正解代码的运行命令或 C++ 源码路径。")
            return
        if brute_prog is None:
            self._log("错误：请填写暴力代码的运行命令或 C++ 源码路径。")
            return
        try:
            total = int(self.rounds.get().strip())
        except ValueError:
            self._log("错误：对拍组数必须是整数（-1 表示无限）。")
            return
        try:
            timeout = float(self.timeout.get().strip())
        except ValueError:
            self._log("错误：超时时间必须是数字。")
            return
        if timeout <= 0:
            self._log("错误：超时时间必须大于 0。")
            return
        seed_str = self.seed.get().strip()
        seed = None
        if seed_str:
            try:
                seed = int(seed_str)
            except ValueError:
                self._log("错误：随机种子必须是整数或留空。")
                return
        gen_mode = self.gen_mode.get()
        ignore_ws = self.ignore_ws.get()
        compiler = self.compiler.get().strip() or "g++"
        flags = self.compile_flags.get().strip() or "-O2 -std=c++17"
        ext_prog = None
        var_config = []
        if gen_mode == "external":
            ext_prog = prog_spec(self.ext_gen_cmd, self.ext_gen_mode,
                                 self.extgen_dir, "外置生成器")
            if ext_prog is None:
                self._log("错误：外置生成器命令为空。")
                return
        else:
            var_config, cerr = self._current_var_config()
            if cerr:
                self._log("错误：生成配置错误：" + cerr)
                return
            if not (var_config or {}).get("items"):
                self._log("错误：内置生成器至少需要添加一个变量或填写 DSL。")
                return

        self.stop_event.clear()
        self.running = True
        self.btn_start.config(state="disabled")
        self.btn_stop.config(state="normal")

        mode = "外置生成器" if gen_mode == "external" else "内置生成器"
        self._log("=" * 60)
        self._log(f"开始对拍（{mode}）：共 {total if total != -1 else '无限'} 组，"
                  f"超时 {timeout}s" + (f"，种子 {seed}" if seed is not None else ""))
        self._log(f"正解：{sol_prog['display']}")
        self._log(f"暴力：{brute_prog['display']}")

        self.worker = threading.Thread(
            target=self._run_loop,
            args=(sol_prog, brute_prog, total, timeout, seed, gen_mode,
                  ignore_ws, ext_prog, var_config, compiler, flags),
            daemon=True)
        self.worker.start()

    def _stop(self):
        """请求停止对拍。"""
        self.stop_event.set()
        self._log("收到停止请求，正在停止……")

    def _run_loop(self, sol_prog, brute_prog, total, timeout, seed, gen_mode,
                  ignore_ws, ext_prog, var_config, compiler, flags):
        """对拍主循环，运行于独立线程中（不访问任何 Tcl 接口）。"""
        workdir = tempfile.mkdtemp(prefix="duipai_")
        tested = 0
        stats = {"pass": 0, "wa": 0, "tle": 0, "re": 0, "error": 0}
        reason = ""
        try:
            # 先编译所有 C++ 源码程序
            sol_run, sol_run_dir, err = self._prepare_program(
                sol_prog, workdir, "sol", compiler, flags)
            if err:
                stats["error"] += 1
                self._log(f"[编译] 正解 失败：{err}")
                reason = "编译失败"
                return
            brute_run, brute_run_dir, err = self._prepare_program(
                brute_prog, workdir, "brute", compiler, flags)
            if err:
                stats["error"] += 1
                self._log(f"[编译] 暴力 失败：{err}")
                reason = "编译失败"
                return
            ext_ready = None
            if ext_prog is not None:
                ext_run, ext_run_dir, err = self._prepare_program(
                    ext_prog, workdir, "gen", compiler, flags)
                if err:
                    stats["error"] += 1
                    self._log(f"[编译] 外置生成器 失败：{err}")
                    reason = "编译失败"
                    return
                ext_ready = {"mode": ext_prog["mode"], "cmd": ext_run,
                             "dir": ext_run_dir}
            random.seed(seed if seed is not None else None)
            while not self.stop_event.is_set() and (total == -1 or tested < total):
                tested += 1
                n = tested
                # 1) 生成输入数据
                input_data, err = self._generate_input(
                    timeout, gen_mode, ext_ready, seed, var_config)
                if err:
                    stats["error"] += 1
                    self._log(f"第 {n} 组：数据生成失败：{err}")
                    reason = "因出错中止"
                    break
                in_path = os.path.join(workdir, "test.in")
                try:
                    with open(in_path, "wb") as fh:
                        fh.write(input_data)
                except OSError as e:
                    self._log(f"第 {n} 组：写入 test.in 失败：{e}")
                    stats["error"] += 1
                    reason = "因出错中止"
                    break

                # 2) 运行正解
                r1 = self._run_program(sol_run, sol_run_dir,
                                       input_data, timeout)
                if r1["status"] != "ok":
                    self._handle_abnormal(r1, "正解", n, workdir, stats, timeout)
                    reason = "因出错中止"
                    break
                if r1["returncode"] != 0:
                    stats["re"] += 1
                    self._log(f"第 {n} 组：正解 返回码 {r1['returncode']}（RE）")
                    self._save_fail(workdir)
                    reason = "因出错中止"
                    break

                # 3) 运行暴力
                r2 = self._run_program(brute_run, brute_run_dir,
                                       input_data, timeout)
                if r2["status"] != "ok":
                    self._handle_abnormal(r2, "暴力", n, workdir, stats, timeout)
                    reason = "因出错中止"
                    break
                if r2["returncode"] != 0:
                    stats["re"] += 1
                    self._log(f"第 {n} 组：暴力 返回码 {r2['returncode']}（RE）")
                    self._save_fail(workdir)
                    reason = "因出错中止"
                    break

                # 4) 比较输出
                out1 = r1["stdout"].decode("utf-8", "replace")
                out2 = r2["stdout"].decode("utf-8", "replace")
                try:
                    with open(os.path.join(workdir, "prog.out"), "w",
                              encoding="utf-8") as fh:
                        fh.write(out1)
                    with open(os.path.join(workdir, "std.out"), "w",
                              encoding="utf-8") as fh:
                        fh.write(out2)
                except OSError:
                    pass

                if self._compare(out1, out2, ignore_ws):
                    stats["pass"] += 1
                    self._log(f"第 {n} 组：PASS")
                else:
                    stats["wa"] += 1
                    self._log(f"第 {n} 组：答案不一致（WA）")
                    self._log(f"    正解输出：{out1[:200]!r}")
                    self._log(f"    暴力输出：{out2[:200]!r}")
                    self._save_fail(workdir)
                    reason = "因出错中止"
                    break

                self._update_status(tested, total)
        except Exception as e:
            self._log(f"对拍线程发生未预期错误：{e!r}")
            stats["error"] += 1
            reason = "因出错中止"
        finally:
            shutil.rmtree(workdir, ignore_errors=True)
            if not reason:
                reason = "手动停止" if self.stop_event.is_set() else "正常完成"
            self.stats = stats
            self.tested = tested
            self.finish_reason = reason
            self._update_status(tested, total)
            self.msg_queue.put(("finish", None))

    def _handle_abnormal(self, r, which, n, workdir, stats, timeout):
        """处理 TLE / 运行出错情况并保存现场。"""
        if r["status"] == "tle":
            stats["tle"] += 1
            self._log(f"第 {n} 组：{which} 超时（TLE，>{timeout}s）")
        else:
            stats["error"] += 1
            self._log(f"第 {n} 组：{which} 运行出错：{r['error']}")
        self._save_fail(workdir)

    def _save_fail(self, workdir):
        """把失败现场（test.in / prog.out / std.out）复制到 ./fail/ 目录。"""
        fail_dir = os.path.join(os.getcwd(), "fail")
        try:
            os.makedirs(fail_dir, exist_ok=True)
        except OSError as e:
            self._log(f"    无法创建 fail 目录：{e}")
            return
        idx = self._next_fail_index(fail_dir)
        mapping = [("test.in", f"fail_{idx}.in"),
                   ("prog.out", f"fail_{idx}_prog.out"),
                   ("std.out", f"fail_{idx}_std.out")]
        saved = []
        for src, dst in mapping:
            sp = os.path.join(workdir, src)
            if os.path.exists(sp):
                try:
                    shutil.copy2(sp, os.path.join(fail_dir, dst))
                    saved.append(dst)
                except OSError:
                    pass
        self._log(f"    现场已保存到 {os.path.join(fail_dir, '')}"
                  f"（{', '.join(saved)}）")

    @staticmethod
    def _next_fail_index(fail_dir):
        """返回下一个可用的失败编号（避免覆盖）。"""
        max_idx = 0
        try:
            for name in os.listdir(fail_dir):
                m = re.match(r"^fail_(\d+)\.in$", name)
                if m:
                    max_idx = max(max_idx, int(m.group(1)))
        except OSError:
            pass
        return max_idx + 1

    # ------------------------------------------------------------------ #
    # 主线程消息处理（队列轮询）
    # ------------------------------------------------------------------ #
    def _log(self, msg):
        """后台线程安全地投递一条日志消息。"""
        self.msg_queue.put(("log", msg))

    def _update_status(self, tested, total):
        """后台线程投递状态栏进度文本。"""
        text = f"已测试：{tested}"
        if total != -1:
            text += f"/{total}"
        self.msg_queue.put(("status", text))

    def _poll_queue(self):
        """主线程轮询消息队列并执行对应 UI 更新。"""
        try:
            while True:
                kind, payload = self.msg_queue.get_nowait()
                if kind == "log":
                    self._insert_log(payload)
                elif kind == "status":
                    self.status_var.set(payload)
                elif kind == "tryout":
                    self._insert_tryout(payload)
                elif kind == "finish":
                    self._do_finish()
        except queue.Empty:
            pass
        self._poll_id = self.after(100, self._poll_queue)

    def _insert_tryout(self, text):
        """把文本写入“试运行输出”区并滚入视野（只能在主线程调用）。"""
        self.tryout_text.configure(state="normal")
        self.tryout_text.delete("1.0", "end")
        self.tryout_text.insert("1.0", text)
        self.tryout_text.configure(state="disabled")
        try:
            y = self.tryout_head.winfo_rooty() - self.body_canvas.winfo_rooty()
            self.body_canvas.yview_moveto(
                max(0.0, y / max(1, self.body_canvas.winfo_height())))
        except Exception:
            pass

    def _insert_log(self, msg):
        """向日志框追加一行（只能在主线程调用）。"""
        self.log_text.configure(state="normal")
        self.log_text.insert("end", time.strftime("[%H:%M:%S] ") + msg + "\n")
        self.log_text.see("end")
        self.log_text.configure(state="disabled")

    def _do_finish(self):
        """对拍结束后恢复按钮状态并输出统计（主线程）。"""
        self.running = False
        self.btn_start.config(state="normal")
        self.btn_stop.config(state="disabled")
        s = self.stats
        errs = s["re"] + s["error"]
        self._insert_log("-" * 60)
        self._insert_log(f"对拍结束（{self.finish_reason}）：共测试 {self.tested} 组")
        self._insert_log(
            f"  通过：{s['pass']}    不一致(WA)：{s['wa']}    "
            f"超时(TLE)：{s['tle']}    运行错误(RE/Error)：{errs}")

    # ------------------------------------------------------------------ #
    # 窗口关闭
    # ------------------------------------------------------------------ #
    def _on_close(self):
        """关闭窗口：先请求停止对拍、保存状态，再等待线程结束。"""
        self.stop_event.set()
        if self.worker and self.worker.is_alive():
            self.worker.join(timeout=15)
        if self._poll_id is not None:
            try:
                self.after_cancel(self._poll_id)
            except Exception:
                pass
            self._poll_id = None
        self._save_state()
        self.destroy()


def main():
    """程序入口：创建主窗口并启动事件循环。"""
    app = Application()
    app.mainloop()


if __name__ == "__main__":
    main()
