//! 受限表达式：tokenizer + 递归下降解析 + 求值（移植 legacy/dsl.py，不使用 eval）。

use crate::error::{DslError, DslResult};

/// 词法 token。
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// 数字字面量（统一存 f64；序列化时按 Rust Display 规范化）
    Num(f64),
    /// 标识符（变量名 / 函数名）
    Name(String),
    /// 字符串字面量（字符集等参数用）
    Str(String),
    /// 运算符：`+ - * / // % ** ( ) = :`
    Op(String),
    /// 逗号
    Comma,
}

/// 把表达式切成 token 列表。无法识别的字符报错。
pub fn tokenize(src: &str) -> DslResult<Vec<Tok>> {
    let mut toks = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let len = bytes.len();
    while i < len {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'0'..=b'9' => {
                let start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < len && bytes[i] == b'.' {
                    i += 1;
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let text = &src[start..i];
                let v: f64 = text
                    .parse()
                    .map_err(|_| DslError::bare(format!("无法解析数字：{text}")))?;
                toks.push(Tok::Num(v));
            }
            b'.' => {
                // 仅当 . 后跟数字才算小数（与 legacy 正则 `\.\d+` 一致）
                if i + 1 < len && bytes[i + 1].is_ascii_digit() {
                    let start = i;
                    i += 1;
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    let text = &src[start..i];
                    let v: f64 = text
                        .parse()
                        .map_err(|_| DslError::bare(format!("无法解析数字：{text}")))?;
                    toks.push(Tok::Num(v));
                } else {
                    return Err(DslError::bare(format!("无法识别的字符：{}", src[i..].chars().next().unwrap())));
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < len
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                toks.push(Tok::Name(src[start..i].to_string()));
            }
            b'"' | b'\'' => {
                let quote = c;
                let start = i + 1;
                i += 1;
                let mut closed = false;
                while i < len {
                    let ch = bytes[i];
                    if ch == quote {
                        closed = true;
                        break;
                    }
                    if ch == b'\n' {
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    return Err(DslError::bare("字符串缺少结束引号"));
                }
                let s = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                toks.push(Tok::Str(s));
                i += 1;
            }
            b'*' => {
                if i + 1 < len && bytes[i + 1] == b'*' {
                    toks.push(Tok::Op("**".into()));
                    i += 2;
                } else {
                    toks.push(Tok::Op("*".into()));
                    i += 1;
                }
            }
            b'/' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    toks.push(Tok::Op("//".into()));
                    i += 2;
                } else {
                    toks.push(Tok::Op("/".into()));
                    i += 1;
                }
            }
            b'+' | b'-' | b'%' | b'(' | b')' | b'=' | b':' => {
                toks.push(Tok::Op((c as char).to_string()));
                i += 1;
            }
            b',' => {
                toks.push(Tok::Comma);
                i += 1;
            }
            other => {
                let ch = other as char;
                return Err(DslError::bare(format!("无法识别的字符：{ch}")));
            }
        }
    }
    Ok(toks)
}

/// 表达式 AST。
#[derive(Debug, Clone, PartialEq)]
pub enum ExprNode {
    Num(f64),
    Name(String),
    Str(String),
    Call { name: String, args: Vec<ExprNode> },
    Neg(Box<ExprNode>),
    Bin { op: BinOp, l: Box<ExprNode>, r: Box<ExprNode> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
}

/// 递归下降表达式解析器（产出小 AST）。
pub struct ExprParser<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl<'a> ExprParser<'a> {
    pub fn new(toks: &'a [Tok]) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos)
    }

    fn expect_op(&mut self, op: &str) -> bool {
        match self.peek() {
            Some(Tok::Op(s)) if s == op => {
                self.pos += 1;
                true
            }
            Some(Tok::Comma) if op == "," => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    pub fn parse(mut self) -> DslResult<ExprNode> {
        if self.toks.is_empty() {
            return Err(DslError::bare("空表达式"));
        }
        let node = self.parse_expr()?;
        if self.pos != self.toks.len() {
            return Err(DslError::bare("表达式末尾有多余内容"));
        }
        Ok(node)
    }

    fn parse_expr(&mut self) -> DslResult<ExprNode> {
        let mut node = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Tok::Op(s)) if s == "+" || s == "-" => {
                    let op = s.clone();
                    self.pos += 1;
                    let right = self.parse_term()?;
                    node = ExprNode::Bin {
                        op: if op == "+" { BinOp::Add } else { BinOp::Sub },
                        l: Box::new(node),
                        r: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_term(&mut self) -> DslResult<ExprNode> {
        let mut node = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(Tok::Op(s))
                    if s == "*" || s == "/" || s == "//" || s == "%" || s == "**" =>
                {
                    let op = match s.as_str() {
                        "*" => BinOp::Mul,
                        "/" => BinOp::Div,
                        "//" => BinOp::FloorDiv,
                        "%" => BinOp::Mod,
                        _ => BinOp::Pow,
                    };
                    self.pos += 1;
                    let right = self.parse_factor()?;
                    node = ExprNode::Bin {
                        op,
                        l: Box::new(node),
                        r: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(node)
    }

    fn parse_factor(&mut self) -> DslResult<ExprNode> {
        match self.peek() {
            Some(Tok::Op(s)) if s == "-" => {
                self.pos += 1;
                Ok(ExprNode::Neg(Box::new(self.parse_factor()?)))
            }
            Some(Tok::Op(s)) if s == "+" => {
                self.pos += 1;
                self.parse_factor()
            }
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> DslResult<ExprNode> {
        match self.peek() {
            None => Err(DslError::bare("表达式意外结束")),
            Some(Tok::Num(v)) => {
                let v = *v;
                self.pos += 1;
                Ok(ExprNode::Num(v))
            }
            Some(Tok::Str(s)) => {
                let s = s.clone();
                self.pos += 1;
                Ok(ExprNode::Str(s))
            }
            Some(Tok::Name(n)) => {
                let name = n.clone();
                self.pos += 1;
                // 函数调用：name( ... )
                if matches!(self.peek(), Some(Tok::Op(s)) if s == "(") {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::Op(s)) if s == ")") {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.expect_op(",") {
                                continue;
                            }
                            break;
                        }
                    }
                    if !self.expect_op(")") {
                        return Err(DslError::bare("缺少右括号"));
                    }
                    Ok(ExprNode::Call { name, args })
                } else {
                    Ok(ExprNode::Name(name))
                }
            }
            Some(Tok::Op(s)) if s == "(" => {
                self.pos += 1;
                let node = self.parse_expr()?;
                if !self.expect_op(")") {
                    return Err(DslError::bare("缺少右括号"));
                }
                Ok(node)
            }
            Some(Tok::Op(s)) => Err(DslError::bare(format!("表达式位置出现非法 token：{s}"))),
            Some(Tok::Comma) => Err(DslError::bare("表达式位置出现非法 token：,")),
        }
    }
}

/// 解析一个表达式字符串，返回 AST。语法错误附带表达式原文。
pub fn parse_expr(src: &str) -> DslResult<ExprNode> {
    let toks = tokenize(src)?;
    ExprParser::new(&toks)
        .parse()
        .map_err(|e| DslError::bare(format!("表达式 {src:?} 语法错误：{}", e.message)))
}

/// 求值 AST。
///
/// `env` 为可引用的变量环境；`rng` 供 `int(a,b)` / `float(a,b[,prec])` 随机取值，
/// 由调用方注入以便将来接入种子随机数。
pub fn eval_node(
    node: &ExprNode,
    env: &std::collections::HashMap<String, f64>,
    rng: &mut impl rand::Rng,
) -> DslResult<f64> {
    match node {
        ExprNode::Num(v) => Ok(*v),
        ExprNode::Name(name) => env
            .get(name)
            .copied()
            .ok_or_else(|| DslError::bare(format!("引用了未定义的变量：{name}"))),
        ExprNode::Str(_) => Err(DslError::bare("未知 AST 节点")),
        ExprNode::Neg(n) => Ok(-eval_node(n, env, rng)?),
        ExprNode::Bin { op, l, r } => {
            let a = eval_node(l, env, rng)?;
            let b = eval_node(r, env, rng)?;
            match op {
                BinOp::Add => Ok(a + b),
                BinOp::Sub => Ok(a - b),
                BinOp::Mul => Ok(a * b),
                BinOp::Div => Ok(a / b),
                BinOp::FloorDiv => Ok((a / b).floor()),
                // 与 Python 的 % 一致：a - floor(a/b)*b（Rust 原生 % 是截断取模）
                BinOp::Mod => Ok(a - (a / b).floor() * b),
                BinOp::Pow => Ok(a.powf(b)),
            }
        }
        ExprNode::Call { name, args } => {
            match name.as_str() {
                "int" => {
                    if args.len() != 2 {
                        return Err(DslError::bare("int(lo,hi) 需要两个参数"));
                    }
                    let lo = eval_node(&args[0], env, rng)? as i64;
                    let hi = eval_node(&args[1], env, rng)? as i64;
                    if lo > hi {
                        return Err(DslError::bare(format!("int 范围 {lo} > {hi}")));
                    }
                    Ok(rng.random_range(lo..=hi) as f64)
                }
                "float" => {
                    if !(2..=3).contains(&args.len()) {
                        return Err(DslError::bare(
                            "float(lo,hi[,prec]) 需要 2 或 3 个参数",
                        ));
                    }
                    let lo = eval_node(&args[0], env, rng)?;
                    let hi = eval_node(&args[1], env, rng)?;
                    if args.len() == 3 {
                        // 校验 prec 表达式合法性
                        eval_node(&args[2], env, rng)?;
                    }
                    if lo > hi {
                        return Err(DslError::bare(format!("float 范围 {lo} > {hi}")));
                    }
                    Ok(rng.random_range(lo..hi))
                }
                other => Err(DslError::bare(format!("未知函数调用：{other}"))),
            }
        }
    }
}

/// 解析并求值一个表达式字符串。语法错误会被包装为
/// `表达式 {src:?} 语法错误：...`，求值错误原样抛出。
pub fn eval_expr(
    src: &str,
    env: &std::collections::HashMap<String, f64>,
    rng: &mut impl rand::Rng,
) -> DslResult<f64> {
    let node = parse_expr(src)?;
    eval_node(&node, env, rng)
}
