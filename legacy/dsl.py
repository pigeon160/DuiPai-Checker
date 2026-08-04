#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
对拍输入 DSL —— 指令式语言，描述内置生成器的输入格式。

语法（一条语句一行，等号赋值，顺序执行，可引用前面定义的名字）：
    注释：以 # 开头（顶部“# 多测模式：重复 N 次”表示多组数据）
    表达式：常数（5、1.5）、变量名（n）、算术（2*n、n+1、n//2）、
            范围随机调用 int(a,b) / float(a,b,prec)，
            支持括号、一元负号；运算符 + - * / // % **
    命令：
        n   = int(1, 100)              # 整数变量，随机 [1,100]，输出一行
        x   = float(0, 1)              # 浮点变量（默认6位精度）
        x4  = float(0, 1, 4)           # 浮点变量，指定精度4
        a   = ints(n, 1, 100)          # 数组：一行 n 个整数，范围 [1,100]
        b   = floats(3, 0, 1)          # 数组：一行 3 个浮点
        c   = ints(int(1,5), 1, 9)     # 数组：个数随机 1~5
        d   = ints(2*n, 0, 1)          # 数组：个数 = 表达式
        M   = matrix(3, n, 0, 1)       # 数组：3 行 × n 列整数（多行）
        F   = matf(int(1,5), n, 0,1,4) # 数组：随机行数 × n 列浮点
        p   = perm(n)                  # 排列：一行 1..n 随机排列
        t   = tree(n)                  # 树：首行 n + n-1 条边，无边权
        t   = tree(n, int(1, 100))     # 树 + 整数边权
        t   = tree(n, float(0, 1, 4))  # 树 + 浮点边权，精度4
    扩展（GUI 往返需要，与“图变量”对应）：
        g   = graph(n, m, 1, 1, int(1,10))  # 有向/连通可用 1/0，边权可选

多组数据：顶部写“# 多测模式：重复 N 次”，生成时首行输出 N，
随后整块语句独立随机重复 N 次（每轮可引用仅当轮前面的值）。

引用规则：只能引用前面定义的名字；数组不可被引用；perm/tree/graph 引用其规模值。
"""

import random
import re

__all__ = ["parse", "serialize", "eval_expr", "DslError"]


class DslError(Exception):
    """DSL 解析或求值错误。"""


# --------------------------------------------------------------------------- #
# 受限表达式求值器（自制 tokenizer + 递归下降，不使用 eval）
# --------------------------------------------------------------------------- #

_TOKEN_RE = re.compile(r"""
    \s*(?:
        (?P<num>\d+\.\d+|\d+|\.\d+)
      | (?P<name>[A-Za-z_][A-Za-z0-9_]*)
      | (?P<str>"[^"\n]*"|'[^'\n]*')
      | (?P<op>\*\*|[+*/%()\-=:]|//)
      | (?P<comma>,)
      | (?P<bad>.)
    )
""", re.VERBOSE)


def tokenize(src):
    """把表达式切成 token 列表。"""
    toks = []
    pos = 0
    while pos < len(src):
        m = _TOKEN_RE.match(src, pos)
        if not m or (m.end() == pos):
            break
        pos = m.end()
        if m.group("num") is not None:
            raw = m.group("num")
            toks.append(("num", float(raw) if ("." in raw) else int(raw)))
        elif m.group("str") is not None:
            raw = m.group("str")
            toks.append(("str", raw[1:-1]))
        elif m.group("name") is not None:
            toks.append(("name", m.group("name")))
        elif m.group("op") is not None:
            toks.append(("op", m.group("op")))
        elif m.group("comma") is not None:
            toks.append(("comma", ","))
        else:
            raise DslError(f"无法识别的字符：{m.group('bad')!r}")
    return toks


class _ExprParser:
    """递归下降表达式解析器，产出小 AST。"""

    def __init__(self, toks):
        self.toks = toks
        self.pos = 0

    def peek(self):
        return self.toks[self.pos] if self.pos < len(self.toks) else None

    def next(self):
        tok = self.peek()
        if tok is not None:
            self.pos += 1
        return tok

    def expect_op(self, op):
        tok = self.peek()
        if tok and ((tok[0] == "op" and tok[1] == op)
                    or (tok[0] == "comma" and op == ",")):
            self.pos += 1
            return True
        return False

    def parse(self):
        if not self.toks:
            raise DslError("空表达式")
        node = self.parse_expr()
        if self.pos != len(self.toks):
            raise DslError("表达式末尾有多余内容")
        return node

    def parse_expr(self):
        node = self.parse_term()
        while True:
            tok = self.peek()
            if tok and tok[0] == "op" and tok[1] in ("+", "-"):
                self.next()
                right = self.parse_term()
                node = ("bin", tok[1], node, right)
            else:
                break
        return node

    def parse_term(self):
        node = self.parse_factor()
        while True:
            tok = self.peek()
            if tok and tok[0] == "op" and tok[1] in ("*", "/", "//", "%"):
                self.next()
                right = self.parse_factor()
                node = ("bin", tok[1], node, right)
            else:
                break
        return node

    def parse_factor(self):
        tok = self.peek()
        if tok and tok[0] == "op" and tok[1] == "-":
            self.next()
            return ("neg", self.parse_factor())
        if tok and tok[0] == "op" and tok[1] == "+":
            self.next()
            return self.parse_factor()
        return self.parse_atom()

    def parse_atom(self):
        tok = self.peek()
        if tok is None:
            raise DslError("表达式意外结束")
        if tok[0] == "num":
            self.next()
            return ("num", tok[1])
        if tok[0] == "name":
            self.next()
            if self.peek() and self.peek()[0] == "op" and self.peek()[1] == "(":
                self.next()
                args = []
                if not (self.peek() and self.peek()[0] == "op"
                        and self.peek()[1] == ")"):
                    while True:
                        args.append(self.parse_expr())
                        if self.expect_op(","):
                            continue
                        break
                if not self.expect_op(")"):
                    raise DslError("缺少右括号")
                return ("call", tok[1], args)
            return ("name", tok[1])
        if tok[0] == "op" and tok[1] == "(":
            self.next()
            node = self.parse_expr()
            if not self.expect_op(")"):
                raise DslError("缺少右括号")
            return node
        raise DslError("表达式位置出现非法 token")


def _eval_node(node, env):
    """求值小 AST。node 形式：("num",v) / ("name",s) / ("bin",op,l,r) /
    ("neg",n) / ("call",name,args)。"""
    kind = node[0]
    if kind == "num":
        return node[1]
    if kind == "name":
        name = node[1]
        if name not in env:
            raise DslError(f"引用了未定义的变量：{name}")
        return env[name]
    if kind == "neg":
        return -_eval_node(node[1], env)
    if kind == "bin":
        op = node[1]
        if op == "**":
            return _eval_node(node[2], env) ** _eval_node(node[3], env)
        a = _eval_node(node[2], env)
        b = _eval_node(node[3], env)
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if op == "/":
            return a / b
        if op == "//":
            return int(a // b)
        if op == "%":
            return a % b
        raise DslError(f"未知运算符：{op}")
    if kind == "call":
        fname, args = node[1], node[2]
        if fname == "int":
            if len(args) != 2:
                raise DslError("int(lo,hi) 需要两个参数")
            lo = _eval_node(args[0], env)
            hi = _eval_node(args[1], env)
            lo, hi = int(lo), int(hi)
            if lo > hi:
                raise DslError(f"int 范围 {lo} > {hi}")
            return random.randint(lo, hi)
        if fname == "float":
            if len(args) not in (2, 3):
                raise DslError("float(lo,hi[,prec]) 需要 2 或 3 个参数")
            lo = float(_eval_node(args[0], env))
            hi = float(_eval_node(args[1], env))
            if len(args) == 3:
                _eval_node(args[2], env)   # 校验 prec 表达式合法性
            if lo > hi:
                raise DslError(f"float 范围 {lo} > {hi}")
            return random.uniform(lo, hi)
        raise DslError(f"未知函数调用：{fname}")
    raise DslError("未知 AST 节点")


def eval_expr(src, env):
    """解析并求值一个表达式字符串。env 为 {变量名: 值}。"""
    try:
        node = _ExprParser(tokenize(src)).parse()
    except DslError as e:
        raise DslError(f"表达式 {src!r} 语法错误：{e}")
    return _eval_node(node, env)


# --------------------------------------------------------------------------- #
# 解析 DSL 文本 -> 统一配置列表
# --------------------------------------------------------------------------- #

_KNOWN_COMMANDS = {"int", "float", "ints", "floats", "matrix", "matf",
                   "perm", "tree", "graph", "str", "strs", "binseq",
                   "intervals", "points", "ring", "base_ring"}
_NAME_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _arg_span(toks, start):
    """从 start 读取一个表达式，返回 (结束下标, 表达式文本切片 token)。"""
    parser = _ExprParser(toks[start:])
    parser.parse()
    return start + parser.pos, toks[start:start + parser.pos]


def _find_comma(toks, i):
    for j in range(i, len(toks)):
        if toks[j] == ("comma", ","):
            return j
    return -1


def _split_args(toks):
    """把括号内的参数按顶层逗号切成若干子 token 列表。"""
    args = []
    depth = 0
    cur = []
    for tok in toks:
        if tok[0] == "op" and tok[1] == "(":
            depth += 1
        elif tok[0] == "op" and tok[1] == ")":
            depth -= 1
        if tok[0] == "comma" and depth == 0:
            args.append(cur)
            cur = []
        else:
            cur.append(tok)
    args.append(cur)
    return args


def _expr_text(toks):
    return "".join(_tok_text(t) for t in toks).strip()


def _tok_text(tok):
    kind, val = tok
    if kind == "num":
        return str(val)
    if kind == "name":
        return val
    if kind == "str":
        return f'"{val}"'
    if kind == "op":
        if val in ("*", "//", "="):
            return " " + val + " "
        return val
    if kind == "comma":
        return ", "
    return val


def _is_single_name(toks):
    return len(toks) == 1 and toks[0][0] == "name"


def _is_range_call(toks, fname):
    """判断表达式是否为 int(a,b) / float(a,b[,prec]) 形式，返回参数或 None。"""
    if not toks or toks[0][0] != "name" or toks[0][1] != fname:
        return None
    if len(toks) < 3 or not (toks[1][0] == "op" and toks[1][1] == "("):
        return None
    if not (toks[-1][0] == "op" and toks[-1][1] == ")"):
        return None
    inner = _split_args(toks[2:-1])
    if not inner or not inner[-1]:
        return None
    if fname == "int" and len(inner) != 2:
        return None
    if fname == "float" and len(inner) not in (2, 3):
        return None
    return [_expr_text(x) for x in inner]


def _weight_to_item(toks):
    """把 tree/graph 的边权参数 token 转成 {kind,min,max,prec} 或 None。"""
    if not toks:
        return None
    if len(toks) == 1 and toks[0][0] == "name" and toks[0][1] == "none":
        return None
    r = _is_range_call(toks, "int")
    if r:
        return {"kind": "int", "min": r[0], "max": r[1], "prec": "6"}
    r = _is_range_call(toks, "float")
    if r:
        prec = r[2] if len(r) == 3 else "6"
        return {"kind": "float", "min": r[0], "max": r[1], "prec": prec}
    raise DslError("边权参数必须是 int(a,b) 或 float(a,b[,prec])")


def _split_kw_args(toks):
    """把命令括号内的 token 拆成 (位置参数列表, 关键字参数字典)。

    每个参数为一个子 token 列表；含顶层 '=' 的视为关键字参数 k=v。"""
    raw = _split_args(toks)
    if raw and not raw[-1]:
        raw.pop()
    pos = []
    kw = {}
    for arg in raw:
        eq_idx = None
        depth = 0
        for i, tok in enumerate(arg):
            if tok[0] == "op" and tok[1] == "(":
                depth += 1
            elif tok[0] == "op" and tok[1] == ")":
                depth -= 1
            elif tok[0] == "op" and tok[1] == "=" and depth == 0:
                eq_idx = i
                break
        if eq_idx is not None and eq_idx > 0:
            kname = _expr_text(arg[:eq_idx]).strip()
            if not _NAME_RE.match(kname):
                raise DslError(f"非法关键字参数名：{kname}")
            if kname in kw:
                raise DslError(f"关键字参数重复：{kname}")
            if kname in ("type",):
                v = _expr_text(arg[eq_idx + 1:]).strip().strip('"')
                kw[kname] = v
            elif kname == "val":
                kw["val"] = _weight_to_item(arg[eq_idx + 1:])
            else:
                kw[kname] = _expr_text(arg[eq_idx + 1:])
        else:
            pos.append(arg)
    return pos, kw


def _parse_cmd(name, cmd, args):
    """根据命令名把参数 token 组装成统一配置项。"""
    pos, kw = _split_kw_args(args)

    def arity(lo, hi):
        if not (lo <= len(pos) <= hi):
            raise DslError(f"{cmd} 需要 {lo}~{hi} 个位置参数，实际 {len(pos)} 个")

    def weight(v=None):
        return v if v is not None else None

    if cmd == "int":
        arity(2, 2)
        return {"name": name, "kind": "int",
                "min": _expr_text(pos[0]), "max": _expr_text(pos[1])}
    if cmd == "float":
        arity(2, 3)
        return {"name": name, "kind": "float",
                "min": _expr_text(pos[0]), "max": _expr_text(pos[1]),
                "prec": kw.get("prec", _expr_text(pos[2]) if len(pos) == 3
                               else "6")}
    if cmd in ("ints", "floats"):
        arity(3, 4)
        return {"name": name, "kind": "array",
                "elem_type": "整数" if cmd == "ints" else "浮点数",
                "el_min": _expr_text(pos[1]), "el_max": _expr_text(pos[2]),
                "prec": kw.get("prec", _expr_text(pos[3]) if len(pos) == 4
                               else "6"),
                "rows": "1", "cols": _expr_text(pos[0])}
    if cmd in ("matrix", "matf"):
        arity(4, 5)
        return {"name": name, "kind": "array",
                "elem_type": "整数" if cmd == "matrix" else "浮点数",
                "el_min": _expr_text(pos[2]), "el_max": _expr_text(pos[3]),
                "prec": kw.get("prec", _expr_text(pos[4]) if len(pos) == 5
                               else "6"),
                "rows": _expr_text(pos[0]), "cols": _expr_text(pos[1])}
    if cmd == "perm":
        arity(1, 1)
        return {"name": name, "kind": "perm", "n": _expr_text(pos[0])}
    if cmd == "tree":
        arity(1, 2)
        item = {"name": name, "kind": "tree", "n": _expr_text(pos[0]),
                "w": None, "val": None}
        if len(pos) == 2:
            item["w"] = _weight_to_item(pos[1])
        if "val" in kw:
            item["val"] = kw["val"]
        if "w" in kw:
            item["w"] = _weight_to_item(
                tokenize(kw["w"])) if isinstance(kw["w"], str) else kw["w"]
        return item
    if cmd == "graph":
        arity(3, 5)
        gtype = kw.get("type", "general")
        directed = bool(int(_expr_text(pos[2]))) if len(pos) >= 3 else True
        connected = bool(int(_expr_text(pos[3]))) if len(pos) >= 4 else False
        if gtype == "dag":
            directed = True
        elif gtype == "bipartite":
            directed = False
        item = {"name": name, "kind": "graph",
                "n": _expr_text(pos[0]), "m": _expr_text(pos[1]),
                "directed": directed, "connected": connected,
                "gtype": gtype, "w": None, "val": None}
        if len(pos) == 5:
            item["w"] = _weight_to_item(pos[4])
        if "val" in kw:
            item["val"] = kw["val"]
        if "w" in kw:
            item["w"] = kw["w"] if isinstance(kw["w"], dict) else \
                _weight_to_item(tokenize(kw["w"]))
        return item
    if cmd in ("str", "strs"):
        arity(1, 3)
        charset = kw.get("charset", None)
        if charset is None:
            for p in pos[1:]:
                if _is_str_toks(p):
                    charset = _expr_text(p).strip('"')
                    pos = [p0 for p0 in pos if p0 is not p]
                    break
        if charset is None:
            charset = "abcdefghijklmnopqrstuvwxyz"
        if cmd == "str":
            arity(1, 1)
            return {"name": name, "kind": "string", "rows": "1",
                    "cols": _expr_text(pos[0]), "charset": charset}
        arity(2, 2)
        return {"name": name, "kind": "string",
                "rows": _expr_text(pos[0]), "cols": _expr_text(pos[1]),
                "charset": charset}
    if cmd == "binseq":
        arity(2, 2)
        return {"name": name, "kind": "binseq",
                "n": _expr_text(pos[0]), "k": _expr_text(pos[1])}
    if cmd == "intervals":
        arity(3, 3)
        return {"name": name, "kind": "intervals",
                "n": _expr_text(pos[0]), "lo": _expr_text(pos[1]),
                "hi": _expr_text(pos[2])}
    if cmd == "points":
        arity(5, 5)
        return {"name": name, "kind": "points",
                "n": _expr_text(pos[0]),
                "xlo": _expr_text(pos[1]), "xhi": _expr_text(pos[2]),
                "ylo": _expr_text(pos[3]), "yhi": _expr_text(pos[4])}
    if cmd == "ring":
        arity(1, 1)
        return {"name": name, "kind": "graph", "gtype": "ring",
                "n": _expr_text(pos[0]), "m": _expr_text(pos[0]),
                "directed": False, "connected": True, "w": None, "val": None}
    if cmd == "base_ring":
        arity(2, 2)
        return {"name": name, "kind": "graph", "gtype": "base_ring",
                "n": _expr_text(pos[0]), "m": _expr_text(pos[0]),
                "k": _expr_text(pos[1]),
                "directed": False, "connected": True, "w": None, "val": None}
    raise DslError(f"未知命令：{cmd}")


def _is_str_toks(toks):
    return len(toks) == 1 and toks[0][0] == "str"


_REPEAT_COMMENT = "多测模式"


def _parse_repeat(lines):
    """从 DSL 文本顶部读取多测模式注释，返回 {"enabled": bool, "count": str}
    或 None。识别形如“# 多测模式：重复 3 次”的注释行。"""
    for raw in lines[:8]:
        s = raw.strip()
        if not s.startswith("#"):
            continue
        body = s.lstrip("#").strip()
        if body == _REPEAT_COMMENT:
            return {"enabled": True, "count": "1"}
        if body.startswith(_REPEAT_COMMENT + "：") or \
                body.startswith(_REPEAT_COMMENT + ":"):
            rest = body.split("：", 1)[-1].split(":", 1)[-1].strip()
            m = re.fullmatch(r"重复\s*(\d+)\s*次?", rest)
            if m:
                return {"enabled": True, "count": m.group(1)}
    return None


def parse(text):
    """解析 DSL 文本，返回 (配置 dict, 错误信息或 None)。

    配置 dict 形如 {"repeat": {...} | None, "items": [...]}；repeat 来自
    顶部注释“# 多测模式：重复 N 次”。所有命令支持位置参数 + 关键字参数混用。"""
    lines = text.splitlines()
    repeat = _parse_repeat(lines)
    items = []
    seen = set()

    def parse_block(start, indent, outer_seen):
        block = []
        local_seen = set(outer_seen)
        i = start
        while i < len(lines):
            raw = lines[i]
            if not raw.strip() or raw.strip().startswith("#"):
                i += 1
                continue
            cur_indent = len(raw) - len(raw.lstrip(" "))
            if cur_indent < indent:
                break
            if cur_indent > indent:
                raise DslError(
                    f"第 {i + 1} 行：缩进不正确（不应超过 {indent}）")
            line = raw.strip()
            try:
                eq = line.find("=")
                if eq < 0:
                    raise DslError("语句缺少 '='")
                name = line[:eq].strip()
                rhs = line[eq + 1:].strip()
                if not name:
                    raise DslError("缺少变量名")
                if not _NAME_RE.match(name):
                    raise DslError(f"非法变量名：{name}")
                if name in local_seen:
                    raise DslError(f"变量名重复：{name}")
                if name in _KNOWN_COMMANDS:
                    raise DslError(f"变量名不能是保留字：{name}")
                toks = tokenize(rhs)
                if not toks or toks[0][0] != "name":
                    raise DslError("语句右侧必须是命令")
                cmd = toks[0][1]
                if cmd not in _KNOWN_COMMANDS:
                    raise DslError(f"未知命令：{cmd}")
                if len(toks) < 3 or not (toks[1][0] == "op"
                                         and toks[1][1] == "("):
                    raise DslError(f"{cmd} 命令缺少左括号")
                if not (toks[-1][0] == "op" and toks[-1][1] == ")"):
                    raise DslError(f"{cmd} 命令缺少右括号")
                item = _parse_cmd(name, cmd, toks[2:-1])
                block.append(item)
                local_seen.add(name)
                i += 1
            except DslError as e:
                raise DslError(f"第 {i + 1} 行：{e}")
        return block, i

    try:
        items, _ = parse_block(0, 0, seen)
    except DslError as e:
        return None, str(e)
    return {"repeat": repeat, "items": items}, None


# --------------------------------------------------------------------------- #
# 配置列表 -> DSL 文本
# --------------------------------------------------------------------------- #

def _fmt_range(item, key_lo, key_hi, is_int, prec_key=None):
    """把 min/max 字段格式化为 int(a,b) / float(a,b,prec) 形式。"""
    lo, hi = item[key_lo], item[key_hi]
    if is_int:
        return f"int({lo}, {hi})"
    prec = item.get(prec_key, "6")
    if str(prec) == "6":
        return f"float({lo}, {hi})"
    return f"float({lo}, {hi}, {prec})"


def _fmt_weight(w):
    """边权/节点权值描述 dict -> DSL 片段，如 int(1,10)。"""
    if w is None:
        return None
    return _fmt_range(w, "min", "max", w["kind"] == "int", "prec")


def _line_for(item):
    """把一个配置项序列化为一行 DSL 语句。"""
    name = item["name"]
    kind = item["kind"]
    if kind == "int":
        return f"{name} = int({item['min']}, {item['max']})"
    if kind == "float":
        return f"{name} = float({item['min']}, {item['max']}, {item['prec']})" \
            if str(item.get("prec", "6")) != "6" else \
            f"{name} = float({item['min']}, {item['max']})"
    if kind == "array":
        cmd = "ints" if item["elem_type"] == "整数" else "floats"
        rows = str(item.get("rows"))
        single_row = rows == "1" or re.fullmatch(r"\s*int\(\s*1\s*,\s*1\s*\)\s*", rows)
        if single_row:
            base = f"{name} = {cmd}({item['cols']}, {item['el_min']}, {item['el_max']}"
        else:
            cmd = "matrix" if item["elem_type"] == "整数" else "matf"
            base = (f"{name} = {cmd}({item['rows']}, {item['cols']}, "
                    f"{item['el_min']}, {item['el_max']}")
        if item["elem_type"] == "浮点数" and str(item.get("prec", "6")) != "6":
            base += f", {item['prec']}"
        return base + ")"
    if kind == "perm":
        return f"{name} = perm({item['n']})"
    if kind == "string":
        cmd = "str" if str(item.get("rows")) == "1" else "strs"
        if cmd == "str":
            base = f"{name} = str({item['cols']}"
        else:
            base = f"{name} = strs({item['rows']}, {item['cols']}"
        charset = item.get("charset", "")
        if charset and charset != "abcdefghijklmnopqrstuvwxyz":
            base += f', "{charset}"'
        return base + ")"
    if kind == "binseq":
        return f"{name} = binseq({item['n']}, {item['k']})"
    if kind == "intervals":
        return f"{name} = intervals({item['n']}, {item['lo']}, {item['hi']})"
    if kind == "points":
        return (f"{name} = points({item['n']}, {item['xlo']}, {item['xhi']}, "
                f"{item['ylo']}, {item['yhi']})")
    if kind == "tree":
        base = f"{name} = tree({item['n']}"
        w = item.get("w")
        if w:
            base += f", w={_fmt_weight(w)}"
        val = item.get("val")
        if val:
            base += f", val={_fmt_weight(val)}"
        return base + ")"
    if kind == "graph":
        gtype = item.get("gtype", "general")
        if gtype == "ring":
            return f"{name} = ring({item['n']})"
        if gtype == "base_ring":
            return f"{name} = base_ring({item['n']}, {item['k']})"
        d = 1 if item.get("directed") else 0
        c = 1 if item.get("connected") else 0
        base = f"{name} = graph({item['n']}, {item['m']}, {d}, {c}"
        if gtype != "general":
            base += f', type="{gtype}"'
        w = item.get("w")
        val = item.get("val")
        if w:
            base += f", w={_fmt_weight(w)}"
        if val:
            base += f", val={_fmt_weight(val)}"
        return base + ")"
    raise DslError(f"未知类型：{kind}")


def _item_lines(item):
    """把一个配置项序列化为一（或多）行 DSL。"""
    return [_line_for(item)]


def serialize(config):
    """把配置 dict 序列化为 DSL 文本（每行一条语句）。

    config 形如 {"repeat": {...} | None, "items": [...]}；勾选多测模式时
    首行输出“# 多测模式：重复 N 次”。"""
    out = []
    repeat = (config or {}).get("repeat")
    if repeat and repeat.get("enabled"):
        out.append(f"# {_REPEAT_COMMENT}：重复 {repeat.get('count', '1')} 次")
    for it in (config or {}).get("items", []):
        out += _item_lines(it)
    return "\n".join(out)
